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
    background: null, // { name, type, data (data URL), opacity, fit, layer }
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
  text2: ["Secondary text", "Labels, inactive tabs"],
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

const ICON = {
  copy: '<rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2"/>',
  paste: '<rect x="8" y="2" width="8" height="4" rx="1"/><path d="M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2"/>',
  cut: '<circle cx="6" cy="6" r="3"/><circle cx="6" cy="18" r="3"/><path d="M20 4 8.1 15.9M14.5 14.5 20 20M8.1 8.1 12 12"/>',
  dup: '<rect x="8" y="8" width="13" height="13" rx="2"/><path d="M4 16V5a1 1 0 0 1 1-1h11"/>',
  select: '<path d="M4.04 4.69a.5.5 0 0 1 .65-.65l16 6.5a.5.5 0 0 1-.06.95l-6.13 1.58a2 2 0 0 0-1.43 1.43l-1.58 6.13a.5.5 0 0 1-.95.06z"/>',
  move: '<path d="M12 2v20M2 12h20M9 5l3-3 3 3M9 19l3 3 3-3M5 9l-3 3 3 3M19 9l3 3-3 3"/>',
  scale: '<path d="M21 3 14 10M21 3h-6M21 3v6M3 21l7-7M3 21h6M3 21v-6"/>',
  rotate: '<path d="M21 12a9 9 0 1 1-3-6.7L21 8"/><path d="M21 3v5h-5"/>',
  transform: '<rect x="4" y="4" width="16" height="16" rx="2" stroke-dasharray="3 3"/>',
  align: '<path d="M4 4v16M8 8h10M8 16h6"/>',
  grid: '<path d="M4 9h16M4 15h16M9 4v16M15 4v16"/>',
  angle: '<path d="M3 12a9 9 0 1 0 9-9"/><path d="M3 3v6h6"/>',
  part: '<path d="M21 8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16Z"/><path d="M3.3 7 12 12l8.7-5M12 22V12"/>',
  script: '<path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><path d="M14 2v6h6M10 13l-2 2 2 2M14 17l2-2-2-2"/>',
  ui: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M3 9h18"/>',
  box: '<path d="M21 8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16Z"/><path d="m3.3 7 8.7 5 8.7-5M12 22V12"/>',
  terrain: '<path d="m3 20 6-12 4 7 3-4 5 9z"/>',
  explorer: '<path d="M3 5h6M7 5v14h4M7 12h4M14 12h7M14 19h7M14 5h7"/>',
  props: '<path d="M4 6h10M18 6h2M4 12h4M12 12h8M4 18h12M20 18h0"/><circle cx="16" cy="6" r="2"/><circle cx="10" cy="12" r="2"/><circle cx="18" cy="18" r="2"/>',
  output: '<path d="m5 8 4 4-4 4M12 17h7"/>',
  play: '<path d="M7 4v16l13-8z"/>',
};
const icon = (n, cls = "i") => `<svg class="${cls}" viewBox="0 0 24 24" aria-hidden="true">${ICON[n]}</svg>`;
const LOGO = '<svg class="e-logo" viewBox="0 0 23.8 17" fill="currentColor" aria-hidden="true"><path d="M15.3 5.196L9.623 1.508L5.389 1.443Q4.405 1.428 4.138 2.374L0 17L5.453 17Q6.059 17 6.223 16.551L7.249 13.728L5.998 13.728Q5.91 13.728 5.966 13.536L8.34 5.357L14.177 9.174L15.3 5.196Z"/><path d="M23.8 0L18.989 0Q18.203 0 17.962 0.674L16.84 3.817L18.026 3.817Q18.147 3.817 18.091 4.009L15.589 12.606L9.526 8.596L8.404 12.574L13.504 16.006Q13.837 16.23 14.017 16.23L18.54 16.23Q19.25 16.23 19.502 15.332L23.8 0Z"/></svg>';

function cls(name) { return `icons/${mode() === "light" ? "light" : "dark"}/${name}.svg`; }

