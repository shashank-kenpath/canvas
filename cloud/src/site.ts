// The public website at the Worker's root: what Inkboard is and where to
// download it. Downloads redirect to the newest GitHub release, so the page
// never needs updating for a new version.

export const RELEASES = "https://github.com/tejasgowda0012/canvas/releases";

/** Asset names in every release, made by .github/workflows/desktop.yml. */
export const DOWNLOADS: Record<string, string> = {
  mac: "Inkboard-macOS.dmg",
  windows: "Inkboard-Windows-x64.exe",
  linux: "Inkboard-Linux-x64.tar.gz",
  android: "Inkboard-Android.apk",
};

export function download(platform: string): Response | null {
  const asset = DOWNLOADS[platform];
  if (!asset) return null;
  return Response.redirect(`${RELEASES}/latest/download/${asset}`, 302);
}

export function homePage(): Response {
  return new Response(HTML, {
    headers: {
      "content-type": "text/html; charset=utf-8",
      "cache-control": "public, max-age=300",
      "x-content-type-options": "nosniff",
      "referrer-policy": "strict-origin-when-cross-origin",
    },
  });
}

const LOGO = `<svg viewBox="0 0 108 108" aria-hidden="true"><rect width="108" height="108" rx="25" fill="#6750d8"/><rect x="28" y="23" width="52" height="64" rx="8" fill="#fff"/><rect x="38" y="36" width="23" height="4" rx="2" fill="#e7e0ff"/><rect x="38" y="46" width="13" height="4" rx="2" fill="#e7e0ff"/><path d="M40 61L64 37L76 49L52 73Z" fill="#ff8966"/><path d="M64 37L68 33Q72 29 76 33L80 37Q84 41 80 45L76 49Z" fill="#ffb39b"/><path d="M44 65L68 41L72 45L48 69Z" fill="#ffa184"/><path d="M36 77L40 61L52 73Z" fill="#ffd8aa"/><path d="M36 77L38 69L44 75Z" fill="#453587"/></svg>`;

