// Runs Inkboard (Rust, compiled to WebAssembly) in the page: restores saved
// boards from IndexedDB, loads the board fonts, then drives the app with
// frames, pointer, wheel and keyboard input, and answers its requests.

import init, { Inkboard, add_font, preload, take_changes, take_requests } from "./inkboard_web.js";

const VERSION = "__VERSION__";
const FONTS = [
  "inter", "opensans", "lato", "montserrat", "poppins", "nunito", "oswald", "raleway", "ubuntu", "merriweather",
  "playfair", "lora", "robotoslab", "jetbrains", "sourcecode", "caveat", "patrick", "kalam", "indie", "architects",
  "shadows", "comicneue", "marker", "amatic", "pacifico", "greatvibes", "lobster", "bebas", "notodeva", "hind", "tiro",
];
const PDFJS = "https://cdnjs.cloudflare.com/ajax/libs/pdf.js/4.10.38/pdf.min.mjs";
const PDFJS_WORKER = "https://cdnjs.cloudflare.com/ajax/libs/pdf.js/4.10.38/pdf.worker.min.mjs";
const MAX_FILE = 32 << 20;

const canvas = document.getElementById("board");
const ctx = canvas.getContext("2d", { alpha: false });
const field = document.getElementById("field");
const statusEl = document.getElementById("status");
const progressEl = document.getElementById("progress");

let wasm, app;
let dpr = 1;
let fieldOpen = false;
let fieldMultiline = false;

// ------------------------------------------------------------ storage