function buildPreview() {
  const tile = (i, label, on) => `<div class="e-tile hov${on ? " on" : ""}">${icon(i)}<span>${label}</span></div>`;
  const node = (depth, icn, name, tw, sel) =>
    `<div class="e-node hov${sel ? " sel" : ""}" style="padding-left:${4 + depth * 16}px"><span class="tw ${tw}"></span><img data-icon="${icn}" alt="">${name}</div>`;
  const prop = (label, val) => `<div class="e-prop hov"><span>${label}</span>${val}</div>`;
  $("ed").innerHTML = `
    <div class="bg" id="bg"></div>
    <div class="e-top">${LOGO}
      ${["Home", "Avatar", "UI", "Script", "Model", "Test", "Plugins"].map((t, i) => `<span class="e-tab hov${i === 0 ? " on" : ""}">${t}</span>`).join("")}
      <span class="sp"></span><span class="e-place">Village.rbxl</span><span class="e-avatar">C</span>
    </div>
    <div class="e-ribbon">
      <div class="e-group">${tile("copy", "Copy")}<div class="e-stack"><div class="hov">${icon("paste")}Paste</div><div class="hov">${icon("cut")}Cut</div><div class="hov">${icon("dup")}Duplicate</div></div></div>
      <span class="e-sep"></span>
      <div class="e-group"><div class="e-tools">${["select", "move", "scale", "rotate", "transform", "align"].map((t, i) => `<span class="e-tool hov${i === 0 ? " on" : ""}">${icon(t)}</span>`).join("")}</div>
        <div class="e-fields"><span class="e-field">${icon("grid")}1 stud</span><span class="e-field">${icon("angle")}45°</span></div></div>
      <span class="e-sep"></span>
      <div class="e-group">${tile("part", "Part")}${tile("script", "Script")}${tile("ui", "UI")}${tile("box", "Toolbox")}${tile("terrain", "Terrain")}</div>
      <span class="e-sep"></span>
      <div class="e-group">${tile("explorer", "Explorer", true)}${tile("props", "Properties", true)}${tile("output", "Output", true)}</div>
      <span class="e-sep"></span>
      <div class="e-group">${tile("play", "Play")}</div>
    </div>
    <div class="e-body">
      <section class="e-dock">
        <div class="e-dockhead"><b>Model "House"</b><span class="x">✕</span><span class="more">···</span></div>
        <div class="e-search">Filter Properties (Ctrl+Shift+P)</div>
        <div class="e-props">
          <div class="e-sel"><img data-icon="model" alt="">House <small>– Village</small></div>
          <div class="e-sec">APPEARANCE</div>
          ${prop("LevelOfDetail", '<span class="e-popwrap"><span class="e-val sel focus">Automatic</span><span class="e-pop" id="pop"><span>Automatic</span><span class="on">StreamingMesh</span><span>Disabled</span></span></span>')}
          ${prop("Transparency", '<span class="e-slider"><i></i><b>0.6</b></span>')}
          <div class="e-sec">DATA</div>
          ${prop("ClassName", '<span class="e-val ro">"Model"</span>')}
          ${prop("Name", '<span class="e-val">House</span>')}
          ${prop("Parent", '<span class="e-val ro">Village</span>')}
          <div class="e-sec">BEHAVIOR</div>
          ${prop("Anchored", '<span class="e-tog on"></span>')}
          ${prop("CanCollide", '<span class="e-tog"></span>')}
          ${prop("Archivable", '<span class="e-chk on"></span>')}
          ${prop("Locked", '<span class="e-chk"></span>')}
        </div>
      </section>
      <div class="e-center">
        <div class="e-view">
          <span class="e-badge">${icon("grid", "i")}Camera speed 1.0</span>
          <span class="e-readout">12.40 studs</span>
          <svg class="e-gizmo" viewBox="0 0 58 58" aria-hidden="true"><circle cx="29" cy="29" r="20" fill="var(--c-viewport_orient_body)" stroke="var(--c-viewport_orient_rim)" stroke-width="1.5"/><path d="M29 29 46 35" stroke="var(--c-viewport_axis_x)" stroke-width="2.5" stroke-linecap="round"/><path d="M29 29 29 9" stroke="var(--c-viewport_axis_y)" stroke-width="2.5" stroke-linecap="round"/><path d="M29 29 14 38" stroke="var(--c-viewport_axis_z)" stroke-width="2.5" stroke-linecap="round"/></svg>
        </div>
        <section class="e-out">
          <div class="e-dockhead"><b>Output</b><span class="x">✕</span><span class="e-chips"><span class="e-chip on">All</span><span class="e-chip">Warnings</span><span class="e-chip">Errors</span></span><span class="more">···</span></div>
          <div class="e-log">
            <div><time>14:02:11</time><span class="server">Server  Village loaded: 42 houses</span></div>
            <div><time>14:02:11</time><span class="client">Client  Hello from Cheeteau</span></div>
            <div><time>14:02:12</time><span class="info">Info    Autosaved to Village.rbxl</span></div>
            <div><time>14:02:14</time><span class="warn">Warning Infinite yield possible on 'Lamp'</span></div>
            <div class="err"><time>14:02:15</time><span>Error   attempt to index nil with 'Position'</span></div>
            <div><time>14:02:16</time><span class="server">Server  tick 3</span></div>
          </div>
        </section>
        <section class="e-script">
          <div class="e-stabs"><span class="e-stab">${icon("ui")}Place</span><span class="e-stab on"><img data-icon="script" alt="">Lights</span><span class="e-stab"><img data-icon="script" alt="">Doors</span></div>
          <div class="e-code">
            <div class="e-gutter">${Array.from({ length: 14 }, (_, i) => `<div>${i + 1}</div>`).join("")}</div>
            <div class="e-lines">
              <div><span class="c">-- Turns the street lamps on at dusk</span></div>
              <div><span class="k">local</span> Lighting = game:<span class="f">GetService</span>(<span class="s">"Lighting"</span>)</div>
              <div><span class="k">local</span> lamps = workspace.Village:<span class="f">GetDescendants</span>()</div>
              <div></div>
              <div><span class="k">local function</span> <span class="f">update</span>()</div>
              <div class="cur">    <span class="k">local</span> night = Lighting.ClockTime &gt; <span class="n">18</span> <span class="k">or</span> Lighting.ClockTime &lt; <span class="n">6</span></div>
              <div>    <span class="k">for</span> _, lamp <span class="k">in</span> <span class="hl">lamps</span> <span class="k">do</span></div>
              <div>        <span class="k">if</span> lamp:<span class="f">IsA</span>(<span class="s">"PointLight"</span>) <span class="k">then</span></div>
              <div>            lamp.Enabled = night</div>
              <div>        <span class="k">end</span></div>
              <div>    <span class="k">end</span></div>
              <div><span class="k">end</span></div>
              <div></div>
              <div>Lighting:<span class="f">GetPropertyChangedSignal</span>(<span class="s">"ClockTime"</span>):<span class="f">Connect</span>(<span class="sq">updat</span>)</div>
            </div>
          </div>
          <div class="e-status"><span>Ln 6, Col 18</span><span>Luau</span><span class="bad">1 error</span></div>
        </section>
      </div>
      <section class="e-dock right">
        <div class="e-dockhead"><b>Explorer</b><span class="x">✕</span><span class="more">···</span></div>
        <div class="e-search">Search</div>
        <div class="e-tree">
          ${node(0, "workspace", "Workspace", "open")}
          ${node(1, "camera", "Camera", "none")}
          ${node(1, "terrain", "Terrain", "none")}
          ${node(1, "folder", "Village", "open")}
          ${node(2, "model", "House", "")}
          ${node(2, "model", "House", "")}
          ${node(2, "model", "House", "", true)}
          ${node(2, "model", "Tree", "")}
          ${node(2, "part", "Baseplate", "none")}
          ${node(2, "script", "Lights", "none")}
          ${node(0, "players", "Players", "")}
          ${node(0, "replicated-storage", "ReplicatedStorage", "")}
          ${node(0, "server-script-service", "ServerScriptService", "")}
        </div>
      </section>
    </div>
`;
}

