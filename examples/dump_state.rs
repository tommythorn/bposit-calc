//! Dumps `stateJson` for a few representative stacks, so the browser-side rendering can be
//! exercised without a browser.

use bposit_calc::Calc;

fn main() {
    let mut cases: Vec<(String, String)> = Vec::new();

    // A capped regime in BPosit8: maxpos, where the terminator is suppressed.
    let mut c = Calc::new();
    c.set_format(0);
    c.push_decimal("240");
    c.push_decimal("-0.5");
    c.push_decimal("1");
    cases.push(("bposit8-capped".into(), c.state_json()));

    // An ordinary BPosit16 value, regime well short of the cap.
    let mut c = Calc::new();
    c.set_format(1);
    c.push_decimal("3.25");
    cases.push(("bposit16-plain".into(), c.state_json()));

    // BPosit64 with a long exact expansion.
    let mut c = Calc::new();
    c.set_format(3);
    c.push_decimal("0.1");
    cases.push(("bposit64-tenth".into(), c.state_json()));

    // Exception values.
    let mut c = Calc::new();
    c.set_format(1);
    c.push_decimal("0");
    c.push_decimal("1");
    c.push_decimal("0");
    c.binary("div"); // 1/0 = NaR
    cases.push(("nar-and-zero".into(), c.state_json()));

    // The tie that prompted the inspector: 60 + 2 in BPosit8.
    let mut c = Calc::new();
    c.set_format(0);
    c.push_decimal("60");
    c.push_decimal("2");
    c.binary("add");
    cases.push(("tie-60-plus-2".into(), c.state_json()));

    // A non-terminating exact result.
    let mut c = Calc::new();
    c.set_format(0);
    c.push_decimal("1");
    c.push_decimal("3");
    c.binary("div");
    cases.push(("one-third".into(), c.state_json()));

    // Saturation past maxpos.
    let mut c = Calc::new();
    c.set_format(0);
    c.push_decimal("200");
    c.push_decimal("200");
    c.binary("mul");
    cases.push(("saturate".into(), c.state_json()));

    // An error state.
    let mut c = Calc::new();
    c.binary("add");
    cases.push(("error".into(), c.state_json()));

    print!("{{");
    for (i, (name, json)) in cases.iter().enumerate() {
        if i > 0 {
            print!(",");
        }
        print!("\"{name}\":{json}");
    }
    println!("}}");
}
