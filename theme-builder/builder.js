// RbxNative theme builder. Everything runs in the page: the theme is built
// as the editor reads it (THEMES.md on dev), layered over Default, and the
// .zip is made here with JSZip. The preview (#ed) is a copy of the editor
// whose colours and sizes are CSS variables, one per token.
import { BASES, HC_WIDGETS } from "./bases.js";

const DEFAULT = BASES.default;
const COLOR_KEYS = Object.keys(DEFAULT.colors);
const SIZE_KEYS = Object.keys(DEFAULT.sizes);
const STORE = "rbxnative-theme-builder-v1";
const $ = id => document.getElementById(id);

// ---------- State ----------

function fresh(base = "default") {
  return {
    base,
    manifest: { name: "My theme", author: "", description: "", version: "1.0.0" },
    id: "",
    colors: {},
    sizes: {},
    mode: null,
    window: "opaque",
    hover: { on: false, glow: "@check_on/0.6", radius: 10 },
    background: null, // { name, type, data (data URL), opacity, fit, layer, blur }
    fonts: { ui: [], mono: [] }, // [{ name, family, data (base64) }]
    view: "place",
  };
}

let state = load() || fresh();

function load() {
  try {
    const s = JSON.parse(localStorage.getItem(STORE) || "null");
    if (s && s.manifest && s.colors) return Object.assign(fresh(), s);
  } catch (e) {}
  return null;
}
let saveTimer = 0;
function save() {
  clearTimeout(saveTimer);
  saveTimer = setTimeout(() => {
    try { localStorage.setItem(STORE, JSON.stringify(state)); }
    catch (e) {
      // A big background or font may not fit; keep the rest.
      try { localStorage.setItem(STORE, JSON.stringify(Object.assign({}, state, { background: null, fonts: { ui: [], mono: [] } }))); } catch (e2) {}
    }
  }, 250);
}

const baseTheme = () => BASES[state.base] || DEFAULT;
const baseColor = k => (baseTheme().colors || {})[k] ?? DEFAULT.colors[k];
const baseSize = k => (baseTheme().sizes || {})[k] ?? DEFAULT.sizes[k];
const rawColor = k => state.colors[k] ?? baseColor(k);
const size = k => state.sizes[k] ?? baseSize(k);
const mode = () => state.mode || baseTheme().mode || "dark";

// ---------- Colours ----------

const HEX = /^#([0-9a-f]{6}|[0-9a-f]{8})$/i;
const REF = /^@([a-z0-9_]+)(?:\/(0|1|0?\.\d+|1\.0+))?$/i;

function validColor(v) {
  if (HEX.test(v)) return true;
  const m = REF.exec(v);
  return !!(m && COLOR_KEYS.includes(m[1]));
}

// Resolves a value to [r, g, b, a(0-1)], following @references like the editor.
function resolve(value, seen = new Set()) {
  if (typeof value !== "string") return [255, 0, 255, 1];
  if (HEX.test(value)) {
    const h = value.slice(1);
    const n = i => parseInt(h.slice(i, i + 2), 16);
    return [n(0), n(2), n(4), h.length === 8 ? n(6) / 255 : 1];
  }
  const m = REF.exec(value);
  if (!m || seen.has(m[1])) return [255, 0, 255, 1];
  seen.add(m[1]);
  const c = resolve(rawColor(m[1]), seen);
  return m[2] !== undefined ? [c[0], c[1], c[2], parseFloat(m[2])] : c;
}
const resolved = k => resolve(rawColor(k));
const css = c => `rgba(${c[0]},${c[1]},${c[2]},${+c[3].toFixed(3)})`;
const hex6 = c => "#" + c.slice(0, 3).map(x => x.toString(16).padStart(2, "0")).join("");
function toHex(c) {
  const a = Math.round(c[3] * 255);
  return (hex6(c) + (a === 255 ? "" : a.toString(16).padStart(2, "0"))).toUpperCase();
}

// ---------- Token rows ----------

const LABELS = {
  check_on: ["Accent", "Buttons, toggles, the active tab"],
  black: ["Background", "Behind everything, the top bar"],
  dock: ["Panels", "Explorer, Properties, Output"],
  field_select: ["Fields", "Text boxes and dropdowns"],
  text: ["Text", ""],
  text2: ["Secondary text", "Labels, inactive tabs, title and menu bar"],
  text3: ["Faint text", "Hints, placeholders"],
  border: ["Lines", "Between panels"],
  border2: ["Strong lines", "Around popovers and chips"],
  selection: ["Selection", "Selected Explorer rows, text"],
  hover: ["Hover", "Under the mouse"],
  warning: ["Warning", ""],
  text_error: ["Error", ""],
  diff_add: ["Success", ""],
  viewport_background: ["3D view sky", "Where nothing is drawn"],
};
const ESSENTIALS = Object.keys(LABELS);

const GROUPS = [
  ["Surfaces", k => /^(black|dock|chrome|field_select|tile|menu_bar|slide_backdrop|veil|shadow|avatar.*|ribbon_tab_active)$/.test(k)],
  ["Accent", k => /^(check_on|accent_.*|tab_active_bar|selection|knob|check_off_border|secondary_hover|danger_hover)$/.test(k)],
  ["Lines and hover", k => /^(hover.*|track|wash_faint|border2?)$/.test(k)],
  ["Text", k => /^text/.test(k) && k !== "text_error"],
  ["Status, diff and Output", k => /^(warning|text_error|error_soft|diff_.*|output_.*)$/.test(k)],
  ["Tools and axes", k => /^(tool_|axis_)/.test(k)],
  ["3D view overlays", k => /^viewport_/.test(k)],
];
function groupOf(k) {
  for (const [name, test] of GROUPS) if (test(k)) return name;
  return "Other";
}

const RESET_ICON = '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M3 8a5 5 0 1 0 1.5-3.5"/><path d="M3 2.5V5h2.5"/></svg>';