// Code colours: the toolkit's own palette for the mode (High contrast ships its own).
const CODE = {
  dark: { bg: "#1A1B1E", fg: "#D7D7DC", gutter: "#5C5C63", line: "#FFFFFF0A", kw: "#C792EA", str: "#C3E88D", num: "#F78C6C", fn: "#82AAFF", com: "#6B7080" },
  light: { bg: "#FFFFFF", fg: "#1B1B1F", gutter: "#8C8C94", line: "#0000000A", kw: "#7B30D0", str: "#1C7A33", num: "#B4501E", fn: "#1F5FC4", com: "#7A7F8C" },
  hc: { bg: "#000000", fg: "#FFFFFF", gutter: "#C8C8D0", line: "#FFFFFF1A", kw: "#B8C6FF", str: "#A8F0B0", num: "#FFD58F", fn: "#A6E2FF", com: "#D6CBA4" },
};

function applyPreview() {
  const ed = $("ed");
  const s = ed.style;
  for (const k of COLOR_KEYS) s.setProperty("--c-" + k, css(resolved(k)));
  for (const k of SIZE_KEYS) s.setProperty("--s-" + k, size(k));
  const code = state.base === "high-contrast" && mode() === "dark" ? CODE.hc : CODE[mode()];
  for (const [k, v] of Object.entries(code)) s.setProperty("--code-" + k, css(resolve(v)));
  s.setProperty("--glow", state.hover.on ? `0 0 ${state.hover.radius}px ${css(resolve(state.hover.glow))}` : "none");
  s.setProperty("--ui", family("ui") ? `"${family("ui")}"` : "Manrope");
  s.setProperty("--mono-font", family("mono") ? `"${family("mono")}"` : '"JetBrains Mono"');
  ed.classList.toggle("blurred", state.window === "blurred");
  ed.classList.toggle("script", state.view === "script");
  $("desk").classList.toggle("solid", state.window === "opaque");
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
  $("pop").hidden = state.view !== "place";
}