function openDb() {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open("inkboard", 1);
    req.onupgradeneeded = () => req.result.createObjectStore("files");
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

function readAll(db) {
  return new Promise((resolve, reject) => {
    const out = [];
    const req = db.transaction("files").objectStore("files").openCursor();
    req.onsuccess = () => {
      const c = req.result;
      if (!c) return resolve(out);
      out.push([c.key, c.value]);
      c.continue();
    };
    req.onerror = () => reject(req.error);
  });
}

let db = null;
function persist() {
  const changes = take_changes();
  if (!db || changes.length === 0) return;
  const store = db.transaction("files", "readwrite").objectStore("files");
  for (let i = 0; i < changes.length; i += 2) {
    if (changes[i + 1] === null) store.delete(changes[i]);
    else store.put(changes[i + 1], changes[i]);
  }
}

// ------------------------------------------------------------ drawing

let wantFrame = true;

function draw(now) {
  if (!app.frame(now)) return;
  const w = app.width(), h = app.height();
  if (w !== canvas.width || h !== canvas.height || w === 0 || h === 0) return;
  const [x0, y0, x1, y1] = app.dirty();
  const pixels = new Uint8ClampedArray(wasm.memory.buffer, app.pixels(), w * h * 4);
  const image = new ImageData(pixels, w, h);
  const x = Math.max(0, x0), y = Math.max(0, y0);
  ctx.putImageData(image, 0, 0, x, y, Math.min(w, x1) - x, Math.min(h, y1) - y);
}

function tick(t) {
  requestAnimationFrame(tick);
  const deadline = app.next_deadline();
  if (wantFrame || app.needs_frame() || (deadline >= 0 && deadline <= t)) {
    wantFrame = false;
    draw(t);
    handleRequests();
    persist();
  }
}

// Desktops sit closer than wall panels, so the interface is a little larger
// there; touch screens use the browser's own scale.
function uiScale() {
  return dpr * (matchMedia("(pointer: fine)").matches ? 1.2 : 1.0);
}

function resize() {
  dpr = window.devicePixelRatio || 1;
  rect = canvas.getBoundingClientRect();
  const w = Math.round(canvas.clientWidth * dpr), h = Math.round(canvas.clientHeight * dpr);
  if (w === canvas.width && h === canvas.height && app) return;
  canvas.width = w;
  canvas.height = h;
  app?.resize(w, h, uiScale());
  wantFrame = true;
}

// ------------------------------------------------------------ input

function after() {
  wantFrame = true;
  // Requests answered inside the input event can open pickers, tabs and the keyboard.
  handleRequests();
}

let rect = canvas.getBoundingClientRect();
const pos = (e) => [(e.clientX - rect.left) * dpr, (e.clientY - rect.top) * dpr];
const isStylus = (e) => e.pointerType === "pen";
const isEraser = (e) => e.pointerType === "pen" && ((e.buttons & 32) !== 0 || e.button === 5);
const contact = (e) => (e.pointerType === "touch" ? Math.max(e.width, e.height) * dpr : 0);
const pressed = new Set();

canvas.addEventListener("pointerdown", (e) => {
  if (e.pointerType === "mouse" && e.button !== 0 && e.button !== 1) return;
  e.preventDefault();
  canvas.focus({ preventScroll: true });
  try { canvas.setPointerCapture(e.pointerId); } catch {}
  pressed.add(e.pointerId);
  const [x, y] = pos(e);
  app.pointer("down", e.pointerId, x, y, e.timeStamp, isStylus(e), isEraser(e), contact(e));
  after();
});

canvas.addEventListener("pointermove", (e) => {
  if (!pressed.has(e.pointerId)) {
    if (e.pointerType !== "touch") {
      const [x, y] = pos(e);
      app.hover(x, y);
      wantFrame = true;
    }
    return;
  }
  const events = e.getCoalescedEvents ? e.getCoalescedEvents() : [e];
  for (const c of events.length ? events : [e]) {
    const [x, y] = pos(c);
    app.pointer("move", e.pointerId, x, y, c.timeStamp, isStylus(e), isEraser(e), contact(c));
  }
  wantFrame = true;
});

function lift(e, phase) {
  if (!pressed.delete(e.pointerId)) return;
  const [x, y] = pos(e);
  app.pointer(phase, e.pointerId, x, y, e.timeStamp, isStylus(e), isEraser(e), contact(e));
  after();
}
canvas.addEventListener("pointerup", (e) => lift(e, "up"));
canvas.addEventListener("pointercancel", (e) => lift(e, "cancel"));
canvas.addEventListener("pointerleave", (e) => {
  if (e.pointerType !== "touch" && !pressed.has(e.pointerId)) {
    app.hover_exit();
    wantFrame = true;
  }
});
canvas.addEventListener("contextmenu", (e) => e.preventDefault());

canvas.addEventListener("wheel", (e) => {
  e.preventDefault();
  const unit = e.deltaMode === 1 ? 60 : e.deltaMode === 2 ? canvas.height : 1;
  const [x, y] = pos(e);
  // Trackpad pinches arrive as wheel events with ctrlKey set.
  const zoom = e.ctrlKey || e.metaKey;
  app.wheel(x, y, e.deltaX * unit, e.deltaY * unit * (zoom && e.deltaMode === 0 ? 2 : 1), zoom);
  wantFrame = true;
}, { passive: false });

window.addEventListener("keydown", (e) => {
  if (fieldOpen) return;
  const command = e.ctrlKey || e.metaKey;
  let handled = true;
  switch (e.key) {
    case "Escape": app.back(); break;
    case "ArrowUp": app.key(19); break;
    case "ArrowDown": app.key(20); break;
    case "ArrowLeft": app.key(21); break;
    case "ArrowRight": app.key(22); break;
    case "Enter": app.key(66); break;
    case "Delete":
    case "Backspace": app.shortcut("delete"); break;
    default:
      if (!command) { handled = false; break; }
      switch (e.key.toLowerCase()) {
        case "z": app.shortcut(e.shiftKey ? "redo" : "undo"); break;
        case "y": app.shortcut("redo"); break;
        case "=": case "+": app.shortcut("zoomIn"); break;
        case "-": app.shortcut("zoomOut"); break;
        case "0": app.shortcut("fit"); break;
        default: handled = false;
      }
  }
  if (handled) {
    e.preventDefault();
    after();
  }
});

// The text field the app is editing (the on-screen keyboard on phones).
field.addEventListener("input", () => {
  app.text_input(field.value, false);
  wantFrame = true;
});
field.addEventListener("keydown", (e) => {
  if (e.key === "Enter" && !(fieldMultiline && e.shiftKey) && !e.isComposing) {
    e.preventDefault();
    app.text_input(field.value, true);
    after();
  } else if (e.key === "Escape") {
    e.preventDefault();
    app.back();
    after();
  }
});

// Browser back (and Android's back gesture) steps back inside the app first.
history.pushState({ inkboard: true }, "");
window.addEventListener("popstate", () => {
  if (app.back()) {
    history.pushState({ inkboard: true }, "");
    after();
  } else {
    history.back();
  }
});

// ------------------------------------------------------------ requests

function download(name, blob) {
  const a = document.createElement("a");
  a.href = URL.createObjectURL(blob);
  a.download = name;
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 10000);
}

