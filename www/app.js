import init, { Calc } from './pkg/bposit_calc.js';

const FORMAT_NAMES = ['BPosit8', 'BPosit16', 'BPosit32', 'BPosit64'];

/** The stack is the four HP registers, always all of them. */
const LEVELS = 4;

let calc;
/** The literal currently being typed. Empty means "not typing". */
let entry = '';

const $ = (id) => document.getElementById(id);

const esc = (s) =>
  String(s).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));

/** Significant digits in an exact expansion, ignoring sign, point and leading zeros. */
const countDigits = (s) => s.replace(/[-.]/g, '').replace(/^0+/, '').length;

// ---------------------------------------------------------------------------- entry handling

/** True when the entry is a raw bit pattern rather than a decimal literal. */
const isBitEntry = (s = entry) => /^0[xXbB]/.test(s);

/**
 * Whether `c` may be appended, so the entry line can never hold something unparseable.
 *
 * With a four-register stack no operation can fail, and guarding entry here is what makes the
 * "no errors, ever" property hold for typing too.
 */
function canType(c) {
  // Hex digits belong to `0x...`; `0b...` takes only bits.
  if (isBitEntry()) return /^0[xX]/.test(entry) ? /^[0-9a-fA-F]$/.test(c) : /^[01]$/.test(c);
  if (/^[0-9]$/.test(c)) return true;
  const [mantissa, exponent] = entry.split(/[eE]/);
  if (c === '.') return exponent === undefined && !mantissa.includes('.');
  if (c === 'e' || c === 'E') return exponent === undefined && /[0-9]/.test(entry);
  if (c === '-' || c === '+') return exponent === '';
  if (c === 'x' || c === 'X' || c === 'b' || c === 'B') return entry === '0';
  return false;
}

/** The digits are already in X, so finishing entry is just forgetting the text. */
function endEntry() {
  entry = '';
}

function doBinary(op) {
  endEntry();
  calc.binary(op);
  render();
}

function doUnary(op) {
  endEntry();
  calc.unary(op);
  render();
}

function doAction(act) {
  switch (act) {
    case 'enter':
      // ENTER always duplicates X, whether or not digits were being typed. The lift that happens
      // when you start typing is a separate push, caused by the *previous* operation enabling
      // lift; ENTER's own push is what puts the copy in Y. So `2 ENTER` leaves 2 in both X and Y,
      // and the suspended lift makes the next digit overwrite the copy.
      endEntry();
      calc.enterKey();
      break;
    case 'chs':
      // While typing, +/- flips the sign of the literal; otherwise it negates level 1.
      if (entry !== '' && !isBitEntry()) {
        entry = entry.startsWith('-') ? entry.slice(1) : '-' + entry;
        calc.typeX(entry);
      } else {
        endEntry();
        calc.unary('neg');
      }
      break;
    case 'back':
      // While typing, back up a character; the last one leaves X at zero. Otherwise drop X.
      if (entry !== '') {
        entry = entry.slice(0, -1);
        calc.typeX(entry);
      } else {
        calc.dropX();
      }
      break;
    case 'swap':
      endEntry();
      calc.swap();
      break;
    case 'clear':
      entry = '';
      calc.clear();
      break;
  }
  render();
}

function typeChar(c) {
  if (!canType(c)) return;
  entry += c;
  calc.typeX(entry);
  render();
}

// ---------------------------------------------------------------------------- rendering

/**
 * Split a bit string into its coloured field segments.
 *
 * When the regime run hits the cap there is no terminator bit to show, so a ghost slot is
 * emitted in its place — that suppressed bit is the whole difference between a bounded posit
 * and an ordinary one.
 */