function fit() {
  const stage = $("stage"), scale = $("scale");
  const w = stage.clientWidth;
  const h = window.innerHeight - 190;
  const k = Math.min(w / 1280, Math.max(h, 320) / 800, 1.25);
  scale.style.transform = `scale(${k})`;
  stage.style.height = 800 * k + "px";
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
  }
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
  if (b) out.background = { image: bgFile(b), opacity: b.opacity, fit: b.fit, layer: b.layer };
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
  // Rendered from the live copy at its full 1280×800, as the card image.
  const prev = state.view;
  if (prev !== "place") { state.view = "place"; applyPreview(); }
  $("pop").hidden = true;
  try {
    return await window.htmlToImage.toBlob($("desk"), { width: 1280, height: 800, pixelRatio: 1, style: { transform: "none" }, cacheBust: true });
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
      next.background = { name: bgName.split("/").pop(), type, data: `data:${type};base64,${data}`, opacity: theme.background.opacity ?? 1, fit: theme.background.fit || "cover", layer: theme.background.layer || "behind" };
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
  segs.push(seg("window", () => state.window, v => (state.window = v)));
  segs.push(seg("bg-fit", () => state.background && state.background.fit, v => state.background && (state.background.fit = v)));
  segs.push(seg("bg-layer", () => state.background && state.background.layer, v => state.background && (state.background.layer = v)));
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
      state.background = Object.assign({ opacity: 0.8, fit: "cover", layer: "behind" }, state.background || {}, { name: f.name, type: f.type, data: r.result });
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

  window.addEventListener("resize", fit);
  new ResizeObserver(fit).observe($("stage"));
  registerFonts();
  changed();
  fit();
}

init();