// One editable colour. `get`/`set` let the hover glow reuse it.
function colorRow(key, label, note, get = () => rawColor(key), set = v => setColor(key, v), base = () => baseColor(key)) {
  const row = document.createElement("div");
  row.className = "tok";
  row.dataset.key = key;
  row.innerHTML = `
    <div class="tok-name"><span></span><small></small></div>
    <label class="swatch" title="Pick a colour"><i></i><input type="color" aria-label="${label} colour"></label>
    <input class="hex" spellcheck="false" aria-label="${label} value" title="#RRGGBB, #RRGGBBAA, or @token / @token/0.5 to follow another colour">
    <input class="alpha" type="number" min="0" max="100" step="1" aria-label="${label} opacity percent" title="Opacity %">
    <button type="button" class="reset" aria-label="Reset ${label}" title="Back to the starting theme's value">${RESET_ICON}</button>`;
  row.querySelector(".tok-name span").textContent = label;
  const small = row.querySelector(".tok-name small");
  const sw = row.querySelector(".swatch i");
  const picker = row.querySelector("input[type=color]");
  const hex = row.querySelector(".hex");
  const alpha = row.querySelector(".alpha");
  row.refresh = () => {
    const v = get();
    const c = resolve(v);
    sw.style.background = css(c);
    if (document.activeElement !== picker) picker.value = hex6(c);
    if (document.activeElement !== hex) { hex.value = v; hex.setCustomValidity(""); }
    if (document.activeElement !== alpha) alpha.value = Math.round(c[3] * 100);
    const m = REF.exec(v);
    small.className = m ? "link" : "";
    small.textContent = m ? `follows ${prettyName(m[1])}${m[2] !== undefined ? " at " + Math.round(m[2] * 100) + "%" : ""}` : (note || key);
    row.classList.toggle("changed", v !== base());
  };
  picker.addEventListener("input", () => {
    const c = resolve(get());
    const a = Math.round(c[3] * 255);
    set((picker.value + (a === 255 ? "" : a.toString(16).padStart(2, "0"))).toUpperCase());
  });
  hex.addEventListener("input", () => {
    let v = hex.value.trim();
    if (/^[0-9a-f]{6}([0-9a-f]{2})?$/i.test(v)) v = "#" + v;
    if (validColor(v)) { hex.setCustomValidity(""); set(/^#/.test(v) ? v.toUpperCase() : v); }
    else hex.setCustomValidity("Use #RRGGBB, #RRGGBBAA or @token");
  });
  hex.addEventListener("blur", () => row.refresh());
  alpha.addEventListener("input", () => {
    const p = Math.max(0, Math.min(100, Number(alpha.value)));
    if (!Number.isFinite(p)) return;
    const v = get();
    const m = REF.exec(v);
    if (m) set(p === 100 && m[2] === undefined ? v : `@${m[1]}/${+(p / 100).toFixed(2)}`);
    else {
      const c = resolve(v);
      c[3] = p / 100;
      set(toHex(c));
    }
  });
  row.querySelector(".reset").addEventListener("click", () => set(null));
  return row;
}
const prettyName = k => (LABELS[k] ? LABELS[k][0] : k);

const rows = [];
function setColor(key, value) {
  if (value === null || value === baseColor(key)) delete state.colors[key];
  else state.colors[key] = value;
  changed();
}

function buildColours() {
  const ess = $("essentials");
  ess.innerHTML = "";
  for (const k of ESSENTIALS) {
    const r = colorRow(k, LABELS[k][0], LABELS[k][1]);
    rows.push(r); ess.append(r);
  }
  const box = $("colour-groups");
  box.innerHTML = "";
  const byGroup = new Map();
  for (const k of COLOR_KEYS) {
    const g = groupOf(k);
    if (!byGroup.has(g)) byGroup.set(g, []);
    byGroup.get(g).push(k);
  }
  for (const [g, keys] of byGroup) {
    const wrap = document.createElement("div");
    wrap.className = "group";
    const t = document.createElement("div");
    t.className = "group-title"; t.textContent = g;
    const list = document.createElement("div");
    list.className = "tokens";
    for (const k of keys) {
      const r = colorRow(k, k, "");
      rows.push(r); list.append(r);
    }
    wrap.append(t, list);
    box.append(wrap);
  }
  $("colour-count").textContent = COLOR_KEYS.length;
  $("colour-filter").addEventListener("input", e => {
    const q = e.target.value.trim().toLowerCase();
    for (const g of box.children) {
      let any = false;
      for (const r of g.querySelectorAll(".tok")) {
        const hit = !q || r.dataset.key.includes(q) || groupOf(r.dataset.key).toLowerCase().includes(q);
        r.hidden = !hit; any ||= hit;
      }
      g.hidden = !any;
    }
  });
  // Glow colour (stored in state.hover, not a token).
  const glow = colorRow("hover_glow", "Glow colour", "", () => state.hover.glow, v => { state.hover.glow = v ?? "@check_on/0.6"; changed(); }, () => "@check_on/0.6");
  rows.push(glow);
  $("glow-colour").append(glow);
}

// ---------- Sizes ----------

const RADII = SIZE_KEYS.filter(k => k.startsWith("radius"));
const TEXTS = SIZE_KEYS.filter(k => /^(text|line)_/.test(k));
const ROWS = ["row_height", "tree_row_height", "input_height", "strip_field_height"];

function setSize(key, value) {
  if (value === null || value === baseSize(key)) delete state.sizes[key];
  else state.sizes[key] = value;
}
const r2 = x => Math.round(x * 2) / 2;

function buildSizes() {
  const list = $("size-list");
  list.innerHTML = "";
  for (const k of SIZE_KEYS) {
    const l = document.createElement("label");
    l.className = "size"; l.dataset.key = k;
    l.innerHTML = `<span></span><input type="number" min="0" max="2000" step="0.5">`;
    l.querySelector("span").textContent = k;
    const inp = l.querySelector("input");
    inp.setAttribute("aria-label", k);
    inp.addEventListener("input", () => {
      const v = Number(inp.value);
      if (inp.value !== "" && Number.isFinite(v) && v >= 0 && v <= 2000) { setSize(k, v); changed(); }
    });
    list.append(l);
  }
  $("size-count").textContent = SIZE_KEYS.length;
  $("corners").addEventListener("input", e => {
    const f = Number(e.target.value) / DEFAULT.sizes.radius;
    for (const k of RADII) setSize(k, k === "radius" ? Number(e.target.value) : r2(DEFAULT.sizes[k] * f));
    changed();
  });
  $("textsize").addEventListener("input", e => {
    const f = Number(e.target.value) / DEFAULT.sizes.text_md;
    for (const k of TEXTS) setSize(k, k === "text_md" ? Number(e.target.value) : r2(DEFAULT.sizes[k] * f));
    changed();
  });
  $("density").addEventListener("input", e => {
    const f = Number(e.target.value) / DEFAULT.sizes.row_height;
    for (const k of ROWS) setSize(k, k === "row_height" ? Number(e.target.value) : Math.round(DEFAULT.sizes[k] * f));
    changed();
  });
}

// ---------- Segmented controls ----------

function seg(id, get, set) {
  const el = $(id);
  el.addEventListener("click", e => {
    const b = e.target.closest("button");
    if (b) { set(b.dataset.v ?? b.dataset.base); changed(); }
  });
  return () => el.querySelectorAll("button").forEach(b => b.setAttribute("aria-pressed", String((b.dataset.v ?? b.dataset.base) === get())));
}
const segs = [];

// ---------- Fonts ----------

// The family name inside a TTF/OTF/TTC (name table, typographic family first).
function fontFamily(buf) {
  const v = new DataView(buf);
  let at = 0;
  if (v.getUint32(0) === 0x74746366) at = v.getUint32(12); // 'ttcf': first font
  const tables = v.getUint16(at + 4);
  let name = -1;
  for (let i = 0; i < tables; i++) {
    const r = at + 12 + i * 16;
    if (v.getUint32(r) === 0x6e616d65) { name = v.getUint32(r + 8); break; }
  }
  if (name < 0) return null;
  const count = v.getUint16(name + 2), strings = name + v.getUint16(name + 4);
  const found = {};
  for (let i = 0; i < count; i++) {
    const r = name + 6 + i * 12;
    const platform = v.getUint16(r), id = v.getUint16(r + 6), len = v.getUint16(r + 8), off = v.getUint16(r + 10);
    if (id !== 1 && id !== 16) continue;
    let s = "";
    if (platform === 3 || platform === 0) for (let j = 0; j < len; j += 2) s += String.fromCharCode(v.getUint16(strings + off + j));
    else if (platform === 1) for (let j = 0; j < len; j++) s += String.fromCharCode(v.getUint8(strings + off + j));
    if (s && !found[id]) found[id] = s;
  }
  return found[16] || found[1] || null;
}
const b64 = buf => { let s = ""; const a = new Uint8Array(buf); for (let i = 0; i < a.length; i += 0x8000) s += String.fromCharCode(...a.subarray(i, i + 0x8000)); return btoa(s); };
const unb64 = s => Uint8Array.from(atob(s), c => c.charCodeAt(0));

const loadedFaces = new Set();
async function registerFonts() {
  for (const slot of ["ui", "mono"]) for (const f of state.fonts[slot]) {
    const key = f.family + "|" + f.name;
    if (loadedFaces.has(key)) continue;
    loadedFaces.add(key);
    try {
      const face = new FontFace(f.family, unb64(f.data).buffer);
      await face.load();
      document.fonts.add(face);
    } catch (e) { msg(`${f.name} couldn't be loaded as a font.`, true); }
  }
}

async function addFonts(slot, files) {
  const total = state.fonts.ui.length + state.fonts.mono.length;
  for (const file of files) {
    if (total + files.length > 16) { msg("A theme can carry at most 16 font files.", true); return; }
    if (file.size > 8 * 1024 * 1024) { msg(`${file.name} is over 8 MiB.`, true); continue; }
    const buf = await file.arrayBuffer();
    let family = null;
    try { family = fontFamily(buf); } catch (e) {}
    if (!family) { msg(`${file.name} doesn't look like a TTF or OTF font.`, true); continue; }
    const name = file.name.replace(/[^A-Za-z0-9._-]/g, "_");
    state.fonts[slot] = state.fonts[slot].filter(f => f.name !== name);
    state.fonts[slot].push({ name, family, data: b64(buf) });
  }
  await registerFonts();
  changed();
}

function renderFontList(slot) {
  const ul = $(`font-${slot}-list`);
  ul.innerHTML = "";
  for (const f of state.fonts[slot]) {
    const li = document.createElement("li");
    li.innerHTML = `<span></span><button type="button">Remove</button>`;
    li.querySelector("span").textContent = f.name;
    const fam = document.createElement("small"); fam.textContent = f.family;
    li.querySelector("span").append(fam);
    li.querySelector("button").setAttribute("aria-label", "Remove " + f.name);
    li.querySelector("button").addEventListener("click", () => {
      state.fonts[slot] = state.fonts[slot].filter(x => x !== f);
      changed();
    });
    ul.append(li);
  }
}
const family = slot => (state.fonts[slot][0] || {}).family || null;

// ---------- Preview ----------
// A copy of the real editor window at its own 1600×900, laid out from
// screenshots of the Linux build: titlebar, menu bar, document tabs, ribbon,
// Properties | 3D view | Explorer, then Output and the Command Bar.

const W = 1600, H = 900;

const ICON = {
  copy: '<rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2"/>',
  paste: '<rect x="8" y="2" width="8" height="4" rx="1"/><path d="M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2"/>',
  cut: '<circle cx="6" cy="6" r="3"/><circle cx="6" cy="18" r="3"/><path d="M20 4 8.1 15.9M14.5 14.5 20 20M8.1 8.1 12 12"/>',
  dup: '<rect x="8" y="8" width="13" height="13" rx="2"/><path d="M4 16V5a1 1 0 0 1 1-1h11M14.5 11.5v6M11.5 14.5h6"/>',
  select: '<path d="M4.04 4.69a.5.5 0 0 1 .65-.65l16 6.5a.5.5 0 0 1-.06.95l-6.13 1.58a2 2 0 0 0-1.43 1.43l-1.58 6.13a.5.5 0 0 1-.95.06z"/>',
  move: '<path d="M5 3v15h15M5 18l7-7M17 15l3 3-3 3M2 6l3-3 3 3"/>',
  scale: '<path d="M5 3v15h15"/><circle cx="12" cy="11" r="2"/><circle cx="20" cy="18" r="2"/>',
  rotate: '<ellipse cx="12" cy="12" rx="9" ry="4"/><path d="M12 3a4 9 0 0 1 0 18M9 21l3 0-1-3"/>',
  transform: '<rect x="4" y="4" width="16" height="16" rx="2" stroke-dasharray="3 3"/>',
  pivot: '<path d="M5 3v15h15M9 14l4-4M15 14h0"/>',
  align: '<path d="M4 4v16M8 6h10a1 1 0 0 1 1 1v2a1 1 0 0 1-1 1H8zM8 14h6a1 1 0 0 1 1 1v2a1 1 0 0 1-1 1H8z"/>',
  stud: '<path d="m4 14 8-8 8 8-8 4z"/><path d="M12 6v12"/>',
  angle: '<path d="M20 12a8 8 0 1 1-2.3-5.7L20 9"/><path d="M20 4v5h-5"/>',
  part: '<path d="M21 8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16Z"/><path d="M3.3 7 12 12l8.7-5M12 22V12"/>',
  script: '<path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><path d="M14 2v6h6M10 12l-2 2 2 2M14 16l2-2-2-2"/>',
  ui: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M3 9h18M7 6.5h0"/>',
  box: '<path d="M21 8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16Z"/><path d="m3.3 7 8.7 5 8.7-5M12 22V12M7.5 4.3l9 5.1"/>',
  terrain: '<path d="M3 20 10 4l4 8 2-3 5 11z"/><path d="m8 9 2 2 2-2"/>',
  gear: '<circle cx="12" cy="12" r="3"/><path d="M12 2v3M12 19v3M4.9 4.9l2.1 2.1M17 17l2.1 2.1M2 12h3M19 12h3M4.9 19.1 7 17M17 7l2.1-2.1"/>',
  db: '<ellipse cx="12" cy="5" rx="8" ry="3"/><path d="M4 5v14c0 1.7 3.6 3 8 3s8-1.3 8-3V5M4 12c0 1.7 3.6 3 8 3s8-1.3 8-3"/>',
  explorer: '<path d="M4 4h16M4 4v14h4M4 11h4M11 11h9M11 18h9M11 4"/>',
  props: '<path d="M4 7h16M4 17h16"/><circle cx="9" cy="7" r="2"/><circle cx="15" cy="17" r="2"/><path d="M4 12h16"/><circle cx="12" cy="12" r="2"/>',
  output: '<path d="m4 7 5 5-5 5M12 18h8"/>',
  viewport: '<rect x="3" y="5" width="18" height="12" rx="2"/><path d="M8 21h8M12 17v4"/><circle cx="17" cy="9" r="2"/>',
  pad: '<rect x="2" y="7" width="20" height="11" rx="5"/><path d="M7 11v3M5.5 12.5h3"/><circle cx="16" cy="11.5" r=".6"/><circle cx="18" cy="13.5" r=".6"/>',
  shield: '<path d="M12 2 4 5v6c0 5 3.4 9.4 8 11 4.6-1.6 8-6 8-11V5z"/><path d="M12 8v4M12 16h0"/>',
  film: '<rect x="3" y="3" width="18" height="18" rx="2"/><path d="M7 3v18M17 3v18M3 8h4M3 16h4M17 8h4M17 16h4M3 12h18"/>',
  layers: '<path d="m12 2 10 5-10 5L2 7z"/><path d="m2 12 10 5 10-5M2 17l10 5 10-5"/>',
  globe: '<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3a14 14 0 0 1 0 18 14 14 0 0 1 0-18"/>',
  code: '<path d="m8 7-5 5 5 5M16 7l5 5-5 5"/>',
  grid4: '<rect x="4" y="4" width="6" height="6" rx="1"/><rect x="14" y="4" width="6" height="6" rx="1"/><rect x="4" y="14" width="6" height="6" rx="1"/><rect x="14" y="14" width="6" height="6" rx="1"/>',
  anim: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M7 4v16M3 8h4M3 12h4M3 16h4"/>',
  min: '<path d="M6 12h12"/>',
  max: '<rect x="6" y="6" width="12" height="12" rx="1"/>',
  close: '<path d="M6 6l12 12M18 6 6 18"/>',
  x: '<path d="M8 8l8 8M16 8l-8 8"/>',
  plus: '<path d="M12 6v12M6 12h12"/>',
  more: '<circle cx="6" cy="12" r=".8"/><circle cx="12" cy="12" r=".8"/><circle cx="18" cy="12" r=".8"/>',
  chev: '<path d="m7 10 5 5 5-5"/>',
  right: '<path d="m10 7 5 5-5 5"/>',
  warn: '<circle cx="12" cy="12" r="9"/><path d="M12 7.5v5.5M12 16.5h0"/>',
  undo: '<path d="M9 14 4 9l5-5"/><path d="M4 9h10a6 6 0 0 1 0 12h-3"/>',
  redo: '<path d="m15 14 5-5-5-5"/><path d="M20 9H10a6 6 0 0 0 0 12h3"/>',
  wand: '<path d="M15 4V2M15 10V8M11 6h2M17 6h2M18 3l-1 1M3 21l11-11"/>',
  bug: '<rect x="8" y="6" width="8" height="14" rx="4"/><path d="M12 10v10M4 13h4M16 13h4M5 7l3 2M19 7l-3 2M5 19l3-2M19 19l-3-2"/>',
  pencil: '<path d="M4 20h4L19 9l-4-4L4 16z"/>',
  file: '<path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><path d="M14 2v6h6"/>',
  cube: '<path d="M21 8 12 3 3 8v8l9 5 9-5z"/><path d="M3 8l9 5 9-5M12 13v8"/>',
};
const icon = (n, cls = "i") => `<svg class="${cls}" viewBox="0 0 24 24" aria-hidden="true">${ICON[n]}</svg>`;
const LOGO = '<svg class="e-logo" viewBox="0 0 23.8 17" fill="currentColor" aria-hidden="true"><path d="M15.3 5.196L9.623 1.508L5.389 1.443Q4.405 1.428 4.138 2.374L0 17L5.453 17Q6.059 17 6.223 16.551L7.249 13.728L5.998 13.728Q5.91 13.728 5.966 13.536L8.34 5.357L14.177 9.174L15.3 5.196Z"/><path d="M23.8 0L18.989 0Q18.203 0 17.962 0.674L16.84 3.817L18.026 3.817Q18.147 3.817 18.091 4.009L15.589 12.606L9.526 8.596L8.404 12.574L13.504 16.006Q13.837 16.23 14.017 16.23L18.54 16.23Q19.25 16.23 19.502 15.332L23.8 0Z"/></svg>';

function cls(name) { return `icons/${mode() === "light" ? "light" : "dark"}/${name}.svg`; }

function buildPreview() {
  const tile = (i, label, extra = "") => `<div class="e-tile hov${extra}">${icon(i)}<span>${label}</span></div>`;
  const node = (depth, icn, name, tw, extra = "") =>
    `<div class="e-node hov${extra}" style="padding-left:${6 + depth * 20}px">${depth ? `<i class="guide" style="left:${depth * 20 - 3}px"></i>` : ""}<span class="tw ${tw}">${tw && tw !== "none" ? icon(tw === "open" ? "chev" : "right") : ""}</span><img data-icon="${icn}" alt=""><span>${name}</span></div>`;
  const prop = (label, val, ro = false) => `<div class="e-prop hov${ro ? " ro" : ""}"><span>${label}</span><div>${val}</div></div>`;
  const sec = name => `<div class="e-sec">${icon("chev")}${name}</div>`;
  const dockHead = title => `<div class="e-dockhead"><span class="e-dtab"><b>${title}</b>${icon("x", "i xs")}</span><span class="e-vsep"></span><span class="more">${icon("more")}</span></div>`;
  const docTab = (i, label, on, closable) => `<span class="e-doc hov${on ? " on" : ""}" data-doc="${on}">${icon(i)}${label}${closable ? icon("x", "i xs") : ""}</span>`;
  $("ed").innerHTML = `
    <div class="wallblur"></div><div class="ground"></div>
    <div class="bg" id="bg"></div>
    <div class="e-title">${LOGO}<span class="e-name"><b>RbxNative</b><i></i><span>Village.rbxl</span></span><span class="e-wc">${icon("min")}${icon("max")}${icon("close")}</span></div>
    <div class="e-menu"><span>File</span><span>Edit</span><span>Model</span><span>View</span></div>
    <div class="e-docs" id="docs">
      ${docTab("globe", "Village.rbxl", true, false)}${docTab("code", "Script Editor", false, true)}${docTab("grid4", "UI Editor", false, true)}${docTab("anim", "Animation Editor", false, true)}<span class="e-docplus">${icon("plus")}</span>
    </div>
    <div class="e-rtabs">${["Home", "Avatar", "UI", "Script", "Model", "Test", "Plugins"].map((t, i) => `<span class="${i === 0 ? "on" : ""}">${t}</span>`).join("")}</div>
    <div class="e-ribbon">
      ${tile("copy", "Copy")}
      <div class="e-stack"><div class="hov off">${icon("paste")}Paste</div><div class="hov">${icon("cut")}Cut</div><div class="hov">${icon("dup")}Duplicate</div></div>
      <span class="e-sep"></span>
      <div class="e-tools">${["select", "move", "scale", "rotate", "transform", "pivot", "align"].map((t, i) => `<span class="e-tool hov${i === 0 ? " on" : ""}">${icon(t)}</span>`).join("")}</div>
      <div class="e-fields"><span class="e-field">${icon("stud")}1 stud</span><span class="e-field">${icon("angle")}45°</span></div>
      <span class="e-sep"></span>
      ${tile("part", "Part")}<div class="e-popwrap">${tile("script", "Script", " open")}
        <div class="e-pop" id="pop"><span class="hov">${icon("file")}Script</span><span class="hov">${icon("file")}Local Script</span><span class="hov">${icon("cube")}Module Script</span><i></i><span class="hov">${icon("pencil")}Manage templates…</span></div></div>
      ${tile("ui", "UI")}${tile("box", "Toolbox")}
      <span class="e-sep"></span>
      ${tile("terrain", "Terrain E…")}
      <span class="e-sep"></span>
      ${tile("gear", "Game Set…")}${tile("db", "Data Stor…")}
      <span class="e-sep"></span>
      ${tile("explorer", "Explorer", " on")}${tile("props", "Properties", " on")}${tile("output", "Output", " on")}${tile("viewport", "Viewport")}${tile("pad", "Input Acti…")}${tile("shield", "Security…")}${tile("film", "Playtests")}${tile("layers", "Lay")}
    </div>
    <div class="e-work">
    <div class="e-body">
      <section class="e-dock">
        ${dockHead('SpawnLocation "SpawnLocation"')}
        <div class="e-search">Filter Properties (Ctrl+Shift+P)</div>
        <div class="e-props">
          <div class="e-sel"><span class="e-selicon"><img data-icon="spawn-location" alt=""></span><b>SpawnLocation</b><small>— Workspace</small></div>
          ${sec("APPEARANCE")}
          ${prop("BrickColor", '<span class="e-val"><i class="sw" style="background:#A3A2A5"></i>Medium stone…</span>')}
          ${prop("CastShadow", '<span class="e-tog on"></span>')}
          ${prop("Color", '<i class="sw big" style="background:#A3A2A5"></i>')}
          ${prop("Material", '<span class="e-val sel">Plastic' + icon("chev", "i xs") + '</span>')}
          ${prop("MaterialVariant", '<span class="e-val"></span>')}
          ${prop("Reflectance", '<span class="e-slider"><i></i><span class="e-val num">0</span></span>')}
          ${prop("Transparency", '<span class="e-slider"><i></i><span class="e-val num">0</span></span>')}
          ${sec("DATA")}
          ${prop("ClassName", '<span class="e-ro">"SpawnLocation"</span>', true)}
          ${prop("Name", '<span class="e-val">SpawnLocation</span>')}
        </div>
      </section>
      <section class="e-center">
        <div class="e-view"></div>
        <div class="e-script">
          <div class="e-stabs"><span class="e-stab on">${icon("file")}Lights${icon("x", "i xs")}</span><span class="sp"></span>${icon("bug")}</div>
          <div class="e-stool"><span class="e-seg"><span class="on">Code</span><span>Graph</span></span><span class="e-vsep"></span><span class="e-crumb">${icon("cube")}Script <code>ServerScriptService.Lights</code></span><span class="sp"></span>${icon("undo")}${icon("redo")}${icon("wand")}<span class="e-vsep"></span><span class="px">13 px</span></div>
          <div class="e-code">
            <div class="e-gutter">${Array.from({ length: 12 }, (_, i) => `<div${i === 5 ? ' class="cur"' : ""}>${i + 1}</div>`).join("")}</div>
            <div class="e-lines">
              <div><span class="c">-- Turns the street lamps on at dusk</span></div>
              <div><span class="k">local</span> Lighting <span class="o">=</span> <span class="v">game</span>:<span class="f">GetService</span>(<span class="s">"Lighting"</span>)</div>
              <div><span class="k">local</span> lamps <span class="o">=</span> <span class="v">workspace</span>.Village:<span class="f">GetDescendants</span>()</div>
              <div></div>
              <div><span class="k">local function</span> <span class="f">update</span>()</div>
              <div class="cur">    <span class="k">local</span> night <span class="o">=</span> Lighting.ClockTime <span class="o">&gt;</span> <span class="n">18</span> <span class="k">or</span> Lighting.ClockTime <span class="o">&lt;</span> <span class="n">6</span></div>
              <div>    <span class="k">for</span> _, lamp <span class="k">in</span> <span class="hl">lamps</span> <span class="k">do</span></div>
              <div>        <span class="k">if</span> lamp:<span class="f">IsA</span>(<span class="s">"PointLight"</span>) <span class="k">then</span> lamp.Enabled <span class="o">=</span> night <span class="k">end</span></div>
              <div>    <span class="k">end</span></div>
              <div><span class="k">end</span></div>
              <div></div>
              <div>Lighting:<span class="f">GetPropertyChangedSignal</span>(<span class="s">"ClockTime"</span>):<span class="f">Connect</span>(<span class="sq">updat</span>)</div>
            </div>
          </div>
          <div class="e-status"><span>Ln 6, Col 18 · Luau · <b class="bad">1 error</b></span><span>Luau</span></div>
        </div>
      </section>
      <section class="e-dock right">
        ${dockHead("Explorer")}
        <div class="e-searchrow"><div class="e-search">Search</div>${icon("code")}</div>
        <div class="e-tree">
          ${node(0, "workspace", "Workspace", "open")}
          ${node(1, "camera", "Camera", "none")}
          ${node(1, "terrain", "Terrain", "none")}
          ${node(1, "spawn-location", "SpawnLocation", "closed", " sel")}
          ${node(1, "part", "Baseplate", "closed")}
          ${node(0, "players", "Players", "none")}
          ${node(0, "light", "Lighting", "closed")}
          ${node(0, "material-service", "MaterialService", "none")}
          ${node(0, "replicated-storage", "ReplicatedFirst", "none")}
          ${node(0, "replicated-storage", "ReplicatedStorage", "none")}
          ${node(0, "server-script-service", "ServerScriptService", "closed")}
          ${node(0, "server-storage", "ServerStorage", "none")}
          ${node(0, "screen-gui", "StarterGui", "none")}
          ${node(0, "backpack", "StarterPack", "none")}
          ${node(0, "starter-player", "StarterPlayer", "closed")}
        </div>
      </section>
    </div>
    <section class="e-out">
      <div class="e-outhead">
        <span class="e-otab on"><b>Output</b>${icon("x", "i xs")}</span><span class="e-otab">Viewport</span><span class="e-otab sc">Argon</span><span class="e-otab sc">Wally</span><span class="e-otab sc">Script Analysis</span><span class="e-otab sc">Watch</span><span class="e-otab sc">Call Stack</span>
        <span class="e-vsep"></span><span class="runs">1 runs</span><span class="e-search grow">Search</span>
        <span class="e-chips"><span class="on">All</span><span>Output</span><span>Info</span><span>Warnings</span><span>Errors</span></span>
        <span class="e-ctx">All Contexts${icon("chev", "i xs")}</span><span class="clear">Clear</span>${icon("more")}${icon("chev")}
      </div>
      <div class="e-log">
        <div class="row"><span class="kind">${icon("warn")}warning</span><span class="warn">Infinite yield possible on 'Workspace.Village:WaitForChild("Lamp")'</span></div>
        <div class="row"><span class="kind srv">Server</span><span class="server">Village loaded: 42 houses</span></div>
        <div class="row"><span class="kind cli">Client</span><span class="client">Hello from Cheeteau</span></div>
        <div class="row err"><span class="kind">error</span><span>Workspace.Village.Lights:12: attempt to index nil with 'Position'</span></div>
      </div>
    </section>
    </div>
    <div class="e-cmd"><span class="e-cmdbox">${icon("right", "i xs")}<span>Execute a command with Ctrl+Enter or use Ctrl+Up/Down for history</span><b>Luau</b></span></div>
`;
}

// Code colours: GPUI Kit's own palette for the mode (High contrast ships its
// own in widgets.json), which is what the editor paints with.
const CODE = {
  dark: { fg: "@text", kw: "#87B1F6", str: "#A3E09F", num: "#CC9E00", fn: "#B3C5F3", com: "#9D9D9D", var: "#87B1F6", line: "#FFFFFF08", warn: "#FACC15" },
  light: { fg: "@text", kw: "#0433FF", str: "#036A07", num: "#0433FF", fn: "#0000A2", com: "#007FFF", var: "#0433FF", line: "#00000008", warn: "#B88400" },
  hc: { fg: "#FFFFFF", kw: "#B8C6FF", str: "#A8F0B0", num: "#FFD58F", fn: "#A6E2FF", com: "#D6CBA4", var: "#FFBCD8", line: "#FFFFFF1A", warn: "#FFD58F" },
};

const imageSizes = new Map();
function imageScale(b) {
  const size = imageSizes.get(b.data);
  if (!size) {
    if (!imageSizes.has(b.data)) {
      imageSizes.set(b.data, null);
      const im = new Image();
      im.onload = () => { imageSizes.set(b.data, [im.naturalWidth, im.naturalHeight]); applyPreview(); };
      im.src = b.data;
    }
    return W / 1600;
  }
  const [w, h] = size, sx = W / w, sy = H / h;
  return b.fit === "contain" ? Math.min(sx, sy) : b.fit === "fill" ? Math.sqrt(sx * sy) : Math.max(sx, sy);
}

function applyPreview() {
  const ed = $("ed");
  const s = ed.style;
  for (const k of COLOR_KEYS) s.setProperty("--c-" + k, css(resolved(k)));
  // The editor paints its ground (`black`) once, under everything; a strip
  // whose colour is that same translucent ground skips its own fill, so the
  // coats don't stack.
  const ground = resolved("black");
  const same = (a, b) => a.every((v, i) => Math.abs(v - b[i]) < 1e-6);
  if (ground[3] < 1) if (same(resolved("menu_bar"), ground)) s.setProperty("--c-menu_bar", "transparent");
  for (const k of SIZE_KEYS) s.setProperty("--s-" + k, size(k));
  const code = state.base === "high-contrast" && mode() === "dark" ? CODE.hc : CODE[mode()];
  for (const [k, v] of Object.entries(code)) s.setProperty("--code-" + k, css(resolve(v)));
  s.setProperty("--glow", state.hover.on ? `0 0 ${state.hover.radius}px ${css(resolve(state.hover.glow))}` : "none");
  s.setProperty("--ui", family("ui") ? `"${family("ui")}"` : "Manrope");
  s.setProperty("--mono-font", family("mono") ? `"${family("mono")}"` : '"JetBrains Mono"');
  ed.classList.toggle("blurred", state.window === "blurred");
  ed.classList.toggle("script", state.view === "script");
  $("desk").classList.toggle("solid", state.window === "opaque");
  const docs = $("docs").children;
  docs[0].classList.toggle("on", state.view !== "script");
  docs[1].classList.toggle("on", state.view === "script");
  for (const img of ed.querySelectorAll("img[data-icon]")) {
    const src = cls(img.dataset.icon);
    if (img.getAttribute("src") !== src) img.src = src;
  }
  const bg = $("bg");
  const b = state.background;
  bg.className = "bg" + (b && b.layer === "over" ? " over" : "");
  bg.style.backgroundImage = b ? `url("${b.data}")` : "none";
  bg.style.opacity = b ? b.opacity : 0;
  bg.style.backgroundSize = b ? { cover: "cover", contain: "contain", fill: "100% 100%" }[b.fit] : "";
  bg.style.backgroundPosition = "center";
  // The editor blurs in the image's own pixels (sigma, like CSS), so on
  // screen the blur grows or shrinks with how far the image is scaled.
  const px = b && b.blur ? b.blur * imageScale(b) : 0;
  bg.style.filter = px ? `blur(${px.toFixed(1)}px)` : "";
  bg.style.inset = px ? `-${Math.ceil(px * 2)}px` : "";
  $("pop").hidden = state.view !== "place";
}

function fit() {
  const stage = $("stage"), scale = $("scale");
  const w = stage.clientWidth;
  const h = window.innerHeight - 190;
  const k = Math.min(w / W, Math.max(h, 320) / H, 1);
  scale.style.transform = `scale(${k})`;
  stage.style.height = H * k + "px";
}

// ---------- See-through panels ----------

// Panel opacity is how much of what's behind the window a panel hides. A panel
// (`dock`) paints over the ground (`black`), so their alphas stack; both get
// 1 - sqrt(1 - a), which stacks back to a. The chrome, tiles and menu bar
// point at those two, so they follow.
const PANEL_KEYS = ["black", "dock"];
const panelAlpha = () => { const g = resolved("black")[3], d = resolved("dock")[3]; return +(1 - (1 - g) * (1 - d)).toFixed(2); };
function setPanelAlpha(a) {
  const each = a >= 1 ? 1 : 1 - Math.sqrt(1 - a);
  for (const k of PANEL_KEYS) {
    const c = resolve(rawColor(k));
    const hex = hex6(c).toUpperCase() + (each >= 1 ? "" : Math.round(each * 255).toString(16).padStart(2, "0").toUpperCase());
    setColor(k, hex);
  }
}
// Something should show through but the panels are solid: make them see-through
// once, and say so, rather than leave a preview that looks unchanged.
function seeThrough(what) {
  if (panelAlpha() < 1) return;
  setPanelAlpha(0.72);
  msg(`Panels set to 72% opacity so ${what} shows through. Change it with Panel opacity under Look.`);
}

// A made-up desktop behind see-through windows, preview only.
function newWallpaper() {
  const r = (a, b) => a + Math.random() * (b - a);
  const h = r(0, 360);
  const spot = (dh, x, y, l) => `radial-gradient(${r(40, 70)}% ${r(45, 80)}% at ${x}% ${y}%, hsl(${(h + dh) % 360} ${r(55, 85)}% ${l}%) 0%, transparent 70%)`;
  // Hard-edged discs and stripes, so Blurred visibly differs from See-through.
  const disc = (dh, l) => { const x = r(5, 95), y = r(5, 95), d = r(6, 16); return `radial-gradient(circle at ${x}% ${y}%, hsl(${(h + dh) % 360} 80% ${l}%) ${d}%, transparent ${d + 0.3}%)`; };
  $("desk").style.setProperty("--wall", [
    disc(r(0, 60), r(60, 75)), disc(r(120, 200), r(55, 70)), disc(r(220, 300), r(50, 65)),
    `repeating-linear-gradient(${r(20, 70)}deg, hsl(${h} 70% 70% / .14) 0 14px, transparent 14px 46px)`,
    spot(0, r(5, 35), r(5, 35), r(55, 68)),
    spot(r(40, 90), r(60, 95), r(10, 45), r(45, 60)),
    spot(r(150, 220), r(20, 70), r(60, 100), r(35, 55)),
    `linear-gradient(${r(0, 360)}deg, hsl(${(h + 200) % 360} 45% 18%), hsl(${(h + 260) % 360} 50% 10%))`,
  ].join(", "));
}

// ---------- Sync the form with the state ----------

function slug(s) {
  return s.toLowerCase().normalize("NFKD").replace(/[̀-ͯ]/g, "").replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 48) || "my-theme";
}
const themeId = () => (state.id && /^[a-z0-9][a-z0-9_-]*$/.test(state.id) ? state.id : slug(state.manifest.name));
const RESERVED = ["default", "light", "high-contrast"];

function refreshForm() {
  for (const k of ["name", "author", "description", "version"]) {
    const el = $("m-" + k);
    if (document.activeElement !== el) el.value = state.manifest[k];
  }
  const idEl = $("m-id");
  if (document.activeElement !== idEl) idEl.value = state.id;
  idEl.placeholder = slug(state.manifest.name);
  document.querySelectorAll(".theme-id").forEach(e => (e.textContent = themeId()));
  for (const r of rows) r.refresh();
  for (const l of document.querySelectorAll(".size")) {
    const k = l.dataset.key, inp = l.querySelector("input");
    if (document.activeElement !== inp) inp.value = size(k);
    l.classList.toggle("changed", size(k) !== DEFAULT.sizes[k]);
  }
  const setRange = (id, v, fmt) => { const el = $(id); if (document.activeElement !== el) el.value = v; $(id + "-out").textContent = fmt(v); };
  setRange("corners", size("radius"), v => v + "px");
  setRange("textsize", size("text_md"), v => v + "px");
  setRange("density", size("row_height"), v => v + "px");
  setRange("glow-radius", state.hover.radius, v => v + "px");
  $("glow-on").checked = state.hover.on;
  $("glow-box").hidden = !state.hover.on;
  const b = state.background;
  $("bg-box").hidden = !b;
  $("bg-clear").hidden = !b;
  if (b) {
    $("bg-name").textContent = b.name;
    setRange("bg-opacity", b.opacity, v => Math.round(v * 100) + "%");
    setRange("bg-blur", b.blur || 0, v => v + "px");
  }
  setRange("panel-alpha", panelAlpha(), v => Math.round(v * 100) + "%");
  $("wallpaper").hidden = state.window === "opaque";
  $("window-hint").hidden = state.window === "opaque";
  segs.forEach(f => f());
  renderFontList("ui");
  renderFontList("mono");
}

function changed() {
  refreshForm();
  applyPreview();
  save();
}

let msgTimer = 0;
function msg(text, bad = false, where = "msg") {
  const el = $(where);
  el.textContent = text;
  el.classList.toggle("bad", bad);
  clearTimeout(msgTimer);
  if (text) msgTimer = setTimeout(() => (el.textContent = ""), 7000);
}

// ---------- Export ----------

function themeJson() {
  const out = {};
  const colors = {};
  for (const k of COLOR_KEYS) if (rawColor(k) !== DEFAULT.colors[k]) colors[k] = rawColor(k);
  const sizes = {};
  for (const k of SIZE_KEYS) if (size(k) !== DEFAULT.sizes[k]) sizes[k] = size(k);
  if (Object.keys(colors).length) out.colors = colors;
  if (Object.keys(sizes).length) out.sizes = sizes;
  if (mode() !== "dark") out.mode = mode();
  if (state.window !== "opaque") out.window = state.window;
  const b = state.background;
  if (b) {
    out.background = { image: bgFile(b), opacity: b.opacity, fit: b.fit, layer: b.layer };
    if (b.blur) out.background.blur = b.blur;
  }
  if (state.hover.on) out.hover = { glow: state.hover.glow, glow_radius: state.hover.radius };
  const files = [...state.fonts.ui, ...state.fonts.mono].map(f => "fonts/" + f.name);
  if (files.length) {
    out.fonts = {};
    if (family("ui")) out.fonts.ui = family("ui");
    if (family("mono")) out.fonts.mono = family("mono");
    out.fonts.files = [...new Set(files)];
  }
  return out;
}
const bgFile = b => "background." + ({ "image/png": "png", "image/jpeg": "jpg", "image/webp": "webp", "image/gif": "gif" }[b.type] || "png");

function problems() {
  const m = state.manifest, p = [];
  for (const k of ["name", "author", "description", "version"]) if (!m[k].trim()) p.push(`Fill in ${k === "name" ? "the name" : k === "author" ? "the author" : k === "description" ? "a description" : "a version"}.`);
  if (RESERVED.includes(themeId())) p.push(`"${themeId()}" is a built-in theme's folder name, pick another.`);
  if (state.id && !/^[a-z0-9][a-z0-9_-]*$/.test(state.id)) p.push("The folder name can only use a-z, 0-9, - and _.");
  return p;
}

async function preview() {
  // Rendered from the live copy, 1600×900 scaled to 1280×720, as the card image.
  const prev = state.view;
  if (prev !== "place") { state.view = "place"; applyPreview(); }
  $("pop").hidden = true;
  try {
    return await window.htmlToImage.toBlob($("desk"), { width: W, height: H, canvasWidth: 1280, canvasHeight: 720, pixelRatio: 1, style: { transform: "none" }, cacheBust: true });
  } finally {
    state.view = prev; applyPreview();
  }
}

async function download() {
  const p = problems();
  if (p.length) { msg(p.join(" "), true, "export-msg"); return; }
  if (!window.JSZip || !window.htmlToImage) { msg("The zip tools didn't load. Check your connection and reload the page.", true, "export-msg"); return; }
  const btn = $("download");
  btn.disabled = true; btn.textContent = "Making the .zip…";
  try {
    const id = themeId();
    const zip = new window.JSZip();
    const dir = zip.folder(id);
    const m = state.manifest;
    dir.file("manifest.json", JSON.stringify({ name: m.name.trim(), author: m.author.trim(), description: m.description.trim(), version: m.version.trim(), preview: "preview.png" }, null, 2) + "\n");
    dir.file("theme.json", JSON.stringify(themeJson(), null, 2) + "\n");
    if (state.base === "high-contrast") dir.file("widgets.json", HC_WIDGETS);
    if (state.background) dir.file(bgFile(state.background), state.background.data.split(",")[1], { base64: true });
    for (const f of [...state.fonts.ui, ...state.fonts.mono]) dir.file("fonts/" + f.name, f.data, { base64: true });
    const png = await preview();
    if (png) dir.file("preview.png", png);
    const blob = await zip.generateAsync({ type: "blob", compression: "DEFLATE" });
    const a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = id + ".zip";
    document.body.append(a); a.click(); a.remove();
    setTimeout(() => URL.revokeObjectURL(a.href), 10000);
    msg(`Saved ${id}.zip. Unzip it into your themes folder (below).`, false, "export-msg");
  } catch (e) {
    msg("Couldn't make the .zip: " + (e && e.message ? e.message : e), true, "export-msg");
  } finally {
    btn.disabled = false; btn.textContent = "Download theme .zip";
  }
}

// ---------- Import ----------

async function openZip(file) {
  if (!window.JSZip) { msg("The zip tools didn't load. Reload the page.", true); return; }
  try {
    const zip = await window.JSZip.loadAsync(file);
    const manifests = Object.keys(zip.files).filter(n => /(^|\/)manifest\.json$/.test(n)).sort((a, b) => a.length - b.length);
    if (!manifests.length) throw new Error("there's no manifest.json in it");
    const root = manifests[0].slice(0, -"manifest.json".length);
    const read = async n => (zip.file(root + n) ? zip.file(root + n).async("string") : null);
    const manifest = JSON.parse(await read("manifest.json"));
    const theme = JSON.parse((await read("theme.json")) || "{}");
    const next = fresh("default");
    for (const k of ["name", "author", "description", "version"]) if (typeof manifest[k] === "string") next.manifest[k] = manifest[k];
    next.id = root.split("/").filter(Boolean).pop() || "";
    let skipped = 0;
    for (const [k, v] of Object.entries(theme.colors || {})) {
      if (COLOR_KEYS.includes(k) && typeof v === "string" && validColor(v)) { if (v !== DEFAULT.colors[k]) next.colors[k] = v; }
      else skipped++;
    }
    for (const [k, v] of Object.entries(theme.sizes || {})) {
      if (SIZE_KEYS.includes(k) && typeof v === "number") next.sizes[k] = v; else skipped++;
    }
    if (theme.mode === "light" || theme.mode === "dark") next.mode = theme.mode;
    if (["opaque", "transparent", "blurred"].includes(theme.window)) next.window = theme.window;
    if (theme.hover && theme.hover.glow) next.hover = { on: true, glow: theme.hover.glow, radius: theme.hover.glow_radius ?? 10 };
    const bgName = theme.background && theme.background.image;
    if (bgName && zip.file(root + bgName)) {
      const ext = bgName.split(".").pop().toLowerCase();
      const type = { png: "image/png", jpg: "image/jpeg", jpeg: "image/jpeg", webp: "image/webp", gif: "image/gif" }[ext] || "image/png";
      const data = await zip.file(root + bgName).async("base64");
      next.background = { name: bgName.split("/").pop(), type, data: `data:${type};base64,${data}`, opacity: theme.background.opacity ?? 1, fit: theme.background.fit || "cover", layer: theme.background.layer || "behind", blur: Math.min(64, Math.max(0, Number(theme.background.blur) || 0)) };
    }
    const fonts = theme.fonts || {};
    for (const path of fonts.files || []) {
      const f = zip.file(root + path);
      if (!f) continue;
      const buf = await f.async("arraybuffer");
      const fam = fontFamily(buf);
      const slot = fam && fam === fonts.mono ? "mono" : "ui";
      next.fonts[slot].push({ name: path.split("/").pop(), family: fam || fonts[slot] || "", data: b64(buf) });
    }
    if (zip.file(root + "widgets.json") && !theme.colors) msg("Its widgets.json isn't editable here and won't be kept.");
    next.view = state.view;
    state = next;
    await registerFonts();
    changed();
    msg(`Opened ${manifest.name || file.name}.` + (skipped ? ` ${skipped} unknown token${skipped > 1 ? "s were" : " was"} skipped.` : ""));
  } catch (e) {
    msg(`Couldn't open ${file.name}: ${e && e.message ? e.message : e}.`, true);
  }
}

// ---------- Wire up ----------

function init() {
  buildColours();
  buildSizes();
  buildPreview();

  for (const k of ["name", "author", "description", "version"]) {
    $("m-" + k).addEventListener("input", e => { state.manifest[k] = e.target.value; changed(); });
  }
  $("m-id").addEventListener("input", e => { state.id = e.target.value.trim(); changed(); });

  segs.push(seg("base", () => state.base, v => {
    // Switching the starting theme keeps your own changes on top of it.
    state.base = v;
    state.mode = null;
    for (const k of Object.keys(state.colors)) if (state.colors[k] === baseColor(k)) delete state.colors[k];
  }));
  segs.push(seg("mode", mode, v => (state.mode = v === (baseTheme().mode || "dark") ? null : v)));
  segs.push(seg("window", () => state.window, v => { state.window = v; if (v !== "opaque") seeThrough("your desktop"); }));
  segs.push(seg("bg-fit", () => state.background && state.background.fit, v => state.background && (state.background.fit = v)));
  segs.push(seg("bg-layer", () => state.background && state.background.layer, v => { if (state.background) { state.background.layer = v; if (v === "behind") seeThrough("the image"); } }));
  $("panel-alpha").addEventListener("input", e => { setPanelAlpha(Number(e.target.value)); changed(); });
  $("bg-blur").addEventListener("input", e => { if (state.background) { state.background.blur = Number(e.target.value); changed(); } });
  $("wallpaper").addEventListener("click", newWallpaper);
  segs.push(seg("pv-view", () => state.view, v => (state.view = v)));

  $("glow-on").addEventListener("change", e => { state.hover.on = e.target.checked; changed(); });
  $("glow-radius").addEventListener("input", e => { state.hover.radius = Number(e.target.value); changed(); });
  $("bg-opacity").addEventListener("input", e => { if (state.background) { state.background.opacity = Number(e.target.value); changed(); } });
  $("bg-file").addEventListener("change", e => {
    const f = e.target.files[0];
    e.target.value = "";
    if (!f) return;
    if (f.size > 16 * 1024 * 1024) { msg("Background images have to be under 16 MiB.", true); return; }
    const r = new FileReader();
    r.onload = () => {
      state.background = Object.assign({ opacity: 0.8, fit: "cover", layer: "behind", blur: 0 }, state.background || {}, { name: f.name, type: f.type, data: r.result });
      if (state.background.layer === "behind") seeThrough("the image");
      changed();
    };
    r.readAsDataURL(f);
  });
  $("bg-clear").addEventListener("click", () => { state.background = null; changed(); });
  for (const slot of ["ui", "mono"]) {
    $("font-" + slot).addEventListener("change", e => { const files = [...e.target.files]; e.target.value = ""; addFonts(slot, files); });
  }
  $("open-zip").addEventListener("change", e => { const f = e.target.files[0]; e.target.value = ""; if (f) openZip(f); });
  $("reset").addEventListener("click", () => {
    if (!confirm("Start over? Your colours, sizes and files here are cleared.")) return;
    state = fresh(state.base);
    changed();
  });
  $("download").addEventListener("click", download);

  newWallpaper();
  window.addEventListener("resize", fit);
  new ResizeObserver(fit).observe($("stage"));
  registerFonts();
  changed();
  fit();
}

init();
