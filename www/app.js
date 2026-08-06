import init, { Calc } from './pkg/bposit_calc.js';

const FORMAT_NAMES = ['BPosit8', 'BPosit16', 'BPosit32', 'BPosit64'];

/** Minimum stack levels drawn, and the most we ever draw. */
const MIN_LEVELS = 4;
const MAX_LEVELS = 8;

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

function commitEntry() {
  if (entry === '') return;
  const s = entry;
  entry = '';
  const ok = isBitEntry(s) ? calc.pushBits(s) : calc.pushDecimal(s);
  // Keep an unparseable literal on the entry line so it can be corrected.
  if (!ok) entry = s;
}

function doBinary(op) {
  commitEntry();
  calc.binary(op);
  render();
}

function doUnary(op) {
  commitEntry();
  calc.unary(op);
  render();
}

function doAction(act) {
  switch (act) {
    case 'enter':
      if (entry !== '') commitEntry();
      else calc.dup();
      break;
    case 'chs':
      // While typing, +/- flips the sign of the literal; otherwise it negates level 1.
      if (entry !== '' && !isBitEntry()) {
        entry = entry.startsWith('-') ? entry.slice(1) : '-' + entry;
      } else {
        commitEntry();
        calc.unary('neg');
      }
      break;
    case 'back':
      if (entry !== '') entry = entry.slice(0, -1);
      else calc.dropTop();
      break;
    case 'swap':
      commitEntry();
      calc.swap();
      break;
    case 'dup':
      commitEntry();
      calc.dup();
      break;
    case 'clear':
      entry = '';
      calc.clear();
      break;
  }
  render();
}

function typeChar(c) {
  entry += c;
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
  const shown = Math.min(Math.max(MIN_LEVELS, stack.length), MAX_LEVELS);
  let html = '';
  // Level 1 is the top of the stack and is drawn last, nearest the entry line.
  for (let lvl = shown; lvl >= 1; lvl--) {
    const e = stack[lvl - 1];
    if (!e) {
      html += `<div class="level empty"><span class="tag">${lvl}:</span><span class="val">&mdash;</span></div>`;
      continue;
    }
    const v = e.bounded;
    html += `<div class="level">
      <span class="tag">${lvl}:</span>
      <span class="val${v.decimalExact ? '' : ' approx'}">${esc(v.decimal)}</span>
      <span class="rowbits mono">${bitRowHtml(e.bits, v)}</span>
    </div>`;
  }
  $('stack').innerHTML = html;
}

function renderAnatomy(entryData, fmt) {
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
  renderAnatomy(state.stack[0], state.format);

  $('entry').textContent = entry;
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
    // Commit anything half-typed so it is converted along with the rest of the stack.
    commitEntry();
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

    // Inside a raw bit pattern every hex letter is a digit, not a shortcut.
    if (isBitEntry() && /^[0-9a-fA-F]$/.test(k)) {
      ev.preventDefault();
      return typeChar(k);
    }
    if (/^[0-9]$/.test(k) || k === '.') {
      ev.preventDefault();
      return typeChar(k);
    }
    // 'e' starts an exponent; '0x'/'0b' start a bit pattern.
    if ((k === 'e' || k === 'E') && entry !== '' && !/[eE]/.test(entry)) {
      ev.preventDefault();
      return typeChar(k);
    }
    if ((k === 'x' || k === 'X' || k === 'b' || k === 'B') && entry === '0') {
      ev.preventDefault();
      return typeChar(k);
    }
    // A sign directly after the exponent marker belongs to the literal.
    if ((k === '-' || k === '+') && /[eE]$/.test(entry)) {
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
      ' ': 'dup',
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
