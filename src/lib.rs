//! An HP-42S style RPN calculator over bounded posits, exposed to the browser via wasm-bindgen.
//!
//! The point of the thing is the display rather than the arithmetic: every stack entry is shown
//! decomposed into its sign / regime / terminator / exponent / fraction fields, alongside what the
//! same bit pattern would mean if the regime were *not* capped.

mod bignum;
mod bits;
mod decimal;
mod format;
mod rational;

use bits::{bitstring, Special};
use core::cmp::Ordering;
use format::{BinOp, Format, UnOp};
use rational::Rational;
use wasm_bindgen::prelude::*;

/// Significant digits shown in the main decimal readout.
const DISPLAY_SIG_DIGITS: usize = 20;

/// What produced the current level 1, kept so the inspector can show what was rounded away.
struct LastOp {
    /// Human-readable form of what was asked for, e.g. `60 + 2`.
    expr: String,
    /// The exact real result, before it was forced back into the format.
    exact: Rational,
    /// The bit pattern it rounded to.
    result: u64,
}

#[wasm_bindgen]
pub struct Calc {
    fmt: Format,
    /// The X, Y, Z, T registers, X first.
    ///
    /// A fixed four-register stack in the HP-42S manner: dropping replicates T and lifting pushes
    /// T off the end, so there is never too little or too much on it. No operation can fail for
    /// want of operands, which is why nothing here reports an error.
    regs: [u64; 4],
    /// HP's stack-lift flag.
    ///
    /// ENTER clears it, so the next number typed *replaces* X rather than pushing it up — which
    /// is what makes `2 ENTER 3 ×` work. Everything else sets it, so typing a digit after an
    /// operation lifts the result out of the way first.
    lift: bool,
    error: Option<String>,
    last: Option<LastOp>,
}

#[wasm_bindgen]
impl Calc {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Calc {
        Calc {
            fmt: Format::B16,
            regs: [0; 4],
            lift: true,
            error: None,
            last: None,
        }
    }

    /// Switch format, re-rounding every register into the new one.
    ///
    /// Values are converted rather than reinterpreted: the numbers you were working with stay the
    /// numbers you were working with, and you get to watch their encodings change.
    #[wasm_bindgen(js_name = setFormat)]
    pub fn set_format(&mut self, idx: u32) {
        let to = Format::from_index(idx);
        if to == self.fmt {
            return;
        }
        for v in self.regs.iter_mut() {
            *v = format::convert(self.fmt, to, *v);
        }
        self.fmt = to;
        self.error = None;
        // Every value was re-rounded, so the recorded operation no longer describes X.
        self.last = None;
    }

    #[wasm_bindgen(js_name = formatIndex)]
    pub fn format_index(&self) -> u32 {
        self.fmt.index()
    }

    /// Push X up into Y and put `bits` in X. T falls off the end.
    fn lift_in(&mut self, bits: u64) {
        self.regs = [bits, self.regs[0], self.regs[1], self.regs[2]];
    }

    /// Put `bits` in X, honouring the lift flag, then re-enable lift.
    fn place(&mut self, bits: u64) {
        if self.lift {
            self.lift_in(bits);
        } else {
            self.regs[0] = bits;
        }
        self.lift = true;
    }

    /// Drop the stack: Y becomes X and T *replicates* rather than emptying.
    ///
    /// The replication is not a detail. It is what lets T hold a constant across a run of
    /// operations: load it once, and every drop refills Z with it again.
    fn drop_stack(&mut self) {
        self.regs = [self.regs[1], self.regs[2], self.regs[3], self.regs[3]];
    }

    /// ENTER: duplicate X and suspend the lift.
    ///
    /// This is unconditional. Starting to type also lifts, but that push comes from the
    /// *previous* operation having enabled lift — it is not ENTER's. ENTER pushes on its own
    /// account, which is why `2 ENTER` leaves 2 in both X and Y, and why the suspended lift then
    /// makes the next digit overwrite the copy rather than pushing again.
    ///
    /// X is unchanged, so whatever the inspector was saying about it still holds.
    #[wasm_bindgen(js_name = enterKey)]
    pub fn enter_key(&mut self) {
        self.lift_in(self.regs[0]);
        self.lift = false;
    }