function choose(accept, multiple) {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = accept;
    input.multiple = multiple;
    input.addEventListener("change", () => resolve([...input.files]));
    input.addEventListener("cancel", () => resolve([]));
    input.click();
  });
}

async function pdfPages(file) {
  const pdfjs = await import(PDFJS);
  pdfjs.GlobalWorkerOptions.workerSrc = PDFJS_WORKER;
  const doc = await pdfjs.getDocument({ data: new Uint8Array(await file.arrayBuffer()) }).promise;
  const pages = [];
  for (let n = 1; n <= Math.min(doc.numPages, 60); n++) {
    const page = await doc.getPage(n);
    const base = page.getViewport({ scale: 1 });
    // About 2000 pixels on the long side, like the Android importer.
    const viewport = page.getViewport({ scale: Math.min(4, 2000 / Math.max(base.width, base.height)) });
    const c = document.createElement("canvas");
    c.width = Math.ceil(viewport.width);
    c.height = Math.ceil(viewport.height);
    const g = c.getContext("2d");
    g.fillStyle = "#fff";
    g.fillRect(0, 0, c.width, c.height);
    await page.render({ canvasContext: g, viewport }).promise;
    const blob = await new Promise((r) => c.toBlob(r, "image/png"));
    pages.push(new Uint8Array(await blob.arrayBuffer()));
  }
  return pages;
}

async function pickFile(token, kind) {
  const accept = { image: "image/png,image/jpeg,image/webp,image/gif,image/bmp", pdf: "application/pdf,.pdf", document: ".svg,.pptx,.docx,.xlsx" }[kind];
  const [file] = await choose(accept, false);
  try {
    if (!file) app.file_failed(token, "");
    else if (file.size > MAX_FILE) app.file_failed(token, "Files must be 32 MB or smaller");
    else if (kind === "pdf") app.pdf_chosen(token, await pdfPages(file));
    else app.file_chosen(token, file.name, file.type || "application/octet-stream", new Uint8Array(await file.arrayBuffer()));
  } catch (err) {
    app.file_failed(token, `Could not read the file (${err.message || err})`);
  }
  wantFrame = true;
}

async function pickBoards() {
  const files = await choose(".json,application/json", true);
  if (files.length) {
    app.import_boards(JSON.stringify(await Promise.all(files.map((f) => f.text()))));
    wantFrame = true;
  }
}

