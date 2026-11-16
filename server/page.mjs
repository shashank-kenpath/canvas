// Served at /join and /pair: turns an invite link or a 6-digit board code
// into copy-paste setup for Claude. The room key stays in the browser.
export const connectPage = `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="robots" content="noindex">
<title>Canvas Studio shared board</title>
<style>
  :root { color-scheme: light dark; --bg:#f9fafb; --card:#fff; --ink:#1d1d1d; --dim:#6b7280; --line:#e8e8ec; --blue:#2f80ed; }
  @media (prefers-color-scheme: dark) { :root { --bg:#111214; --card:#1b1c1f; --ink:#f2f2f3; --dim:#9aa0aa; --line:#2c2d31; } }
  * { box-sizing: border-box; }
  body { margin:0; background:var(--bg); color:var(--ink); font:16px/1.5 system-ui,-apple-system,"Segoe UI",Roboto,sans-serif; }
  main { max-width:720px; margin:0 auto; padding:32px 16px 64px; }
  h1 { font-size:26px; margin:0 0 4px; } h2 { font-size:17px; margin:0 0 6px; }
  p { margin:0 0 10px; } .dim { color:var(--dim); font-size:14px; }
  section { background:var(--card); border:1px solid var(--line); border-radius:12px; padding:16px; margin-top:16px; }
  pre { margin:8px 0; padding:12px; background:var(--bg); border:1px solid var(--line); border-radius:8px; overflow-x:auto; font:13px/1.45 ui-monospace,Consolas,monospace; white-space:pre-wrap; word-break:break-all; }
  button { font:inherit; font-weight:600; border:0; border-radius:8px; padding:9px 14px; background:var(--blue); color:#fff; cursor:pointer; }
  button.ghost { background:transparent; color:var(--blue); border:1px solid var(--line); padding:6px 12px; font-size:14px; }
  input { font:inherit; font-size:24px; letter-spacing:4px; width:190px; padding:8px 12px; border:1px solid var(--line); border-radius:8px; background:var(--bg); color:var(--ink); }
  .row { display:flex; gap:10px; align-items:center; flex-wrap:wrap; } .err { color:#e03131; }
  [hidden] { display:none !important; }
</style>
</head>
<body>
<main>
  <h1>Canvas Studio shared board</h1>
  <p class="dim" id="name">Connect Claude or another device to a shared board.</p>

  <section id="ask">
    <h2>Enter the board code</h2>
    <p class="dim">In the app, open the board and tap <b>Share</b> to see its 6-digit code.</p>
    <div class="row">
      <input id="code" inputmode="numeric" autocomplete="off" maxlength="7" placeholder="123 456" aria-label="Board code">
      <button id="go">Continue</button>
    </div>
    <p class="err" id="error" hidden></p>
  </section>

  <div id="ready" hidden>
    <section>
      <h2>Claude Code</h2>
      <p class="dim">Run once in a terminal, then ask Claude to read or draw on the board.</p>
      <pre id="cc"></pre><button class="ghost" data-copy="cc">Copy</button>
    </section>
    <section>
      <h2>Claude Desktop</h2>
      <p class="dim">Settings &rarr; Connectors &rarr; Add custom connector, and paste this URL. If your plan has no custom connectors, add the JSON below to <code>claude_desktop_config.json</code> under <code>mcpServers</code> and restart Claude (needs Node.js).</p>
      <pre id="url"></pre><button class="ghost" data-copy="url">Copy URL</button>
      <pre id="json"></pre><button class="ghost" data-copy="json">Copy JSON</button>
    </section>
    <section>
      <h2>Another phone, tablet or display</h2>
      <p class="dim">In Canvas Studio choose <b>Join shared board</b> and paste this invite link.</p>
      <pre id="invite"></pre><button class="ghost" data-copy="invite">Copy</button>
    </section>
    <p class="dim" style="margin-top:16px">Anyone with these links can edit the board. Share them only with people you trust.</p>
  </div>
</main>
<script>
(function () {
  var $ = function (id) { return document.getElementById(id); };
  function show(room, key, name) {
    if (!/^[a-f0-9]{12}$/.test(room) || !/^[A-Za-z0-9_-]{43}$/.test(key)) return false;
    var mcp = location.origin + '/mcp/' + room + '/' + key;
    $('cc').textContent = 'claude mcp add --transport http canvas-studio ' + mcp;
    $('url').textContent = mcp;
    $('json').textContent = JSON.stringify({ 'canvas-studio': { command: 'npx', args: ['-y', 'mcp-remote', mcp] } }, null, 2);
    $('invite').textContent = location.origin + '/join?room=' + room + '&key=' + key;
    if (name) $('name').textContent = 'Board: ' + name;
    $('ask').hidden = true; $('ready').hidden = false;
    return true;
  }
  var query = new URLSearchParams(location.search);
  show(query.get('room') || '', query.get('key') || '', '');
  function fail(text) { $('error').textContent = text; $('error').hidden = false; }
  function submit() {
    var code = $('code').value.replace(/[^0-9]/g, '');
    if (code.length !== 6) return fail('Enter all 6 digits.');
    $('go').disabled = true; $('error').hidden = true;
    fetch('/api/join-code', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ code: code }) })
      .then(function (r) { return r.json().then(function (d) { return { ok: r.ok, status: r.status, d: d }; }); })
      .then(function (r) {
        if (!r.ok || !show(r.d.roomId, r.d.token, r.d.name)) fail(r.status === 429 ? 'Too many attempts. Wait a minute and try again.' : 'That code is not valid. Tap Share in the app for a new one.');
      })
      .catch(function () { fail('Could not reach the server. Check your connection.'); })
      .then(function () { $('go').disabled = false; });
  }
  $('go').addEventListener('click', submit);
  $('code').addEventListener('keydown', function (e) { if (e.key === 'Enter') submit(); });
  document.addEventListener('click', function (e) {
    var id = e.target && e.target.getAttribute && e.target.getAttribute('data-copy');
    if (!id) return;
    var label = e.target.textContent;
    navigator.clipboard.writeText($(id).textContent).then(function () {
      e.target.textContent = 'Copied'; setTimeout(function () { e.target.textContent = label; }, 1500);
    });
  });
})();
</script>
</body>
</html>
`;
