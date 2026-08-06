# Bounded Posit RPN Calculator

An HP-48 style RPN calculator for **bounded posits**, running in the browser as WebAssembly.

**[Live demo →](https://tommythorn.github.io/bposit-calc/)**

The arithmetic is almost incidental. The point is the display: every value on the stack is shown
decomposed into its sign / regime / terminator / exponent / fraction fields, next to what the same
bit pattern would mean if the regime were *not* capped. It exists to make the behaviour of bounded
posits legible rather than to compute anything in particular.

## What a bounded posit is

An ordinary posit spends bits on a variable-length *regime* — a run of identical bits terminated by
their complement. A long run buys enormous dynamic range but starves the fraction, so accuracy
tapers away at the extremes and there is no lower bound on precision.

A **bounded posit** (b-posit) caps the regime run at `k_max + 1` bits. One rule follows, and it is
the only structural difference:

> When the regime run reaches the cap, the terminating bit is **suppressed** — there is nothing
> left to terminate, so that bit goes to the exponent instead.

That single rule puts a floor under the fraction width:

```
p_min = n - 1 - (k_max + 1) - es
```

You trade dynamic range for a guaranteed minimum precision and a uniform bound on relative error.

## The four standard formats

Parameters follow the reference definition in
[BPosits.jl](https://github.com/jamesquinlan/BPosits.jl): `es = min(4, n / 4)`, and
`k_max = 1, 7, 13, 19`.

| Format | n | es | k_max | p_min | maxpos | minpos | dynamic range |
|---|---|---|---|---|---|---|---|
| BPosit8 | 8 | 2 | 1 | 3 | 240 | 4.39e-3 | 4.7 decades |
| BPosit16 | 16 | 4 | 7 | 3 | 3.19e38 | 3.31e-39 | 77 decades |
| BPosit32 | 32 | 4 | 13 | 13 | 2.70e67 | 3.71e-68 | 135 decades |
| BPosit64 | 64 | 4 | 19 | 39 | 2.14e96 | 4.68e-97 | 193 decades |

BPosit8 is the clearest illustration: with `cap = 2`, the regime and terminator always consume
exactly two bits, so *every* value has exactly 3 fraction bits. Precision is completely uniform,
and doubling stays exact right up to the point it saturates.

Try `0x7F` in BPosit8. As a bounded posit it is **240** — the regime hit the cap, the terminator
was suppressed, `k = 1`, `E = 7`. Uncapped, the same eight bits would read as a regime run of 7,
`k = 6`, `E = 24`, and mean **16777216**, with no bits left for a fraction at all.

## Running it

```sh
./build.sh                  # wasm-pack build --target web --out-dir www/pkg
cd www && python3 -m http.server 8777
```

Then open <http://127.0.0.1:8777/>. A plain file:// open will not work — ES modules and the wasm
fetch both need a real origin.

The page is a fixed-viewport app rather than a scrolling document, and carries a web manifest, so
"Add to Home Screen" on iOS gives it a full-screen standalone window.

### Using it

Standard RPN. Type a number, `ENTER` to push, then an operator consumes the top of the stack.
Operations are `+ - x /`, `1/x`, `x·2`, `x/2`, and `+/-`.

Keyboard: digits, `.`, `e`, `+ - * /`, `Enter`, `Backspace` (drops when not typing), `n` negate,
`r` reciprocal, `d` double, `h` halve, `s` swap, `Esc` clear. Note `-` is always subtract; use `n`
to change sign.

Typing `0x7f` or `0b0110` pushes a **raw bit pattern** instead of a number, which is the fastest
way to go exploring the encoding.

Switching format converts the values on the stack rather than reinterpreting the bits, so you can
watch the same number re-encode itself as you move between formats.

## Correctness

The bit-field decoder is independent of the arithmetic, so the two check each other. `cargo test`
verifies, among other things:

- our decoder against `fast-posit`'s arithmetic **exhaustively** over the whole 8- and 16-bit
  spaces, and over 200k random 32-bit patterns;
- that `fast-posit`'s `RS` (max regime *bits*) equals the reference's `k_max + 1` (max regime
  *value*), which is the one place the two parameterisations could have been mismatched;
- exact decimal rounding against `fast-posit`'s own `f64` conversion, over ~9000 values per format;
- an exhaustive 16-bit decimal round-trip, and a BPosit64 round-trip through expansions hundreds of
  digits long;
- that bit patterns are monotonically ordered by value, which the encoder's binary search relies on;
- that nothing nonzero ever rounds to zero or to NaR.

### Why the decimal display avoids `f64`

A BPosit64 significand runs to 58 bits, against an `f64`'s 53. Empirically about 89% of BPosit64
values do not survive an `f64` round-trip, so routing the display through one would quietly print
the wrong number. Every posit is a dyadic rational and therefore has a terminating decimal
expansion, so the readout is computed exactly with big integers instead — as is decimal *input*,
which is rounded to the nearest posit without an `f64` intermediate. Where the exact expansion is
too long to lead with, the display marks the value `≈` and offers the full expansion.

## Licence

**LGPL-3.0**, because the arithmetic comes from
[fast-posit](https://crates.io/crates/fast-posit), which is LGPL-3.0, and is statically linked
into the wasm binary. See [LICENSE](LICENSE).