function segments(bitstr, v) {
  if (v.special) {
    return [{ text: bitstr, cls: 'f-none', label: v.special === 'nar' ? 'NaR' : 'zero' }];
  }
  const f = v.fields;
  const segs = [];
  let p = 0;
  const take = (n, cls, label) => {
    if (n <= 0) return;
    segs.push({ text: bitstr.slice(p, p + n), cls, label });
    p += n;
  };

  take(f.sign, 'f-sign', 'sign');
  take(f.regime, 'f-regime', `regime  k=${v.k >= 0 ? '+' : ''}${v.k}`);
  if (f.term > 0) {
    take(f.term, 'f-term', 'term');
  } else if (v.capped) {
    segs.push({ ghost: true, cls: 'f-term', label: 'terminator suppressed' });
  }
  take(f.exp, 'f-exp', `exp  e=${v.e}`);
  take(f.frac, 'f-frac', `frac  ${f.frac} bit${f.frac === 1 ? '' : 's'}`);
  return segs;
}

/** Field-coloured bits with labels underneath. */
function bitFieldsHtml(bitstr, v) {
  const segs = segments(bitstr, v);
  const groups = segs
    .map((s) => {
      if (s.ghost) {
        return `<div class="fieldgroup ghost"><div class="bits"> </div><div class="label">${esc(
          s.label
        )}</div></div>`;
      }
      return `<div class="fieldgroup ${s.cls}"><div class="bits">${esc(
        s.text
      )}</div><div class="label">${esc(s.label)}</div></div>`;
    })
    .join('');
  return `<div class="scroller"><div class="bitfields">${groups}</div></div>`;
}

/** Compact coloured bits, no labels — used in the stack rows. */
function bitRowHtml(bitstr, v) {
  return segments(bitstr, v)
    .map((s) => (s.ghost ? '' : `<span class="${s.cls}">${esc(s.text)}</span>`))
    .join('');
}

function renderFacts(fmt) {
  $('facts').innerHTML = `
    <span><b>${esc(fmt.name)}</b></span>
    <span>n = <b>${fmt.n}</b></span>
    <span>es = <b>${fmt.es}</b></span>
    <span>k_max = <b>${fmt.kmax}</b></span>
    <span>regime cap = <b>${fmt.cap}</b> bits</span>
    <span>fraction floor p_min = <b>${fmt.pMin}</b> bits</span>
    <span>useed = <b>${esc(fmt.useed)}</b></span>
    <span>range <b>&plusmn;${esc(fmt.minPositive)}</b> &hellip; <b>&plusmn;${esc(fmt.max)}</b></span>`;
}

function renderStack(stack) {
  let html = '';
  // X is level 1 and is drawn last, at the bottom.
  for (let lvl = LEVELS; lvl >= 1; lvl--) {
    const e = stack[lvl - 1];
    const v = e.bounded;
    // While typing, X shows the digits as entered rather than the value they rounded to --
    // otherwise typing "0.1" in BPosit8 would replace itself with 0.099609375 mid-keystroke.
    const typing = lvl === 1 && entry !== '';
    const shown = typing
      ? `${esc(entry)}<span class="caret"></span>`
      : `${esc(v.decimal)}`;
    html += `<div class="level${typing ? ' typing' : ''}">
      <span class="tag">${lvl}:</span>
      <span class="val${!typing && !v.decimalExact ? ' approx' : ''}">${shown}</span>
      <span class="rowbits mono">${bitRowHtml(e.bits, v)}</span>
    </div>`;
  }
  $('stack').innerHTML = html;
}

/**
 * Parity of an encoding, which is what decides a tie: the pattern ending in 0 wins.
 */
const parity = (bits) => (bits.endsWith('1') ? 'odd' : 'even');

/**
 * What the last operation produced before rounding, and where it landed.
 *
 * The bar is the interval between the two representable values that bracket the exact result;
 * the tick at the centre is the tie point, which is where the even-encoding rule takes over from
 * "round to the nearer one".
 */
