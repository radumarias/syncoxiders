// Theme lab (theme.html). Kept out of the page so the Content-Security-Policy
// needs no 'unsafe-inline' script source; loaded as a classic script at the
// end of <body>, where the inline script used to be.
const TOKENS = [
  ["bg", "Page / panel"],
  ["surface_lowest", "Terminal well"],
  ["surface_low", "Cards"],
  ["surface", "Header, chrome"],
  ["surface_high", "Hover, quiet pills"],
  ["primary", "Main buttons"],
  ["on_primary", "Text on primary"],
  ["secondary", "Accents, copied, status"],
  ["on_surface", "Body text"],
  ["on_surface_var", "Captions"],
  ["outline", "Outline buttons"],
  ["outline_var", "Hairlines"],
  ["error", "Warnings"],
];

const CLEAN = {
  light: {
    bg: "#e4e4e4", surface_lowest: "#fefefe", surface_low: "#ffffff", surface: "#f4f5f7",
    surface_high: "#dce8fc", primary: "#1674d8", on_primary: "#ffffff", secondary: "#007951",
    on_surface: "#151724", on_surface_var: "#5c6070", outline: "#4e5260", outline_var: "#c5c6cb",
    error: "#c4374a",
  },
  dark: {
    bg: "#151724", surface_lowest: "#0e1018", surface_low: "#1c1f2e", surface: "#23263a",
    surface_high: "#2e3250", primary: "#1a88fe", on_primary: "#06101c", secondary: "#04f2a2",
    on_surface: "#f5f6f8", on_surface_var: "#b1b2b8", outline: "#9aa0b0", outline_var: "#3a3e52",
    error: "#ff8b9a",
  },
  control_radius: 18, card_radius: 20,
};
const RUSTY = {
  light: {
    bg: "#faf6f2", surface_lowest: "#fffdfb", surface_low: "#f4ebe4", surface: "#edded4",
    surface_high: "#e2cabb", primary: "#b04216", on_primary: "#fff9f5", secondary: "#007158",
    on_surface: "#301c14", on_surface_var: "#5e4337", outline: "#846352", outline_var: "#d3bbad",
    error: "#b12e1e",
  },
  dark: {
    bg: "#100d0c", surface_lowest: "#0a0807", surface_low: "#1c1613", surface: "#231b17",
    surface_high: "#36271f", primary: "#ef7038", on_primary: "#220f07", secondary: "#44dab5",
    on_surface: "#f7ebe2", on_surface_var: "#d5beb0", outline: "#a88978", outline_var: "#51392e",
    error: "#ffb5a4",
  },
  control_radius: 10, card_radius: 16,
};

const state = {
  name: "",
  intent: "",
  author: "",
  control_radius: 18,
  card_radius: 20,
  light: { ...CLEAN.light },
  dark: { ...CLEAN.dark },
  edit: "light",
  previewDark: false,
  screen: "home",
  compact: false,
};