    /// Place a decimal literal in X, rounded exactly into the current format.
    #[wasm_bindgen(js_name = pushDecimal)]
    pub fn push_decimal(&mut self, s: &str) -> bool {
        match format::from_decimal(self.fmt, s) {
            Some(bits) => {
                self.place(bits);
                self.error = None;
                // Typing a literal is itself a rounding worth inspecting: 0.1 is not
                // representable in any of these formats.
                self.last = format::decimal_rational(s).map(|exact| LastOp {
                    expr: s.trim().to_string(),
                    exact,
                    result: bits,
                });
                true
            }
            None => {
                self.error = Some(format!("cannot parse `{s}` as a number"));
                false
            }
        }
    }

    /// Rebuild X from the literal currently being typed.
    ///
    /// Called on every keystroke: the number is built in X itself rather than in a separate entry
    /// area, as on the real machines. The first keystroke lifts the stack if lift is enabled;
    /// clearing the flag afterwards means later keystrokes rewrite X instead of pushing it up
    /// again. Re-parsing the whole literal each time keeps rounding from compounding.
    #[wasm_bindgen(js_name = typeX)]
    pub fn type_x(&mut self, s: &str) {
        // A half-written exponent or a lone sign is not yet a number; show what there is so far.
        let t = s.trim();
        let t = t.strip_suffix(['+', '-']).unwrap_or(t);
        let t = t.strip_suffix(['e', 'E']).unwrap_or(t);

        let is_bits = is_bit_literal(t);
        let bits = if t.is_empty() {
            Some(0)
        } else if is_bits {
            parse_bit_literal(t).map(|v| v & self.fmt.mask())
        } else {
            format::from_decimal(self.fmt, t)
        }
        .unwrap_or(0);

        self.place(bits);
        // Stay in entry: the next keystroke rewrites X rather than lifting again.
        self.lift = false;
        self.error = None;
        self.last = if t.is_empty() || is_bits {
            None
        } else {
            format::decimal_rational(t).map(|exact| LastOp {
                expr: t.to_string(),
                exact,
                result: bits,
            })
        };
    }

    /// Place a raw bit pattern in X: `0x` prefixed for hex, otherwise read as binary.
    #[wasm_bindgen(js_name = pushBits)]
    pub fn push_bits(&mut self, s: &str) -> bool {
        match parse_bit_literal(s) {
            Some(v) => {
                self.place(v & self.fmt.mask());
                self.error = None;
                // A raw pattern is not a rounding of anything.
                self.last = None;
                true
            }
            None => {
                self.error = Some(format!("cannot parse `{s}` as a bit pattern"));
                false
            }
        }
    }

    /// `X = Y op X`, dropping the stack.
    pub fn binary(&mut self, op: &str) {
        let op = match op {
            "add" => BinOp::Add,
            "sub" => BinOp::Sub,
            "mul" => BinOp::Mul,
            "div" => BinOp::Div,
            _ => {
                self.error = Some(format!("unknown operation `{op}`"));
                return;
            }
        };
        let (x, y) = (self.regs[0], self.regs[1]);
        let result = format::bin_op(self.fmt, op, y, x);
        self.drop_stack();
        self.regs[0] = result;
        self.lift = true;
        self.error = None;
        self.last = format::exact_bin(self.fmt, op, y, x).map(|exact| LastOp {
            expr: format!(
                "{} {} {}",
                self.decimal_of(y, 12),
                match op {
                    BinOp::Add => "+",
                    BinOp::Sub => "\u{2212}",
                    BinOp::Mul => "\u{00d7}",
                    BinOp::Div => "\u{00f7}",
                },
                self.decimal_of(x, 12)
            ),
            exact,
            result,
        });
    }

    /// `X = op X`.
    pub fn unary(&mut self, op: &str) {
        let op = match op {
            "neg" => UnOp::Neg,
            "recip" => UnOp::Recip,
            "double" => UnOp::Double,
            "half" => UnOp::Half,
            _ => {
                self.error = Some(format!("unknown operation `{op}`"));
                return;
            }
        };
        let x = self.regs[0];
        let result = format::un_op(self.fmt, op, x);
        self.regs[0] = result;
        self.lift = true;
        self.error = None;
        self.last = format::exact_un(self.fmt, op, x).map(|exact| LastOp {
            expr: match op {
                UnOp::Neg => format!("\u{2212}({})", self.decimal_of(x, 12)),
                UnOp::Recip => format!("1 \u{00f7} {}", self.decimal_of(x, 12)),
                UnOp::Double => format!("{} \u{00d7} 2", self.decimal_of(x, 12)),
                UnOp::Half => format!("{} \u{00f7} 2", self.decimal_of(x, 12)),
            },
            exact,
            result,
        });
    }