function renderRounding(last) {
  if (!last) return '';

  let h = `<div class="rounding"><h2>Rounding</h2>
    <div class="roundrow"><span class="rk">asked</span><span class="rv mono">${esc(
      last.expr
    )}</span></div>
    <div class="roundrow"><span class="rk">exact</span><span class="rv mono">${
      last.exactShown ? '' : '&approx; '
    }${esc(last.exact)}${last.terminating ? '' : '&hellip;'}</span></div>`;

  if (!last.wasRounded) {
    h += `<div class="verdict">Exactly representable &mdash; nothing was lost.</div></div>`;
    return h;
  }

  h += `<div class="roundrow"><span class="rk">stored</span><span class="rv mono">${esc(
    last.rounded
  )}</span></div>`;

  if (last.saturated) {
    h += `<div class="verdict differs">Past the end of the format's range, so it saturated to the
      extreme value. Posits have no infinity to overflow to.</div>`;
  } else {
    const frac = parseFloat(last.position);
    const pct = Math.round(frac * 1000) / 10;
    h += `<div class="ulpbar" style="--p:${(frac * 100).toFixed(4)}%">
        <div class="tick" title="tie point"></div><div class="mark"></div>
      </div>
      <div class="ulpends mono">
        <span>${esc(last.lo.decimal)}<br><span class="par">${esc(last.lo.bits)} &middot; ${parity(
      last.lo.bits
    )}</span></span>
        <span>${esc(last.hi.decimal)}<br><span class="par">${esc(last.hi.bits)} &middot; ${parity(
      last.hi.bits
    )}</span></span>
      </div>`;
    h += last.tie
      ? `<div class="verdict differs"><b>Exact tie.</b> The result fell precisely halfway between
         two representable values, so "the nearer one" does not exist. The tie goes to the
         <b>even encoding</b> &mdash; the ${parity(last.lo.bits) === 'even' ? 'left' : 'right'}
         one above, whose bit pattern ends in 0. That rule is what keeps repeated rounding from
         drifting in one direction.</div>`
      : `<div class="verdict">Landed ${pct}% of the way along, so it rounded to the nearer end.</div>`;
    h += `<div class="roundrow"><span class="rk">error</span><span class="rv mono">${esc(
      last.relError
    )} relative</span></div>`;
  }

  return h + `</div>`;
}

/**
 * The representable values either side of level 1. The gaps are what precision *is*: they grow
 * with the exponent, and the regime cap is what stops them growing without bound.
 */
function renderNeighbours(nb, self) {
  if (!nb) return '';
  const row = (label, v, gap, cls) => {
    if (!v) {
      return `<div class="nrow ${cls}"><span class="nk">${label}</span>
        <span class="nv end">none &mdash; end of the range</span><span class="ng"></span></div>`;
    }
    return `<div class="nrow ${cls}"><span class="nk">${label}</span>
      <span class="nv mono">${esc(v.decimal)}</span>
      <span class="ng mono">${gap ? esc(gap) : ''}</span></div>`;
  };
  return `<div class="neighbours"><h2>Neighbouring values</h2>
    ${row('next', nb.next, nb.gapAbove ? '+' + nb.gapAbove : '', '')}
    ${row('this', { decimal: self }, '', 'self')}
    ${row('prior', nb.prior, nb.gapBelow ? '\u2212' + nb.gapBelow : '', '')}
  </div>`;
}