function handleRequests() {
  const list = JSON.parse(take_requests());
  for (const r of list) {
    switch (r.t) {
      case "keyboard":
        fieldOpen = true;
        fieldMultiline = r.multiline;
        field.value = r.text;
        field.focus({ preventScroll: true });
        field.setSelectionRange(field.value.length, field.value.length);
        break;
      case "hideKeyboard":
        fieldOpen = false;
        field.blur();
        canvas.focus({ preventScroll: true });
        break;
      case "copy":
        navigator.clipboard?.writeText(r.text).catch(() => {});
        break;
      case "open":
        window.open(r.url, "_blank", "noopener");
        break;
      case "signin": {
        // Google's account chooser in this tab; it comes back to /app/#signin=…
        const state = Array.from(crypto.getRandomValues(new Uint8Array(18)), (b) => b.toString(16).padStart(2, "0")).join("");
        try { sessionStorage.setItem("inkboard-signin", state); } catch {}
        app.save_now();
        persist();
        location.href = `/auth/google?return=web&state=${state}`;
        break;
      }
      case "png": {
        const bytes = Uint8Array.from(atob(r.data), (c) => c.charCodeAt(0));
        download(`${(r.name || "Inkboard").replace(/[^\w \-]/g, "_")}.png`, new Blob([bytes], { type: "image/png" }));
        app.toast("Picture downloaded");
        break;
      }
      case "pickFile":
        pickFile(r.token, r.kind);
        break;
      case "pickBoards":
        pickBoards();
        break;
      case "ink":
        app.ink_result(r.token);
        app.toast("Handwriting to text needs an Android panel for now");
        break;
      case "updates":
        app.no_updates();
        break;
      case "exit":
        location.href = "/";
        break;
    }
  }
  if (list.length) wantFrame = true;
}

// ------------------------------------------------------------ links

// Back from Google (#signin=<token>&state=…), or a shared board (#board=<id>).
function handleHash() {
  const q = new URLSearchParams(location.hash.slice(1));
  if (![...q.keys()].length) return;
  history.replaceState(history.state, "", location.pathname + location.search);
  let expected = null;
  try {
    expected = sessionStorage.getItem("inkboard-signin");
    sessionStorage.removeItem("inkboard-signin");
  } catch {}
  if (q.has("signin") || q.has("signin_error")) {
    if (!expected || q.get("state") !== expected) app.sign_in_failed("Sign-in didn't start here. Try again.");
    else if (q.has("signin")) app.finish_sign_in(q.get("signin"));
    else app.sign_in_failed(q.get("signin_error"));
  }
  if (q.has("board")) app.open_board_link(q.get("board"));
  after();
}

// ------------------------------------------------------------ start

function fail(message) {
  statusEl.innerHTML = `${message} <a href="/">Back to the website</a>`;
}

async function start() {
  if (!("WebAssembly" in window) || !window.indexedDB) {
    return fail("This browser can't run Inkboard. Try a recent Chrome, Edge, Safari or Firefox.");
  }
  try {
    [wasm, db] = await Promise.all([init(), openDb().catch(() => null)]);
  } catch (err) {
    return fail(`Inkboard couldn't start (${err.message || err}).`);
  }
  if (db) {
    for (const [path, bytes] of await readAll(db)) preload(path, bytes);
  }
  statusEl.textContent = "Loading fonts…";
  let loaded = 0;
  await Promise.all(FONTS.map(async (id) => {
    try {
      const r = await fetch(`fonts/${id}.ttf`);
      if (r.ok) add_font(id, new Uint8Array(await r.arrayBuffer()));
    } catch {}
    progressEl.style.width = `${(++loaded / FONTS.length) * 100}%`;
  }));
  app = new Inkboard(location.origin, VERSION, -new Date().getTimezoneOffset());
  window.inkboard = app; // for debugging from the console
  resize();
  new ResizeObserver(resize).observe(canvas);
  matchMedia(`(resolution: ${dpr}dppx)`).addEventListener?.("change", resize);
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "hidden") {
      app.save_now();
      persist();
    }
  });
  window.addEventListener("pagehide", () => {
    app.save_now();
    persist();
  });
  handleHash();
  window.addEventListener("hashchange", handleHash);
  requestAnimationFrame(tick);
  document.getElementById("splash").classList.add("gone");
  canvas.focus({ preventScroll: true });
}

start();