    /// Drop X; T replicates.
    #[wasm_bindgen(js_name = dropX)]
    pub fn drop_x(&mut self) {
        self.drop_stack();
        self.lift = true;
        self.error = None;
        self.last = None;
    }

    /// Exchange X and Y.
    pub fn swap(&mut self) {
        self.regs.swap(0, 1);
        self.lift = true;
        self.error = None;
        self.last = None;
    }

    /// Zero every register.
    pub fn clear(&mut self) {
        self.regs = [0; 4];
        self.lift = true;
        self.error = None;
        self.last = None;
    }

    /// Everything the UI needs, as JSON.
    #[wasm_bindgen(js_name = stateJson)]
    pub fn state_json(&self) -> String {
        let mut s = String::from("{\"format\":");
        s.push_str(&self.format_json());
        s.push_str(",\"stack\":[");
        // X first, matching how the display is read.
        for (i, bits) in self.regs.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&self.entry_json(*bits));
        }
        s.push(']');
        s.push_str(",\"lastOp\":");
        s.push_str(&self.last_op_json());
        s.push_str(",\"neighbours\":");
        s.push_str(&self.neighbours_json(self.regs[0]));
        s.push_str(",\"error\":");
        match &self.error {
            Some(e) => push_json_string(&mut s, e),
            None => s.push_str("null"),
        }
        s.push('}');
        s
    }

    /// The value of a bit pattern as decimal text, to `sig` significant digits.
    fn decimal_of(&self, bits: u64, sig: usize) -> String {
        let d = self.fmt.decode(bits);
        match d.special {
            Some(Special::Zero) => "0".to_string(),
            Some(Special::NaR) => "NaR".to_string(),
            None => {
                let sci = decimal::exact_sci(d.frac, d.total_exp - bits::HIDDEN_BIT as i64);
                decimal::render(d.neg, &sci, sig).0
            }
        }
    }

    /// `{decimal, bits}` for one neighbouring value.
    fn brief_json(&self, bits: u64) -> String {
        let mut s = String::from("{\"decimal\":");
        push_json_string(&mut s, &self.decimal_of(bits, DISPLAY_SIG_DIGITS));
        s.push_str(",\"bits\":");
        push_json_string(&mut s, &bitstring(bits, self.fmt.n()));
        s.push('}');
        s
    }

    /// The values either side of level 1, and the size of the steps to them.
    ///
    /// The gaps are what "precision" means concretely: they widen as the exponent grows, and the
    /// regime cap is what stops them widening without bound.
    fn neighbours_json(&self, bits: u64) -> String {
        let fmt = self.fmt;
        let Some(here) = format::rational_of(fmt, bits) else {
            return "null".to_string();
        };
        let mut s = String::from("{");
        for (i, (key, up)) in [("prior", false), ("next", true)].iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&format!("\"{key}\":"));
            match format::neighbour(fmt, bits, *up) {
                Some(nb) => s.push_str(&self.brief_json(nb)),
                None => s.push_str("null"),
            }
            s.push_str(&format!(
                ",\"{}\":",
                if *up { "gapAbove" } else { "gapBelow" }
            ));
            match format::neighbour(fmt, bits, *up).and_then(|nb| format::rational_of(fmt, nb)) {
                Some(other) => {
                    let gap = if *up {
                        other.sub(&here)
                    } else {
                        here.sub(&other)
                    };
                    push_json_string(&mut s, &gap.to_decimal(8).0);
                }
                None => s.push_str("null"),
            }
        }
        s.push('}');
        s
    }

    /// What the last operation produced exactly, and what rounding did to it.
    fn last_op_json(&self) -> String {
        let fmt = self.fmt;
        let Some(last) = &self.last else {
            return "null".to_string();
        };
        // Only describes X while X is still that result.
        if self.regs[0] != last.result {
            return "null".to_string();
        }
        let Some(rounded) = format::rational_of(fmt, last.result) else {
            return "null".to_string();
        };
        let exact = &last.exact;

        let mut s = String::from("{\"expr\":");
        push_json_string(&mut s, &last.expr);
        let (text, whole) = exact.to_decimal(DISPLAY_SIG_DIGITS);
        s.push_str(",\"exact\":");
        push_json_string(&mut s, &text);
        s.push_str(&format!(
            ",\"exactShown\":{},\"terminating\":{}",
            whole,
            exact.is_terminating()
        ));
        s.push_str(",\"rounded\":");
        push_json_string(&mut s, &self.decimal_of(last.result, DISPLAY_SIG_DIGITS));

        if exact.cmp(&rounded) == Ordering::Equal {
            s.push_str(",\"wasRounded\":false}");
            return s;
        }
        s.push_str(",\"wasRounded\":true");

        // The exact result lies between the answer and one of its neighbours; which side depends
        // on whether rounding went up or down.
        let below = exact.cmp(&rounded) == Ordering::Less;
        let (lo_bits, hi_bits) = if below {
            (
                format::neighbour(fmt, last.result, false),
                Some(last.result),
            )
        } else {
            (Some(last.result), format::neighbour(fmt, last.result, true))
        };

        match (lo_bits, hi_bits) {
            (Some(lo), Some(hi)) => {
                let (rlo, rhi) = (
                    format::rational_of(fmt, lo).unwrap(),
                    format::rational_of(fmt, hi).unwrap(),
                );
                let gap = rhi.sub(&rlo);
                let offset = exact.sub(&rlo);
                // An exact tie is offset·2 == gap, which is what sends the answer to the even
                // encoding rather than to the nearer neighbour.
                let tie = offset.mul(&Rational::from_int(2)).cmp(&gap) == Ordering::Equal;
                s.push_str(&format!(",\"saturated\":false,\"tie\":{tie}"));
                s.push_str(",\"position\":");
                match offset.div(&gap) {
                    Some(p) => push_json_string(&mut s, &p.to_decimal(6).0),
                    None => s.push_str("null"),
                }
                s.push_str(",\"lo\":");
                s.push_str(&self.brief_json(lo));
                s.push_str(",\"hi\":");
                s.push_str(&self.brief_json(hi));
            }
            // Ran off the end of the range: posits saturate rather than overflow.
            _ => s.push_str(",\"saturated\":true,\"tie\":false,\"position\":null"),
        }

        s.push_str(",\"relError\":");
        match exact.sub(&rounded).abs().div(&exact.abs()) {
            Some(rel) => push_json_string(&mut s, &rel.to_decimal(4).0),
            None => s.push_str("null"),
        }
        s.push('}');
        s
    }

    fn format_json(&self) -> String {
        let f = self.fmt;
        let dec = |bits: u64| {
            let d = f.decode(bits);
            let sci = decimal::exact_sci(d.frac, d.total_exp - bits::HIDDEN_BIT as i64);
            decimal::render(d.neg, &sci, 6).0
        };
        let mut s = String::new();
        s.push('{');
        s.push_str("\"name\":");
        push_json_string(&mut s, f.name());
        s.push_str(&format!(
            ",\"n\":{},\"es\":{},\"kmax\":{},\"cap\":{},\"pMin\":{}",
            f.n(),
            f.es(),
            f.kmax(),
            f.cap(),
            f.p_min()
        ));
        // useed = 2^(2^es), the factor one regime step is worth.
        s.push_str(&format!(",\"useed\":\"2^{}\"", 1u32 << f.es()));
        s.push_str(",\"max\":");
        push_json_string(&mut s, &dec(f.max_bits()));
        s.push_str(",\"minPositive\":");
        push_json_string(&mut s, &dec(f.min_positive_bits()));
        s.push('}');
        s
    }

    fn entry_json(&self, bits: u64) -> String {
        let f = self.fmt;
        let mut s = String::new();
        s.push('{');
        s.push_str("\"bits\":");
        push_json_string(&mut s, &bitstring(bits, f.n()));
        s.push_str(",\"hex\":");
        push_json_string(
            &mut s,
            &format!("0x{:0w$X}", bits, w = (f.n() as usize).div_ceil(4)),
        );
        s.push_str(",\"bounded\":");
        s.push_str(&self.view_json(bits, false));
        s.push_str(",\"unbounded\":");
        s.push_str(&self.view_json(bits, true));
        s.push('}');
        s
    }

    /// One interpretation of a bit pattern — either under this format's cap, or uncapped.
    fn view_json(&self, bits: u64, uncapped: bool) -> String {
        let f = self.fmt;
        let d = if uncapped {
            f.decode_unbounded(bits)
        } else {
            f.decode(bits)
        };

        let mut s = String::new();
        s.push('{');

        match d.special {
            Some(Special::Zero) => {
                s.push_str("\"special\":\"zero\",\"decimal\":\"0\",\"exactDecimal\":\"0\"");
                s.push_str(",\"decimalExact\":true");
            }
            Some(Special::NaR) => {
                s.push_str("\"special\":\"nar\",\"decimal\":\"NaR\",\"exactDecimal\":\"NaR\"");
                s.push_str(",\"decimalExact\":true");
            }
            None => {
                let sci = decimal::exact_sci(d.frac, d.total_exp - bits::HIDDEN_BIT as i64);
                let (shown, exact) = decimal::render(d.neg, &sci, DISPLAY_SIG_DIGITS);
                s.push_str("\"special\":null,\"decimal\":");
                push_json_string(&mut s, &shown);
                s.push_str(",\"exactDecimal\":");
                push_json_string(&mut s, &decimal::exact_string(d.neg, &sci));
                s.push_str(&format!(",\"decimalExact\":{}", exact));
            }
        }

        // Significand as a decimal, i.e. the `1.fff…` the fraction bits spell out.
        if d.special.is_none() {
            let sig = decimal::exact_sci(d.frac, -(bits::HIDDEN_BIT as i64));
            s.push_str(",\"significand\":");
            push_json_string(&mut s, &decimal::render(false, &sig, DISPLAY_SIG_DIGITS).0);
        } else {
            s.push_str(",\"significand\":null");
        }

        s.push_str(&format!(
            ",\"neg\":{},\"capped\":{},\"k\":{},\"e\":{},\"totalExp\":{}",
            d.neg, d.capped, d.k, d.e, d.total_exp
        ));
        s.push_str(&format!(
            ",\"fields\":{{\"sign\":{},\"regime\":{},\"term\":{},\"exp\":{},\"frac\":{}}}",
            if d.special.is_some() { 0 } else { 1 },
            d.regime_len,
            d.term_len,
            d.exp_len,
            d.frac_len
        ));
        s.push_str(",\"magnitudeBits\":");
        push_json_string(&mut s, &bitstring(d.magnitude, f.n()));
        s.push('}');
        s
    }
}