function hexOk(v) {
  return /^#[0-9a-fA-F]{6}$/.test(v);
}
function lum(hex) {
  const n = parseInt(hex.slice(1), 16);
  const srgb = [n >> 16, (n >> 8) & 255, n & 255].map((c) => {
    const x = c / 255;
    return x <= 0.04045 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * srgb[0] + 0.7152 * srgb[1] + 0.0722 * srgb[2];
}
function contrast(a, b) {
  if (!hexOk(a) || !hexOk(b)) return 0;
  const L1 = lum(a), L2 = lum(b);
  const hi = Math.max(L1, L2), lo = Math.min(L1, L2);
  return (hi + 0.05) / (lo + 0.05);
}

function slug() {
  return (state.name || "untitled").toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "untitled";
}

function bundle() {
  return {
    format: "oxfer-theme",
    version: 1,
    name: state.name.trim() || "Untitled",
    slug: slug(),
    intent: state.intent.trim(),
    author: state.author.trim(),
    control_radius: Number(state.control_radius) || 18,
    card_radius: Number(state.card_radius) || 20,
    outline_button_radius: 10,
    loading: {
      spinner: state.dark.primary,
      label: state.light.primary,
    },
    light: { ...state.light },
    dark: { ...state.dark },
  };
}

function rgbHex(r, g, b) {
  return "#" + [r, g, b].map((n) => Math.max(0, Math.min(255, Math.round(n))).toString(16).padStart(2, "0")).join("");
}
function mix(a, b, t) {
  return rgbHex(a.r + (b.r - a.r) * t, a.g + (b.g - a.g) * t, a.b + (b.b - a.b) * t);
}
function rgbOf(hex) {
  return { r: parseInt(hex.slice(1, 3), 16), g: parseInt(hex.slice(3, 5), 16), b: parseInt(hex.slice(5, 7), 16) };
}
function hslOf(hex) {
  const { r, g, b } = rgbOf(hex);
  const R = r / 255, G = g / 255, B = b / 255;
  const max = Math.max(R, G, B), min = Math.min(R, G, B);
  const l = (max + min) / 2;
  const d = max - min;
  let h = 0, s = 0;
  if (d) {
    s = d / (1 - Math.abs(2 * l - 1));
    h = max === R ? ((G - B) / d) % 6 : max === G ? (B - R) / d + 2 : (R - G) / d + 4;
    h *= 60; if (h < 0) h += 360;
  }
  return { hex, ...rgbOf(hex), h, s, l, chroma: d };
}
function distRgb(a, b) {
  return Math.abs(a.r - b.r) + Math.abs(a.g - b.g) + Math.abs(a.b - b.b);
}
function onColor(bg) {
  return contrast("#ffffff", bg) >= contrast("#111111", bg) ? "#ffffff" : "#111111";
}
function darkenToContrast(color, bg, minC) {
  let cur = hslOf(color);
  for (let i = 0; i < 24 && contrast(cur.hex, bg) < minC; i++) {
    cur = hslOf(rgbHex(cur.r * 0.88, cur.g * 0.88, cur.b * 0.88));
  }
  return cur.hex;
}
function lightenToContrast(color, bg, minC) {
  let cur = hslOf(color);
  for (let i = 0; i < 24 && contrast(cur.hex, bg) < minC; i++) {
    cur = hslOf(mix(cur, { r: 255, g: 255, b: 255 }, 0.14));
  }
  return cur.hex;
}

function extractSwatches(imageData, width, height) {
  const data = imageData.data;
  const pix = (x, y) => {
    const i = (y * width + x) * 4;
    return { r: data[i], g: data[i + 1], b: data[i + 2] };
  };
  const runsAt = (y) => {
    const runs = [];
    let start = 0, cur = pix(0, y);
    for (let x = 1; x < width; x++) {
      const c = pix(x, y);
      if (distRgb(c, cur) > 28) {
        if (x - start > width * 0.08) runs.push({ start, end: x, color: cur });
        start = x; cur = c;
      }
    }
    if (width - start > width * 0.08) runs.push({ start, end: width, color: cur });
    return runs;
  };
  const ys = [0.2, 0.35, 0.5, 0.65, 0.8].map((f) => Math.min(height - 1, Math.floor(height * f)));
  const found = [];
  for (const y of ys) {
    for (const run of runsAt(y)) {
      const hex = rgbHex(run.color.r, run.color.g, run.color.b);
      if (!found.some((s) => distRgb(s, run.color) < 36)) found.push({ ...run.color, hex });
    }
  }
  if (found.length >= 3) return found;
  const buckets = new Map();
  const step = 24;
  for (let y = 0; y < height; y += 2) {
    for (let x = 0; x < width; x += 2) {
      const c = pix(x, y);
      const key = (Math.round(c.r / step) << 16) | (Math.round(c.g / step) << 8) | Math.round(c.b / step);
      const prev = buckets.get(key) || { n: 0, r: 0, g: 0, b: 0 };
      prev.n++; prev.r += c.r; prev.g += c.g; prev.b += c.b;
      buckets.set(key, prev);
    }
  }
  return [...buckets.values()]
    .sort((a, b) => b.n - a.n)
    .slice(0, 8)
    .map((b) => {
      const r = b.r / b.n, g = b.g / b.n, bl = b.b / b.n;
      return { r, g, b: bl, hex: rgbHex(r, g, bl) };
    });
}

function themeFromSwatches(swatches) {
  const cols = swatches.map((s) => hslOf(s.hex)).sort((a, b) => a.l - b.l);
  const lightest = cols[cols.length - 1];
  const darkest = cols[0];
  const chromatic = [...cols].filter((c) => c.s > 0.18 && c.l > 0.12 && c.l < 0.92).sort((a, b) => b.s * (0.4 + b.chroma) - a.s * (0.4 + a.chroma));
  const primarySrc = chromatic[0] || cols[Math.floor(cols.length / 2)];
  const secondarySrc = chromatic.find((c) => Math.abs(c.h - primarySrc.h) > 25) || chromatic[1] || primarySrc;
  const navy = darkest.l < 0.25 ? darkest : hslOf(mix(darkest, { r: 0, g: 0, b: 0 }, 0.35));
  const paper = lightest.l > 0.82 ? lightest : hslOf(mix(lightest, { r: 255, g: 255, b: 255 }, 0.55));
  const gray = cols.find((c) => c.s < 0.12 && c.l > 0.4 && c.l < 0.8);
  const page = gray && gray.l > 0.7 ? gray : hslOf(mix(paper, navy, 0.08));

  const lightBg = page.hex;
  const lightSurface = mix(paper, { r: 255, g: 255, b: 255 }, 0.15);
  const lightPrimary = darkenToContrast(primarySrc.hex, "#ffffff", 4.5);
  const lightSecondary = darkenToContrast(secondarySrc.hex, "#ffffff", 4.5);
  const light = {
    bg: lightBg,
    surface_lowest: paper.hex,
    surface_low: "#ffffff",
    surface: lightSurface,
    surface_high: mix(rgbOf(lightSurface), rgbOf(lightPrimary), 0.16),
    primary: lightPrimary,
    on_primary: onColor(lightPrimary),
    secondary: lightSecondary,
    on_surface: navy.hex,
    on_surface_var: mix(navy, rgbOf(lightBg), 0.42),
    outline: mix(navy, rgbOf(lightBg), 0.28),
    outline_var: mix(navy, rgbOf(lightBg), 0.78),
    error: darkenToContrast("#d6455d", lightSurface, 4.5),
  };

  const darkBg = navy.hex;
  const darkPrimary = lightenToContrast(primarySrc.hex, darkBg, 4.5);
  const darkSecondary = lightenToContrast(secondarySrc.hex, darkBg, 4.5);
  const darkSurface = mix(navy, { r: 255, g: 255, b: 255 }, 0.08);
  const dark = {
    bg: darkBg,
    surface_lowest: mix(navy, { r: 0, g: 0, b: 0 }, 0.25),
    surface_low: mix(navy, { r: 255, g: 255, b: 255 }, 0.05),
    surface: darkSurface,
    surface_high: mix(navy, rgbOf(darkPrimary), 0.18),
    primary: darkPrimary,
    on_primary: onColor(darkPrimary),
    secondary: darkSecondary,
    on_surface: paper.hex,
    on_surface_var: gray ? gray.hex : mix(paper, navy, 0.35),
    outline: mix(paper, navy, 0.4),
    outline_var: mix(navy, { r: 255, g: 255, b: 255 }, 0.16),
    error: lightenToContrast("#ff8b9a", darkSurface, 4.5),
  };
  return { light, dark, swatches: cols.map((c) => c.hex) };
}

function showExtracted(hexes) {
  const el = document.getElementById("extracted");
  el.hidden = !hexes.length;
  el.innerHTML = hexes.map((h) => `<i style="background:${h}" title="${h}"></i>`).join("");
}

async function fillFromPaletteImage(file) {
  const url = URL.createObjectURL(file);
  try {
    const img = await new Promise((resolve, reject) => {
      const im = new Image();
      im.onload = () => resolve(im);
      im.onerror = () => reject(new Error("Could not read image"));
      im.src = url;
    });
    const max = 360;
    const scale = Math.min(1, max / Math.max(img.width, img.height));
    const w = Math.max(8, Math.round(img.width * scale));
    const h = Math.max(8, Math.round(img.height * scale));
    const canvas = document.createElement("canvas");
    canvas.width = w; canvas.height = h;
    const ctx = canvas.getContext("2d", { willReadFrequently: true });
    ctx.drawImage(img, 0, 0, w, h);
    const swatches = extractSwatches(ctx.getImageData(0, 0, w, h), w, h);
    if (swatches.length < 2) throw new Error("No color blocks found");
    const themed = themeFromSwatches(swatches);
    state.light = themed.light;
    state.dark = themed.dark;
    showExtracted(themed.swatches);
    renderTokens();
    renderAll();
    document.getElementById("status").textContent = "Filled light and dark from " + file.name + ". Tweak tokens, then export.";
  } finally {
    URL.revokeObjectURL(url);
  }
}

function applyPreset(preset, name) {
  state.light = { ...preset.light };
  state.dark = { ...preset.dark };
  state.control_radius = preset.control_radius;
  state.card_radius = preset.card_radius;
  if (!state.name) state.name = name;
  document.getElementById("name").value = state.name;
  document.getElementById("r-control").value = state.control_radius;
  document.getElementById("r-card").value = state.card_radius;
  renderTokens();
  renderAll();
}

function renderTokens() {
  const root = document.getElementById("tokens");
  const pal = state[state.edit];
  root.innerHTML = TOKENS.map(([key, role]) => `
    <div class="token" data-key="${key}">
      <span title="${role}">${key}</span>
      <input type="color" value="${hexOk(pal[key]) ? pal[key] : "#000000"}" aria-label="${key} picker">
      <input type="text" value="${pal[key]}" spellcheck="false" aria-label="${key} hex">
    </div>`).join("");
  root.querySelectorAll(".token").forEach((row) => {
    const key = row.dataset.key;
    const picker = row.querySelector('input[type=color]');
    const text = row.querySelector('input[type=text]');
    picker.addEventListener("input", () => {
      pal[key] = picker.value;
      text.value = picker.value;
      renderAll();
    });
    text.addEventListener("input", () => {
      let v = text.value.trim();
      if (v && v[0] !== "#") v = "#" + v;
      pal[key] = v;
      if (hexOk(v)) picker.value = v;
      renderAll();
    });
  });
}

function renderContrast() {
  const pal = state[state.edit];
  const pairs = [
    ["on_surface on bg", pal.on_surface, pal.bg, 4.5],
    ["on_surface on surface", pal.on_surface, pal.surface, 4.5],
    ["on_surface_var on surface", pal.on_surface_var, pal.surface, 4.5],
    ["on_primary on primary", pal.on_primary, pal.primary, 3],
    ["error on surface", pal.error, pal.surface, 4.5],
  ];
  document.getElementById("contrast").innerHTML = pairs.map(([label, a, b, min]) => {
    const c = contrast(a, b);
    const ok = c >= min;
    return `<div class="contrast ${ok ? "ok" : "bad"}">${label}: ${c ? c.toFixed(2) : "—"} : 1 ${ok ? "" : "(need " + min + ")"}</div>`;
  }).join("");
}

function renderPreview() {
  const pal = state.previewDark ? state.dark : state.light;
  const stage = document.getElementById("stage");
  stage.classList.toggle("compact", state.compact);
  const map = {
    "--bg": pal.bg, "--surface-lowest": pal.surface_lowest, "--surface-low": pal.surface_low,
    "--surface": pal.surface, "--surface-high": pal.surface_high, "--primary": pal.primary,
    "--on-primary": pal.on_primary, "--secondary": pal.secondary, "--on-surface": pal.on_surface,
    "--on-surface-var": pal.on_surface_var, "--outline": pal.outline, "--outline-var": pal.outline_var,
    "--error": pal.error, "--r-control": state.control_radius + "px", "--r-card": state.card_radius + "px",
  };
  Object.entries(map).forEach(([k, v]) => stage.style.setProperty(k, v));
  const mark = state.previewDark ? "assets/oxfer-wordmark-dark.svg" : "assets/oxfer-wordmark-light.svg";
  const header = `
    <div class="app-header">
      <img src="${mark}" alt="Oxfer">
      ${state.compact ? "" : '<span class="pill">READY</span>'}
      <span class="grow"></span>
      ${state.compact ? "" : `<button class="btn" type="button">${state.previewDark ? "Light mode" : "Dark mode"}</button>`}
      <button class="btn" type="button">Theme</button>
    </div>`;
  const home = `
    <div class="body">
      <div class="hero">
        <span class="pill">NO ACCOUNT NEEDED</span>
        <h2>Send files without cloud storage.</h2>
        <p class="muted">Pick a file, send them a link, and they save it on their device.</p>
        <p>Your file is encrypted while it travels. Oxfer doesn't store a copy on a server or in the cloud.</p>
        <button class="btn primary" type="button">Send files</button>
      </div>
      <p style="font-weight:700;font-size:21px;margin:20px 0 10px">From your device to theirs</p>
      <div class="steps">
        <div class="step"><h3>1. Pick a file</h3><p class="muted">Choose a photo, video, or document.</p></div>
        <div class="step"><h3>2. Send the link</h3><p class="muted">Send it to the person you want to share with.</p></div>
        <div class="step"><h3>3. They save it</h3><p class="muted">They open your link and choose where to save it.</p></div>
      </div>
    </div>`;
  const send = `
    <div class="body">
      <div class="card">
        <p style="font-weight:700;font-size:21px;margin:0 0 12px">Send a file</p>
        <div class="file"><div class="glyph">↑</div><div><strong>holiday-reel.mp4</strong><div class="muted">248 MB</div></div></div>
        <p class="muted" style="margin:12px 0 6px">Send this link</p>
        <div class="linkbox">https://oxfer.app/#ticket&cap=example</div>
        <div style="margin-top:12px"><button class="btn primary" type="button">Copy link</button></div>
        <p class="warn">Anyone with this link can download your files while you're sharing. Send it only to people you trust.</p>
      </div>
    </div>`;
  stage.innerHTML = header + (state.screen === "home" ? home : send) + `<div class="footer">▲ Terminal Output &gt;_</div>`;
}

function renderAll() {
  renderContrast();
  renderPreview();
}

function setPressed(id, on) {
  document.getElementById(id).setAttribute("aria-pressed", on ? "true" : "false");
}

function download(name, blob) {
  const a = document.createElement("a");
  a.href = URL.createObjectURL(blob);
  a.download = name;
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 1000);
}

function crc32(data) {
  let c = ~0;
  for (let i = 0; i < data.length; i++) {
    c ^= data[i];
    for (let k = 0; k < 8; k++) c = (c >>> 1) ^ (0xedb88320 & -(c & 1));
  }
  return ~c >>> 0;
}
function u16(n) { const b = new Uint8Array(2); new DataView(b.buffer).setUint16(0, n, true); return b; }
function u32(n) { const b = new Uint8Array(4); new DataView(b.buffer).setUint32(0, n, true); return b; }
function zipStore(files) {
  const enc = new TextEncoder();
  const locals = [];
  const centrals = [];
  let offset = 0;
  for (const file of files) {
    const name = enc.encode(file.name);
    const data = typeof file.data === "string" ? enc.encode(file.data) : file.data;
    const crc = crc32(data);
    const local = new Uint8Array(30 + name.length + data.length);
    local.set([0x50,0x4b,0x03,0x04, 0x14,0x00, 0x00,0x00, 0x00,0x00, 0x00,0x00,0x00,0x00], 0);
    local.set(u32(crc), 14);
    local.set(u32(data.length), 18);
    local.set(u32(data.length), 22);
    local.set(u16(name.length), 26);
    local.set(name, 30);
    local.set(data, 30 + name.length);
    const central = new Uint8Array(46 + name.length);
    central.set([0x50,0x4b,0x01,0x02, 0x14,0x00, 0x14,0x00, 0x00,0x00, 0x00,0x00, 0x00,0x00,0x00,0x00], 0);
    central.set(u32(crc), 16);
    central.set(u32(data.length), 20);
    central.set(u32(data.length), 24);
    central.set(u16(name.length), 28);
    central.set(u32(offset), 42);
    central.set(name, 46);
    locals.push(local);
    centrals.push(central);
    offset += local.length;
  }
  const cd = centrals.reduce((n, p) => n + p.length, 0);
  const eocd = new Uint8Array(22);
  eocd.set([0x50,0x4b,0x05,0x06], 0);
  eocd.set(u16(files.length), 8);
  eocd.set(u16(files.length), 10);
  eocd.set(u32(cd), 12);
  eocd.set(u32(offset), 16);
  const out = new Uint8Array(offset + cd + 22);
  let i = 0;
  for (const p of locals) { out.set(p, i); i += p.length; }
  for (const p of centrals) { out.set(p, i); i += p.length; }
  out.set(eocd, i);
  return out;
}

function applyBundle(data) {
  if (data.format !== "oxfer-theme") throw new Error("Not an oxfer-theme file");
  state.name = data.name || "";
  state.intent = data.intent || "";
  state.author = data.author || "";
  state.control_radius = data.control_radius ?? 18;
  state.card_radius = data.card_radius ?? 20;
  state.light = { ...CLEAN.light, ...(data.light || {}) };
  state.dark = { ...CLEAN.dark, ...(data.dark || {}) };
  document.getElementById("name").value = state.name;
  document.getElementById("intent").value = state.intent;
  document.getElementById("author").value = state.author;
  document.getElementById("r-control").value = state.control_radius;
  document.getElementById("r-card").value = state.card_radius;
  renderTokens();
  renderAll();
}

document.getElementById("name").addEventListener("input", (e) => { state.name = e.target.value; });
document.getElementById("intent").addEventListener("input", (e) => { state.intent = e.target.value; });
document.getElementById("author").addEventListener("input", (e) => { state.author = e.target.value; });
document.getElementById("r-control").addEventListener("input", (e) => { state.control_radius = e.target.value; renderAll(); });
document.getElementById("r-card").addEventListener("input", (e) => { state.card_radius = e.target.value; renderAll(); });
document.getElementById("edit-light").addEventListener("click", () => { state.edit = "light"; setPressed("edit-light", true); setPressed("edit-dark", false); renderTokens(); renderAll(); });
document.getElementById("edit-dark").addEventListener("click", () => { state.edit = "dark"; setPressed("edit-light", false); setPressed("edit-dark", true); renderTokens(); renderAll(); });
document.getElementById("prev-light").addEventListener("click", () => { state.previewDark = false; setPressed("prev-light", true); setPressed("prev-dark", false); renderAll(); });
document.getElementById("prev-dark").addEventListener("click", () => { state.previewDark = true; setPressed("prev-light", false); setPressed("prev-dark", true); renderAll(); });
document.getElementById("prev-home").addEventListener("click", () => { state.screen = "home"; setPressed("prev-home", true); setPressed("prev-send", false); renderAll(); });
document.getElementById("prev-send").addEventListener("click", () => { state.screen = "send"; setPressed("prev-home", false); setPressed("prev-send", true); renderAll(); });
document.getElementById("prev-wide").addEventListener("click", () => { state.compact = false; setPressed("prev-wide", true); setPressed("prev-compact", false); renderAll(); });
document.getElementById("prev-compact").addEventListener("click", () => { state.compact = true; setPressed("prev-wide", false); setPressed("prev-compact", true); renderAll(); });
document.getElementById("load-clean").addEventListener("click", () => applyPreset(CLEAN, state.name));
document.getElementById("load-rusty").addEventListener("click", () => applyPreset(RUSTY, state.name || "Rusty"));
document.getElementById("export-json").addEventListener("click", () => {
  const json = JSON.stringify(bundle(), null, 2);
  download(`oxfer-theme-${slug()}.json`, new Blob([json], { type: "application/json" }));
  document.getElementById("status").textContent = "Downloaded JSON. Send that file back to add the theme.";
});
document.getElementById("export-zip").addEventListener("click", () => {
  const json = JSON.stringify(bundle(), null, 2);
  const readme = `Oxfer theme bundle
==================
Theme: ${bundle().name}
Slug: ${slug()}

Give this zip (or just oxfer-theme.json) back in the Oxfer thread and ask to add the theme.
The JSON is the source of truth; it uses format "oxfer-theme" version 1.
`;
  const bytes = zipStore([
    { name: "oxfer-theme.json", data: json },
    { name: "README.txt", data: readme },
  ]);
  download(`oxfer-theme-${slug()}.zip`, new Blob([bytes], { type: "application/zip" }));
  document.getElementById("status").textContent = "Downloaded zip. Send it back to add the theme.";
});
document.getElementById("palette-image").addEventListener("change", async (e) => {
  const file = e.target.files[0];
  if (!file) return;
  try {
    await fillFromPaletteImage(file);
  } catch (err) {
    document.getElementById("status").textContent = "Palette image failed: " + err.message;
  }
  e.target.value = "";
});
document.getElementById("import").addEventListener("change", async (e) => {
  const file = e.target.files[0];
  if (!file) return;
  try {
    applyBundle(JSON.parse(await file.text()));
    document.getElementById("status").textContent = "Imported " + file.name;
  } catch (err) {
    document.getElementById("status").textContent = "Import failed: " + err.message;
  }
  e.target.value = "";
});

renderTokens();
renderAll();