function renderAnatomy(entryData, fmt, lastOp, neighbours) {
  const el = $('anatomy');
  if (!entryData) {
    el.innerHTML = `<h2>Anatomy</h2><p class="placeholder">Enter a number to see how it is encoded.</p>`;
    return;
  }

  const v = entryData.bounded;
  const u = entryData.unbounded;
  const twosComplement = entryData.bits !== v.magnitudeBits;

  let html = `<h2>Level 1 &mdash; ${esc(fmt.name)}</h2>
    <div class="headline">${v.decimalExact ? '' : '&approx; '}${esc(v.decimal)}</div>
    <div class="sub">${esc(entryData.hex)}
      ${v.decimalExact ? '' : '&middot; shown to 20 significant digits'}
    </div>`;

  // Every finite posit is a dyadic rational, so it always has an exact terminating expansion —
  // it is just too long to lead with.
  if (!v.decimalExact) {
    html += `<details class="exact">
      <summary>exact value (${countDigits(v.exactDecimal)} digits)</summary>
      <div class="exactline mono">${esc(v.exactDecimal)}</div>
    </details>`;
  }

  html += renderRounding(lastOp);
  html += bitFieldsHtml(entryData.bits, v);

  // A negative posit's fields are defined on its two's complement, so the run of identical bits
  // the regime describes is not visible in the stored pattern at all. Show both.
  if (twosComplement) {
    html += `<p class="capnote inactive">Negative: the pattern above is what is stored, carved at
      the field widths its magnitude implies. The fields are actually defined on the
      two's-complement magnitude below, which is where the regime run is legible.</p>`;
    html += bitFieldsHtml(v.magnitudeBits, v);
  }

  // The regime cap: the one rule that separates a b-posit from a posit.
  if (v.special) {
    html += `<p class="capnote inactive">${
      v.special === 'nar' ? 'NaR' : 'Zero'
    } is an exception encoding &mdash; it has no fields.</p>`;
  } else if (v.capped) {
    html += `<p class="capnote"><b>Regime capped.</b> The run reached k_max = ${fmt.kmax}
      (${fmt.cap} bits), so the terminating bit is <b>suppressed</b> &mdash; there is nothing left
      to terminate, and that bit goes to the exponent instead. This is what puts a floor of
      ${fmt.pMin} fraction bits under every value.</p>`;
  } else {
    const room = fmt.cap - v.fields.regime;
    html += `<p class="capnote inactive">Regime run is ${v.fields.regime} bit${
      v.fields.regime === 1 ? '' : 's'
    }, ${room} short of the cap of ${fmt.cap}, so a terminating bit is stored normally.</p>`;
  }

  if (!v.special) {
    html += `<div class="decomp">
      <div>regime <span class="eq">k = ${v.k}</span>,
           exponent field <span class="eq">e = ${v.e}</span>,
           so <span class="eq">E = k&middot;2<sup>${fmt.es}</sup> + e = ${v.k}&middot;${
      1 << fmt.es
    } + ${v.e} = ${v.totalExp}</span></div>
      <div>significand <span class="eq">${esc(v.significand)}</span>
           (${v.fields.frac} fraction bit${v.fields.frac === 1 ? '' : 's'}${
      v.fields.frac === fmt.pMin ? ' &mdash; at the guaranteed floor' : ''
    })</div>
      <div>value <span class="eq">= ${v.neg ? '&minus;' : '+'}${esc(
      v.significand
    )} &times; 2<sup>${v.totalExp}</sup></span></div>
    </div>`;
  }

  html += renderNeighbours(neighbours, v.decimal);

  // The unbounded shadow: same bits, no cap.
  html += `<div class="shadow">
    <h2>Same bits, uncapped regime</h2>
    <div class="sub">how an ordinary posit&lt;n=${fmt.n}, es=${fmt.es}&gt; would read this pattern</div>
    ${bitFieldsHtml(entryData.bits, u)}`;

  if (v.special) {
    html += `<div class="verdict">Exception encodings are the same either way.</div>`;
  } else if (u.decimal === v.decimal && u.totalExp === v.totalExp) {
    html += `<div class="verdict">Identical &mdash; the regime run is short enough that the cap
      never comes into play. Most everyday values sit here.</div>`;
  } else {
    html += `<div class="verdict differs">Uncapped, these bits would mean
      <b>${esc(u.decimal)}</b> instead of <b>${esc(v.decimal)}</b>.
      The run of ${u.fields.regime} identical bits would be read as regime k = ${u.k}
      (E = ${u.totalExp}) rather than being cut off at k_max = ${fmt.kmax} (E = ${v.totalExp}),
      leaving ${u.fields.frac} fraction bit${u.fields.frac === 1 ? '' : 's'} instead of
      ${v.fields.frac}.</div>`;
  }
  html += `</div>`;

  el.innerHTML = html;
}