const HTML = `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Inkboard</title>
<meta name="description" content="Infinite whiteboards your team edits together, live, on panels, tablets, phones and computers.">
<link rel="icon" href="data:image/svg+xml,${encodeURIComponent(LOGO.replace('aria-hidden="true"', 'xmlns="http://www.w3.org/2000/svg"'))}">
<style>
  :root { color-scheme: light dark; --bg:#f7f6fb; --card:#fff; --ink:#1b1a22; --dim:#5f5d6b; --line:#e6e3ef; --brand:#6750d8; --brand-ink:#fff; --accent:#ff8966; }
  @media (prefers-color-scheme: dark) { :root { --bg:#121118; --card:#1c1b24; --ink:#f1f0f6; --dim:#a7a4b5; --line:#2d2b38; --brand:#8b78f0; --brand-ink:#fff; } }
  * { box-sizing: border-box; }
  body { margin:0; background:var(--bg); color:var(--ink); font:16px/1.55 system-ui,-apple-system,"Segoe UI",Roboto,sans-serif; }
  main { max-width:960px; margin:0 auto; padding:40px 16px 64px; }
  header { display:flex; align-items:center; gap:12px; }
  header svg { width:44px; height:44px; flex:none; }
  header strong { font-size:20px; letter-spacing:-0.01em; }
  .hero { padding:56px 0 32px; }
  h1 { font-size:clamp(32px, 6vw, 52px); line-height:1.08; letter-spacing:-0.025em; margin:0 0 16px; max-width:14ch; }
  .lede { font-size:19px; color:var(--dim); max-width:56ch; margin:0 0 28px; }
  .cta { display:flex; flex-wrap:wrap; gap:12px; align-items:center; }
  .btn { display:inline-flex; align-items:center; gap:8px; padding:12px 18px; border-radius:10px; font-weight:600; text-decoration:none; border:1px solid var(--line); color:var(--ink); background:var(--card); }
  .btn.primary { background:var(--brand); border-color:var(--brand); color:var(--brand-ink); }
  .btn:hover { filter:brightness(1.05); }
  .btn small { font-weight:500; opacity:.8; }
  .note { color:var(--dim); font-size:14px; margin-top:12px; }
  h2 { font-size:22px; letter-spacing:-0.01em; margin:48px 0 16px; }
  .grid { display:grid; grid-template-columns:repeat(auto-fit, minmax(210px, 1fr)); gap:14px; }
  .card { background:var(--card); border:1px solid var(--line); border-radius:14px; padding:18px; }
  .card h3 { margin:0 0 6px; font-size:16px; }
  .card p { margin:0; color:var(--dim); font-size:15px; }
  .dl .card { display:flex; flex-direction:column; gap:10px; }
  .dl .card .btn { justify-content:center; }
  ol { padding-left:20px; color:var(--dim); } ol li { margin:6px 0; } ol b { color:var(--ink); }
  footer { margin-top:56px; color:var(--dim); font-size:14px; display:flex; flex-wrap:wrap; gap:16px; }
  footer a, .note a { color:inherit; }
</style>
</head>
<body>
<main>
  <header>${LOGO}<strong>Inkboard</strong></header>

  <section class="hero">
    <h1>One infinite board, the whole team on it.</h1>
    <p class="lede">Write and draw on the big panel in the meeting room, keep going on your laptop, tablet or phone. Everyone edits the same board live, sees each other's cursors, and every change shows who made it.</p>
    <div class="cta">
      <a class="btn primary" id="primary" href="/download/mac">Download for macOS</a>
      <a class="btn" href="#download">Other platforms</a>
    </div>
    <p class="note">Sign in with your Kenpath Google account inside the app.</p>
  </section>

  <section>
    <h2>Built for working together</h2>
    <div class="grid">
      <div class="card"><h3>Live, on every device</h3><p>Each person has their own view, tools and selection. Edits appear on everyone's screen as they happen.</p></div>
      <div class="card"><h3>See who did what</h3><p>Named cursors and selections while you work. Select anything to see who added it and who changed it last.</p></div>
      <div class="card"><h3>Share in the org</h3><p>Invite colleagues to edit or view, or open a board to everyone at Kenpath. Undo only ever undoes your own changes.</p></div>
      <div class="card"><h3>Keeps working offline</h3><p>Boards live on each device. Changes made offline catch up when you reconnect.</p></div>
    </div>
  </section>

  <section id="download" class="dl">
    <h2>Download</h2>
    <div class="grid">
      <div class="card"><h3>macOS</h3><p>Apple Silicon and Intel, macOS 11 or later.</p><a class="btn primary" href="/download/mac">Download .dmg</a></div>
      <div class="card"><h3>Windows</h3><p>Windows 10 or 11, 64-bit.</p><a class="btn" href="/download/windows">Download .exe</a></div>
      <div class="card"><h3>Android</h3><p>Panels, tablets and phones, Android 7 or later.</p><a class="btn" href="/download/android">Download .apk</a></div>
      <div class="card"><h3>Linux</h3><p>64-bit, X11 or Wayland.</p><a class="btn" href="/download/linux">Download .tar.gz</a></div>
    </div>
    <p class="note">The macOS app isn't notarised yet. The first time, right-click Inkboard in Applications and choose <b>Open</b>. Windows may show a SmartScreen prompt: choose <b>More info → Run anyway</b>. <a href="${RELEASES}">All releases and notes</a></p>
  </section>

  <section>
    <h2>Get started</h2>
    <ol>
      <li><b>Install</b> the app for your device.</li>
      <li><b>Sign in</b>: Account → Sign in with Google. The app shows a code to approve on your phone or computer.</li>
      <li><b>Create a team board</b> on the Team boards tab, or copy a board from this device.</li>
      <li><b>Share</b> it: invite people by email, or open it to everyone in the org.</li>
    </ol>
  </section>

  <footer><span>Inkboard by Kenpath Labs</span><a href="${RELEASES}">Releases</a><a href="https://github.com/tejasgowda0012/canvas">Source</a></footer>
</main>
<script>
  (function () {
    var ua = navigator.userAgent, p = document.getElementById("primary");
    var pick = /Android/i.test(ua) ? ["android", "Android"] : /Windows/i.test(ua) ? ["windows", "Windows"] : /Linux/i.test(ua) && !/CrOS/i.test(ua) ? ["linux", "Linux"] : null;
    if (pick) { p.href = "/download/" + pick[0]; p.textContent = "Download for " + pick[1]; }
  })();
</script>
</body>
</html>`;