impl Default for Calc {
    fn default() -> Self {
        Self::new()
    }
}

/// Whether a literal is a raw bit pattern rather than a decimal number.
fn is_bit_literal(s: &str) -> bool {
    let t = s.trim();
    t.starts_with("0x") || t.starts_with("0X") || t.starts_with("0b") || t.starts_with("0B")
}

/// Parse `0x…` as hex, anything else as binary.
fn parse_bit_literal(s: &str) -> Option<u64> {
    let t = s.trim().replace('_', "");
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).ok()
    } else {
        let b = t
            .strip_prefix("0b")
            .or_else(|| t.strip_prefix("0B"))
            .unwrap_or(&t);
        u64::from_str_radix(b, 2).ok()
    }
}

/// Append `v` as a quoted, escaped JSON string.
fn push_json_string(out: &mut String, v: &str) {
    out.push('"');
    for c in v.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The UI consumes `stateJson`, so it must be valid JSON with the shape app.js expects.
    #[test]
    fn state_json_shape() {
        let mut c = Calc::new();
        c.set_format(0); // BPosit8
        assert!(c.push_decimal("240"));
        let s = c.state_json();
        // maxpos of BPosit8: regime run hits the cap, so no terminator and 3 fraction bits.
        assert!(s.contains("\"name\":\"BPosit8\""), "{s}");
        assert!(s.contains("\"capped\":true"), "{s}");
        assert!(s.contains("\"bits\":\"01111111\""), "{s}");
        assert!(s.contains("\"decimal\":\"240\""), "{s}");
        assert!(s.contains("\"term\":0"), "{s}");
        assert!(s.contains("\"error\":null"), "{s}");
    }

    #[test]
    fn rpn_flow() {
        let mut c = Calc::new();
        c.set_format(2); // BPosit32
        c.push_decimal("3");
        c.push_decimal("5");
        c.binary("add");
        assert!(c.state_json().contains("\"decimal\":\"8\""));
        c.unary("recip");
        assert!(c.state_json().contains("\"decimal\":\"0.125\""));
        c.unary("double");
        assert!(c.state_json().contains("\"decimal\":\"0.25\""));
    }

    #[test]
    fn operand_order() {
        let mut c = Calc::new();
        c.push_decimal("1");
        c.push_decimal("2");
        c.binary("sub"); // Y - X = 1 - 2
        assert_eq!(c.regs[0], from_decimal_bits(&c, "-1"));
    }

    fn from_decimal_bits(c: &Calc, s: &str) -> u64 {
        format::from_decimal(c.fmt, s).unwrap()
    }

    /// The four registers are always present, so nothing can under- or overflow.
    #[test]
    fn stack_is_always_four_deep() {
        let mut c = Calc::new();
        assert_eq!(c.regs, [0; 4]);

        for v in ["1", "2", "3", "4", "5"] {
            c.push_decimal(v);
        }
        // The fifth push pushed the first value off the end.
        let want = ["5", "4", "3", "2"].map(|v| from_decimal_bits(&c, v));
        assert_eq!(c.regs, want);

        // Dropping replicates T rather than running out, so the last value persists.
        let two = from_decimal_bits(&c, "2");
        for _ in 0..6 {
            c.drop_x();
        }
        assert_eq!(c.regs, [two; 4], "T replicates down the whole stack");
    }

    /// T replicating on a drop is what makes it usable as a constant register: load it once and
    /// every subsequent drop refills Z with it again.
    #[test]
    fn t_replicates_on_drop() {
        let mut c = Calc::new();
        c.set_format(2);
        for v in ["9", "3", "2", "1"] {
            c.push_decimal(v);
        }
        // X=1, Y=2, Z=3, T=9
        let (one, two, three, nine) = (
            from_decimal_bits(&c, "1"),
            from_decimal_bits(&c, "2"),
            from_decimal_bits(&c, "3"),
            from_decimal_bits(&c, "9"),
        );
        assert_eq!(c.regs, [one, two, three, nine]);

        c.drop_x();
        assert_eq!(c.regs, [two, three, nine, nine], "T copies down into Z");
        c.drop_x();
        assert_eq!(c.regs, [three, nine, nine, nine]);
        c.drop_x();
        assert_eq!(c.regs, [nine; 4], "and keeps refilling");
    }

    /// The classic use, and the exact sequence the README advertises: fill the stack with a
    /// constant and every drop refills Y with it again.
    #[test]
    fn t_acts_as_a_constant_through_repeated_operations() {
        let mut c = Calc::new();
        c.set_format(2);
        keys(&mut c, &["2", "ENTER", "ENTER", "ENTER"]);
        let two = from_decimal_bits(&c, "2");
        assert_eq!(c.regs, [two; 4], "the constant is in every register");

        for want in ["4", "8", "16"] {
            keys(&mut c, &["*"]);
            assert_eq!(c.regs[0], from_decimal_bits(&c, want));
            assert_eq!(c.regs[1], two, "T keeps refilling Y with the constant");
        }
    }

    /// With a fixed stack there are no operand-count failures left to report.
    #[test]
    fn operations_never_error() {
        let mut c = Calc::new();
        for op in ["add", "sub", "mul", "div"] {
            c.binary(op);
            assert!(c.state_json().contains("\"error\":null"), "{op}");
        }
        c.drop_x();
        c.swap();
        c.enter_key();
        assert!(c.state_json().contains("\"error\":null"));
    }

    /// A binary op consumes X and Y, drops the stack, and T replicates.
    #[test]
    fn binary_drops_the_stack() {
        let mut c = Calc::new();
        for v in ["7", "5", "3", "2"] {
            c.push_decimal(v);
        }
        // regs are X=2, Y=3, Z=5, T=7
        c.binary("add"); // 3 + 2
        let want = ["5", "5", "7", "7"].map(|v| from_decimal_bits(&c, v));
        assert_eq!(c.regs, want);
    }

    /// Drive the calculator the way the keypad does, one keystroke at a time.
    ///
    /// `type_x` is called per character because that is what the UI does; a test that pushed
    /// whole values instead would not have caught ENTER failing to duplicate mid-entry.
    fn keys(c: &mut Calc, seq: &[&str]) {
        let mut entry = String::new();
        for k in seq {
            match *k {
                "ENTER" => {
                    entry.clear();
                    c.enter_key();
                }
                "+" | "-" | "*" | "/" => {
                    entry.clear();
                    c.binary(match *k {
                        "+" => "add",
                        "-" => "sub",
                        "*" => "mul",
                        _ => "div",
                    });
                }
                "SWAP" => {
                    entry.clear();
                    c.swap();
                }
                digits => {
                    for ch in digits.chars() {
                        entry.push(ch);
                        c.type_x(&entry);
                    }
                }
            }
        }
    }

    /// ENTER duplicates X and suspends the lift, so the next number typed replaces the copy and
    /// leaves the original in Y. This is what makes `2 ENTER 3 x` work.
    #[test]
    fn enter_duplicates_then_the_next_number_overwrites() {
        let mut c = Calc::new();
        keys(&mut c, &["2", "ENTER"]);
        let two = from_decimal_bits(&c, "2");
        assert_eq!(c.regs[0], two, "X keeps the value");
        assert_eq!(c.regs[1], two, "and Y gets a copy");
        assert!(!c.lift, "ENTER suspends the lift");

        keys(&mut c, &["3"]);
        assert_eq!(c.regs[0], from_decimal_bits(&c, "3"));
        assert_eq!(c.regs[1], two, "the original stays in Y");

        keys(&mut c, &["*"]);
        assert_eq!(c.regs[0], from_decimal_bits(&c, "6"));
    }

    /// The whole point of ENTER duplicating: `2 ENTER +` doubles.
    #[test]
    fn enter_then_an_operator_uses_the_copy() {
        let mut c = Calc::new();
        keys(&mut c, &["2", "ENTER", "+"]);
        assert_eq!(c.regs[0], from_decimal_bits(&c, "4"));
    }

    /// Full keystroke sequences, checked end to end.
    #[test]
    fn keystroke_sequences() {
        let cases: &[(&[&str], &str)] = &[
            (&["2", "ENTER", "3", "*"], "6"),
            (&["2", "ENTER", "3", "+"], "5"),
            (&["10", "ENTER", "4", "-"], "6"),
            (&["12", "ENTER", "4", "/"], "3"),
            // Repeated ENTER stacks copies.
            (&["7", "ENTER", "ENTER", "+"], "14"),
            // Typing after an operator lifts the result rather than replacing it.
            (&["2", "ENTER", "3", "+", "4", "*"], "20"),
            (&["1", "ENTER", "2", "SWAP", "-"], "1"),
        ];
        for (seq, want) in cases {
            let mut c = Calc::new();
            c.set_format(2);
            keys(&mut c, seq);
            assert_eq!(
                c.regs[0],
                from_decimal_bits(&c, want),
                "{seq:?} should give {want}"
            );
        }
    }

    /// After anything other than ENTER, typing lifts the previous value out of the way.
    #[test]
    fn typing_after_an_operation_lifts() {
        let mut c = Calc::new();
        keys(&mut c, &["2", "ENTER", "3", "+"]); // X = 5, and the lift is re-enabled
        assert!(c.lift);

        keys(&mut c, &["4"]);
        assert_eq!(c.regs[0], from_decimal_bits(&c, "4"));
        assert_eq!(
            c.regs[1],
            from_decimal_bits(&c, "5"),
            "the result was pushed up"
        );
    }

    /// Successive keystrokes rewrite X rather than lifting again, and re-parse the whole literal
    /// so rounding does not compound.
    #[test]
    fn typing_builds_in_x_without_repeated_lifts() {
        let mut c = Calc::new();
        c.set_format(2);
        c.push_decimal("9");
        for prefix in ["1", "1.", "1.2", "1.25"] {
            c.type_x(prefix);
        }
        assert_eq!(c.regs[0], from_decimal_bits(&c, "1.25"));
        assert_eq!(
            c.regs[1],
            from_decimal_bits(&c, "9"),
            "only one lift happened"
        );
        assert_eq!(c.regs[2], 0);
    }

    /// Half-written literals are shown as far as they go rather than collapsing to zero.
    #[test]
    fn partial_literals_are_tolerated() {
        let mut c = Calc::new();
        c.set_format(2);
        c.type_x("");
        assert_eq!(c.regs[0], 0);
        c.type_x("1");
        c.type_x("1e");
        assert_eq!(
            c.regs[0],
            from_decimal_bits(&c, "1"),
            "a dangling exponent is ignored"
        );
        c.type_x("1e-");
        assert_eq!(c.regs[0], from_decimal_bits(&c, "1"));
        c.type_x("1e-2");
        assert_eq!(c.regs[0], from_decimal_bits(&c, "0.01"));
    }

    #[test]
    fn bit_entry() {
        let mut c = Calc::new();
        c.set_format(0);
        assert!(c.push_bits("0x7f"));
        assert!(c.state_json().contains("\"decimal\":\"240\""));
        c.clear();
        assert!(c.push_bits("0b01000000"));
        assert!(c.state_json().contains("\"decimal\":\"1\""));
    }

    /// The inspector must explain the tie that makes 60 + 2 and 64 - 2 both land on 64.
    #[test]
    fn inspector_reports_an_exact_tie() {
        let mut c = Calc::new();
        c.set_format(0); // BPosit8
        c.push_decimal("60");
        c.push_decimal("2");
        c.binary("add");
        let s = c.state_json();
        assert!(s.contains("\"exact\":\"62\""), "{s}");
        assert!(s.contains("\"rounded\":\"64\""), "{s}");
        assert!(s.contains("\"tie\":true"), "{s}");
        assert!(s.contains("\"position\":\"0.5\""), "{s}");
        assert!(s.contains("\"saturated\":false"), "{s}");
    }

    /// An exactly representable result must not be dressed up as a rounding.
    #[test]
    fn inspector_reports_exact_results() {
        let mut c = Calc::new();
        c.set_format(2);
        c.push_decimal("3");
        c.push_decimal("5");
        c.binary("add");
        let s = c.state_json();
        assert!(s.contains("\"wasRounded\":false"), "{s}");
        assert!(s.contains("\"exact\":\"8\""), "{s}");
    }

    /// 1/3 has no finite expansion, so the exact value must be flagged as non-terminating rather
    /// than presented as if the shown digits were all of it.
    #[test]
    fn inspector_flags_non_terminating_results() {
        let mut c = Calc::new();
        c.set_format(0);
        c.push_decimal("1");
        c.push_decimal("3");
        c.binary("div");
        let s = c.state_json();
        assert!(s.contains("\"terminating\":false"), "{s}");
        assert!(s.contains("\"exactShown\":false"), "{s}");
        assert!(s.contains("\"exact\":\"0.33333333333333333333\""), "{s}");
    }

    /// Past maxpos there is no bracketing pair, so it must be reported as saturation.
    #[test]
    fn inspector_reports_saturation() {
        let mut c = Calc::new();
        c.set_format(0);
        c.push_decimal("200");
        c.push_decimal("200");
        c.binary("mul");
        let s = c.state_json();
        assert!(s.contains("\"saturated\":true"), "{s}");
        assert!(s.contains("\"position\":null"), "{s}");
        assert!(s.contains("\"rounded\":\"240\""), "{s}");
    }

    /// Typing a literal is itself a rounding, and worth inspecting.
    #[test]
    fn inspector_covers_entry_rounding() {
        let mut c = Calc::new();
        c.set_format(3); // BPosit64
        c.push_decimal("0.1");
        let s = c.state_json();
        assert!(s.contains("\"expr\":\"0.1\""), "{s}");
        assert!(s.contains("\"exact\":\"0.1\""), "{s}");
        assert!(s.contains("\"wasRounded\":true"), "{s}");
    }

    /// The inspector describes level 1; once level 1 is something else it must go quiet rather
    /// than keep explaining a value that is no longer there.
    #[test]
    fn inspector_clears_when_it_no_longer_applies() {
        let mut c = Calc::new();
        c.set_format(0);
        c.push_decimal("60");
        c.push_decimal("2");
        c.binary("add");
        assert!(c.state_json().contains("\"tie\":true"));

        c.drop_x();
        assert!(c.state_json().contains("\"lastOp\":null"));

        // A format switch re-rounds everything, so the recorded operation no longer applies.
        c.push_decimal("60");
        c.push_decimal("2");
        c.binary("add");
        c.set_format(2);
        assert!(c.state_json().contains("\"lastOp\":null"));
    }

    /// Neighbours bound the value on both sides, except at the ends of the range.
    #[test]
    fn neighbours_are_reported() {
        let mut c = Calc::new();
        c.set_format(0);
        c.push_decimal("1");
        let s = c.state_json();
        assert!(s.contains("\"gapBelow\":\"0.0625\""), "{s}");
        assert!(s.contains("\"gapAbove\":\"0.125\""), "{s}");

        // maxpos has nothing above it.
        c.clear();
        c.push_bits("0x7f");
        let s = c.state_json();
        assert!(s.contains("\"next\":null"), "{s}");
        assert!(s.contains("\"gapAbove\":null"), "{s}");
    }

    /// Switching format must preserve values, not bit patterns.
    #[test]
    fn format_switch_converts_values() {
        let mut c = Calc::new();
        c.set_format(0);
        c.push_decimal("0.5");
        c.set_format(3); // BPosit64
        let s = c.state_json();
        assert!(s.contains("\"decimal\":\"0.5\""), "{s}");
        assert!(s.contains("\"name\":\"BPosit64\""), "{s}");
    }
}