function render() {
  const state = JSON.parse(calc.stateJson());
  renderFacts(state.format);
  renderStack(state.stack);
  renderAnatomy(state.stack[0], state.format, state.lastOp, state.neighbours);

  $('error').textContent = state.error || '';

  const active = calc.formatIndex();
  for (const b of $('formats').children) {
    b.setAttribute('aria-pressed', String(Number(b.dataset.fmt) === active));
  }
}

// ---------------------------------------------------------------------------- wiring

function buildFormatButtons() {
  $('formats').innerHTML = FORMAT_NAMES.map(
    (n, i) => `<button data-fmt="${i}" aria-pressed="false">${n}</button>`
  ).join('');
  $('formats').addEventListener('click', (ev) => {
    const b = ev.target.closest('button');
    if (!b) return;
    // Whatever was being typed is already in X, and gets converted with the rest.
    endEntry();
    calc.setFormat(Number(b.dataset.fmt));
    render();
  });
}

function bindButtons() {
  $('keys').addEventListener('click', (ev) => {
    const b = ev.target.closest('button');
    if (!b) return;
    if (b.dataset.k !== undefined) typeChar(b.dataset.k);
    else if (b.dataset.op) doBinary(b.dataset.op);
    else if (b.dataset.un) doUnary(b.dataset.un);
    else if (b.dataset.act) doAction(b.dataset.act);
  });
}

function bindKeyboard() {
  window.addEventListener('keydown', (ev) => {
    if (ev.ctrlKey || ev.metaKey || ev.altKey) return;
    const k = ev.key;

    // Anything the entry line will accept is typing, not a shortcut. That keeps hex digits
    // working inside `0x...` without stealing `d` or `e` the rest of the time.
    if (canType(k)) {
      ev.preventDefault();
      return typeChar(k);
    }

    const binary = { '+': 'add', '-': 'sub', '*': 'mul', '/': 'div' }[k];
    if (binary) {
      ev.preventDefault();
      return doBinary(binary);
    }
    const unary = { r: 'recip', d: 'double', h: 'half' }[k];
    if (unary) {
      ev.preventDefault();
      return doUnary(unary);
    }
    const action = {
      Enter: 'enter',
      Backspace: 'back',
      Escape: 'clear',
      n: 'chs',
      s: 'swap',
    }[k];
    if (action) {
      ev.preventDefault();
      return doAction(action);
    }
  });
}

/**
 * Keep the page behaving like an app rather than a document.
 *
 * `user-scalable=no` alone does not bind on iOS Safari — it deliberately ignores it so pages stay
 * zoomable. Refusing the gesture events is what actually holds the layout still, and
 * `touch-action: manipulation` on the buttons kills double-tap zoom.
 */
function lockViewport() {
  for (const ev of ['gesturestart', 'gesturechange', 'gestureend']) {
    document.addEventListener(ev, (e) => e.preventDefault(), { passive: false });
  }
  // Suppress rubber-band scrolling of the shell itself; the anatomy pane still scrolls normally.
  document.body.addEventListener(
    'touchmove',
    (e) => {
      const t = e.target;
      if (!(t instanceof Element) || !t.closest('.anatomy, .scroller, .facts')) e.preventDefault();
    },
    { passive: false }
  );
}

async function main() {
  await init();
  calc = new Calc();
  buildFormatButtons();
  bindButtons();
  bindKeyboard();
  lockViewport();
  render();
}

// A failed wasm fetch would otherwise leave a blank shell with the reason only in the console.
main().catch((err) => {
  console.error(err);
  $('anatomy').innerHTML = `<h2>Failed to start</h2>
    <p class="placeholder">The WebAssembly module did not load.<br>${esc(String(err))}</p>`;
});
