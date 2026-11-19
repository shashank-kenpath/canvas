// The public website at the Worker's root: what Inkboard is, a live drawing
// demo, and where to download it. Downloads redirect to the newest GitHub release, so the page
// never needs updating for a new version.

export const RELEASES = "https://github.com/tejasgowda0012/inkboard/releases";

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

const LOGO = `<svg viewBox="0 0 56 56" aria-hidden="true"><rect width="56" height="56" rx="16" fill="#2748e8"/><polyline points="12,36 17,31 23,36 29,28 36,32 44,18" fill="none" stroke="#fff" stroke-width="5" stroke-linecap="round" stroke-linejoin="round"/></svg>`;

const HTML = String.raw`<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Inkboard: the infinite whiteboard for teams</title>
<meta name="description" content="One infinite board, the whole team on it. Live team whiteboards on meeting-room panels, tablets, phones and computers, by Kenpath Labs.">
<meta name="theme-color" content="#f6f3ec" media="(prefers-color-scheme: light)">
<meta name="theme-color" content="#0e0d14" media="(prefers-color-scheme: dark)">
<meta property="og:title" content="Inkboard: one board, no edges, everyone on it">
<meta property="og:description" content="An infinite whiteboard your whole team edits live, on panels, tablets, phones and computers.">
<link rel="icon" href="data:image/svg+xml,${encodeURIComponent(LOGO.replace('aria-hidden="true"', 'xmlns="http://www.w3.org/2000/svg"'))}">
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Bricolage+Grotesque:opsz,wght@12..96,400..800&family=Caveat:wght@500;700&family=Instrument+Serif:ital@0;1&family=Kalam:wght@400;700&display=swap">
<script>document.documentElement.className+=" js"</script>
<style>
  :root {
    color-scheme: light dark;
    --bg:#f6f3ec; --glass:rgba(246,243,236,.82); --surface:#fffdf8; --board:#fffefb; --ink:#16141f; --dim:#5b5869; --line:#e0dacb; --dot:#d6cfbd;
    --edge:#16141f; --shade:#16141f; --brand:#6750d8; --brand-2:#5640c6; --brand-ink:#fff; --coral:#e5562f; --teal:#0b8a7a; --sun:#ffd25e; --hl:#ffd25e; --violet:#b04fd6; --blue:#3e63dd;
    --display:"Bricolage Grotesque",ui-sans-serif,system-ui,-apple-system,"Segoe UI",Roboto,sans-serif;
    --serif:"Instrument Serif",Georgia,"Times New Roman",serif;
    --hand:"Caveat","Kalam","Segoe Print","Bradley Hand",cursive;
    --mono:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;
  }
  @media (prefers-color-scheme: dark) { :root {
    --bg:#0e0d14; --glass:rgba(14,13,20,.8); --surface:#17161f; --board:#15141d; --ink:#f3f1fa; --dim:#a9a6bb; --line:#2a2837; --dot:#2a2938;
    --edge:#ebe8f8; --shade:#8b78f0; --brand:#8b78f0; --brand-2:#9d8cf5; --brand-ink:#120f24; --coral:#ff8a68; --teal:#2ccdb7; --hl:#6b5400; --violet:#d08bf2; --blue:#7f9bff;
  } }
  * { box-sizing:border-box; }
  html { scroll-behavior:smooth; scroll-padding-top:76px; overflow-x:clip; -webkit-text-size-adjust:100%; }
  body { margin:0; background:var(--bg); color:var(--ink); font:400 17px/1.55 var(--display); overflow-x:clip; -webkit-font-smoothing:antialiased; }
  img, svg { display:block; max-width:100%; }
  a { color:inherit; }
  :focus-visible { outline:3px solid var(--brand); outline-offset:3px; border-radius:6px; }
  .sr { position:absolute; width:1px; height:1px; overflow:hidden; clip:rect(0 0 0 0); white-space:nowrap; }
  .wrap { width:100%; max-width:1240px; margin:0 auto; padding:0 16px; }
  @media (min-width:720px) { .wrap { padding:0 28px; } }

  /* infinite-canvas dot grid, drifts as you scroll */
  #grid { position:fixed; inset:0; z-index:-1; pointer-events:none; background-image:radial-gradient(var(--dot) 1.5px, transparent 1.8px); background-size:30px 30px; -webkit-mask-image:linear-gradient(#000, rgba(0,0,0,.35)); mask-image:linear-gradient(#000, rgba(0,0,0,.35)); }
  #bar { position:fixed; left:0; top:0; height:4px; width:100%; z-index:80; transform-origin:0 50%; transform:scaleX(0); background:linear-gradient(90deg,var(--brand),var(--coral),var(--sun)); }

  /* header */
  .top { position:sticky; top:0; z-index:60; background:var(--glass); -webkit-backdrop-filter:blur(14px) saturate(1.5); backdrop-filter:blur(14px) saturate(1.5); border-bottom:1.5px solid transparent; transition:border-color .25s; }
  .top.stuck { border-color:var(--line); }
  .top .wrap { display:flex; align-items:center; gap:14px; height:66px; }
  .brand { display:flex; align-items:center; gap:10px; text-decoration:none; font:800 21px/1 var(--display); letter-spacing:-.03em; }
  .brand svg { width:38px; height:38px; flex:none; border-radius:10px; transition:transform .4s cubic-bezier(.3,1.6,.5,1); }
  .brand:hover svg { transform:rotate(-8deg) scale(1.08); }
  .nav { margin-left:auto; display:none; gap:4px; }
  .nav a { text-decoration:none; font-weight:600; font-size:15px; padding:8px 12px; border-radius:10px; color:var(--dim); }
  .nav a:hover { color:var(--ink); background:color-mix(in srgb,var(--ink) 7%,transparent); }
  .top .btn { margin-left:auto; padding:9px 14px; font-size:14px; border-width:2px; box-shadow:2px 2px 0 var(--shade); }
  @media (min-width:860px) { .nav { display:flex; } .top .btn { margin-left:8px; } }

  /* buttons */
  .btn { display:inline-flex; align-items:center; justify-content:center; gap:10px; padding:12px 18px; border:2.5px solid var(--edge); border-radius:14px; background:var(--surface); color:var(--ink); font:700 16px/1.1 var(--display); letter-spacing:-.01em; text-decoration:none; box-shadow:4px 4px 0 var(--shade); transition:transform .15s, box-shadow .15s, background .15s; cursor:pointer; }
  .btn:hover { transform:translate(-2px,-2px); box-shadow:6px 6px 0 var(--shade); }
  .btn:active { transform:translate(3px,3px); box-shadow:1px 1px 0 var(--shade); }
  .btn svg { width:22px; height:22px; flex:none; }
  .btn.primary { background:var(--brand); color:var(--brand-ink); }
  .btn.sun { background:var(--sun); color:#16141f; }
  .btn.big { padding:17px 24px; font-size:18px; border-radius:16px; }
  .btn small { font:600 12px/1 var(--display); opacity:.8; }

  /* hero */
  .hero { position:relative; display:grid; gap:34px; padding-top:34px; padding-bottom:44px; }
  @media (min-width:1000px) { .hero { grid-template-columns:minmax(0,1fr) minmax(0,1.28fr); gap:48px; align-items:center; padding-top:52px; padding-bottom:70px; } }
  .eyebrow { display:inline-flex; align-items:center; gap:9px; margin:0 0 20px; padding:7px 14px 7px 10px; border:2px solid var(--edge); border-radius:99px; background:var(--surface); font:700 13px/1 var(--display); letter-spacing:.02em; }
  .eyebrow i { width:10px; height:10px; border-radius:50%; background:var(--teal); box-shadow:0 0 0 0 var(--teal); animation:ping 2s infinite; }
  @keyframes ping { 0% { box-shadow:0 0 0 0 color-mix(in srgb,var(--teal) 60%,transparent); } 70%,100% { box-shadow:0 0 0 10px transparent; } }
  h1 { margin:0 0 22px; font:800 clamp(52px,14vw,92px)/.92 var(--display); letter-spacing:-.055em; font-variation-settings:"opsz" 96; }
  @media (min-width:1000px) { h1 { font-size:clamp(54px,5.5vw,82px); } }
  h1 span { display:block; position:relative; }
  h1 .hl { display:inline-block; z-index:0; }
  h1 .hl::before { content:""; position:absolute; z-index:-1; left:-.08em; right:-.1em; top:.2em; bottom:-.02em; background:var(--hl); transform:skewX(-6deg) scaleX(0); transform-origin:0 50%; border-radius:.08em; animation:swipe .7s .55s cubic-bezier(.7,0,.2,1) forwards; }
  @keyframes swipe { to { transform:skewX(-6deg) scaleX(1); } }
  h1 em { font:italic 400 1.12em/.8 var(--serif); letter-spacing:-.02em; color:var(--brand); position:relative; display:inline-block; }
  h1 em svg { position:absolute; left:-2%; bottom:-.2em; width:104%; height:.34em; color:var(--coral); overflow:visible; }
  .lede { margin:0 0 28px; max-width:46ch; font-size:clamp(17px,2.2vw,20px); line-height:1.5; color:var(--dim); }
  .lede b { color:var(--ink); font-weight:650; }
  .cta { display:flex; flex-wrap:wrap; gap:16px; align-items:center; }
  .cta .btn { flex:1 1 230px; }
  @media (min-width:1000px) { .cta .btn { flex:0 1 auto; } }
  .fine { display:flex; flex-wrap:wrap; align-items:center; gap:8px 16px; margin:22px 0 0; font-size:14.5px; color:var(--dim); }
  .fine svg { width:17px; height:17px; flex:none; color:var(--brand); margin-right:-6px; }
  .fine a { font-weight:600; color:var(--ink); }

  /* the live demo */
  .hero-board { position:relative; min-width:0; }
  .frame { position:relative; border:2.5px solid var(--edge); border-radius:22px; background:var(--surface); box-shadow:8px 8px 0 var(--shade); overflow:hidden; }
  .fbar { display:flex; align-items:center; gap:10px; padding:11px 14px; border-bottom:2px solid var(--edge); }
  .fbar b { font:700 15px/1 var(--display); letter-spacing:-.01em; white-space:nowrap; overflow:hidden; text-overflow:ellipsis; }
  .chip { display:inline-flex; align-items:center; gap:6px; padding:4px 9px; border-radius:99px; background:color-mix(in srgb,var(--teal) 16%,transparent); color:var(--teal); font:700 11.5px/1 var(--display); white-space:nowrap; }
  .chip i { width:7px; height:7px; border-radius:50%; background:currentColor; animation:blink 1.6s infinite; }
  @keyframes blink { 50% { opacity:.25; } }
  .who { display:flex; margin-left:auto; }
  .who i { position:relative; display:grid; place-items:center; width:28px; height:28px; margin-left:-7px; border-radius:50%; border:2.5px solid var(--surface); background:var(--c); color:#fff; font:700 12px/1 var(--display); font-style:normal; transition:transform .3s; }
  .who i.busy { transform:translateY(-4px) scale(1.12); z-index:2; }
  .who i.busy::after { content:""; position:absolute; inset:-5px; border-radius:50%; border:2px solid var(--c); opacity:.5; }
  .who i.you { background:var(--ink); color:var(--bg); font-size:9.5px; letter-spacing:-.02em; }
  .boardwrap { position:relative; height:370px; background-color:var(--board); background-image:radial-gradient(var(--dot) 1.4px,transparent 1.7px); background-size:24px 24px; }
  @media (min-width:640px) { .boardwrap { height:auto; aspect-ratio:5/4; } }
  #board { position:absolute; inset:0; width:100%; height:100%; touch-action:none; cursor:crosshair; -webkit-user-select:none; user-select:none; -webkit-tap-highlight-color:transparent; }
  #board.sel { cursor:default; }
  .hint { position:absolute; left:0; right:0; bottom:16px; text-align:center; pointer-events:none; font:700 clamp(20px,5.4vw,26px)/1 var(--hand); color:var(--dim); transition:opacity .5s; }
  .hint.gone { opacity:0; }
  .tools { display:flex; flex-wrap:wrap; align-items:center; gap:6px 4px; padding:10px 12px; border-top:2px solid var(--edge); background:var(--surface); }
  .tools .sep { width:2px; height:24px; margin:0 6px; background:var(--line); border-radius:2px; }
  .tools button { display:inline-flex; align-items:center; justify-content:center; gap:6px; min-width:40px; height:40px; padding:0 10px; border:2px solid transparent; border-radius:11px; background:none; color:var(--ink); font:700 13px/1 var(--display); cursor:pointer; }
  .tools button:hover { background:color-mix(in srgb,var(--ink) 8%,transparent); }
  .tools button[aria-pressed="true"] { background:color-mix(in srgb,var(--brand) 18%,transparent); border-color:var(--brand); }
  .tools button svg { width:21px; height:21px; }
  .tools .sw { min-width:0; width:30px; padding:0; }
  .tools .sw i { width:20px; height:20px; border-radius:50%; background:var(--c); box-shadow:inset 0 0 0 2px color-mix(in srgb,var(--ink) 25%,transparent); }
  .tools .sw[aria-pressed="true"] i { box-shadow:0 0 0 3px var(--surface), 0 0 0 5px var(--c); }
  .tools .push { margin-left:auto; }
  .tools .snap b { font-weight:700; }
  .scrawl { display:none; position:absolute; right:6%; top:-58px; width:210px; font:700 25px/1 var(--hand); color:var(--coral); transform:rotate(3deg); pointer-events:none; }
  .scrawl svg { position:absolute; left:-46px; top:20px; width:62px; height:56px; overflow:visible; }
  @media (min-width:1000px) { .scrawl { display:block; } }
  .sticky { position:absolute; z-index:3; display:none; width:128px; padding:12px 12px 14px; background:#ffe27a; color:#3a2f00; font:700 21px/1.05 var(--hand); box-shadow:3px 5px 0 rgba(0,0,0,.18); transform:rotate(var(--r,0deg)); }
  .sticky small { display:block; margin-top:6px; font:600 12px/1 var(--display); opacity:.65; }
  .sticky.t { background:#9ee9d9; right:44px; bottom:-52px; --r:4deg; width:150px; }
  .sticky.y { left:26px; top:-62px; --r:-4deg; width:170px; }
  @media (min-width:1280px) { .sticky { display:block; } }

  /* marquee */
  .tape { position:relative; z-index:2; margin:6px 0 0; padding:26px 0; overflow:hidden; }
  .tape .band { display:flex; width:max-content; padding:14px 0; background:var(--ink); color:var(--bg); transform:rotate(-1.4deg); box-shadow:0 6px 0 var(--brand); animation:roll 38s linear infinite; }
  .tape .band div { display:flex; flex:none; align-items:center; }
  .tape span { padding:0 22px; font:800 26px/1 var(--display); letter-spacing:-.03em; white-space:nowrap; }
  .tape span::after { content:"\2726"; margin-left:44px; color:var(--sun); font-weight:400; }
  .tape span:nth-child(even) { font:italic 400 31px/1 var(--serif); letter-spacing:0; }
  @keyframes roll { to { transform:translateX(-50%) rotate(-1.4deg); } }

  /* sections */
  section.s { padding:84px 0 20px; }
  .kicker { display:inline-block; margin:0 0 14px; padding:5px 11px; border:2px solid var(--edge); border-radius:8px; font:700 12px/1 var(--mono); letter-spacing:.08em; text-transform:uppercase; background:var(--sun); color:#16141f; transform:rotate(-1.5deg); }
  h2 { margin:0 0 18px; max-width:18ch; font:800 clamp(36px,7.4vw,66px)/.98 var(--display); letter-spacing:-.045em; text-wrap:balance; }
  h2 em { font:italic 400 1.1em/.9 var(--serif); letter-spacing:-.02em; color:var(--brand); }
  .sub { margin:0 0 44px; max-width:56ch; font-size:clamp(17px,2.2vw,20px); color:var(--dim); }

  /* team boards: bento */
  .bento { display:grid; gap:18px; }
  @media (min-width:640px) { .bento { grid-template-columns:repeat(6,1fr); } .bento .b { grid-column:span 3; } .bento .b.w6 { grid-column:span 6; } }
  @media (min-width:1000px) { .bento .b.w4 { grid-column:span 4; } .bento .b.w2 { grid-column:span 2; } .bento .b.w6 { grid-column:span 6; } .bento .b.w3 { grid-column:span 3; } }
  .b { position:relative; display:flex; flex-direction:column; border:2.5px solid var(--edge); border-radius:24px; background:var(--surface); padding:22px 22px 24px; box-shadow:6px 6px 0 var(--shade); transition:transform .2s, box-shadow .2s; overflow:hidden; }
  .b:hover { transform:translate(-2px,-2px); box-shadow:8px 8px 0 var(--shade); }
  .b h3 { margin:0 0 8px; font:800 clamp(22px,3vw,28px)/1.05 var(--display); letter-spacing:-.035em; }
  .b p { margin:0; color:var(--dim); font-size:16px; line-height:1.5; }
  .b p b { color:var(--ink); font-weight:650; }
  .b .tag { align-self:flex-start; margin-bottom:14px; padding:4px 9px; border-radius:7px; background:var(--tc,var(--brand)); color:#fff; font:700 11px/1 var(--mono); letter-spacing:.08em; text-transform:uppercase; }
  .ill { width:100%; height:auto; margin:0 0 18px; flex:none; }
  .illbox { display:grid; place-items:center; min-height:190px; margin:0 0 18px; border-radius:16px; background:color-mix(in srgb,var(--ink) 4%,transparent); background-image:radial-gradient(var(--dot) 1.2px,transparent 1.5px); background-size:18px 18px; overflow:hidden; padding:14px; }
  .illbox .ill { margin:0; max-height:210px; }
  .b:not(.in) * { animation-play-state:paused !important; }
  .dev { color:var(--ink); }
  .flow { fill:none; stroke:var(--brand); stroke-width:3; stroke-linecap:round; stroke-dasharray:2 9; animation:flow 1.2s linear infinite; }
  @keyframes flow { to { stroke-dashoffset:-11; } }
  .node { fill:var(--teal); transform-box:fill-box; transform-origin:center; animation:pulse 2.4s ease-out infinite; }
  @keyframes pulse { 0% { transform:scale(.6); opacity:1; } 70%,100% { transform:scale(2.4); opacity:0; } }
  .svgtxt { font:700 12px var(--display); fill:#fff; }
  .hand { font-family:var(--hand); font-weight:700; fill:currentColor; }
  .cur1 { animation:drift1 6s ease-in-out infinite; } .cur2 { animation:drift2 7s ease-in-out infinite; }
  @keyframes drift1 { 50% { transform:translate(-22px,12px); } } @keyframes drift2 { 50% { transform:translate(16px,-14px); } }
  .glow { animation:glow 3.2s ease-out infinite; }
  @keyframes glow { 0%,6% { filter:drop-shadow(0 0 7px var(--coral)); stroke-width:6; } 40%,100% { filter:none; stroke-width:3.4; } }

  .share { display:grid; gap:9px; width:100%; max-width:340px; font-size:14px; }
  .share .row { display:flex; align-items:center; gap:10px; padding:8px 10px; border:2px solid var(--edge); border-radius:12px; background:var(--surface); }
  .share .av { display:grid; place-items:center; flex:none; width:28px; height:28px; border-radius:50%; background:var(--c); color:#fff; font:700 12px/1 var(--display); }
  .share .nm { font-weight:650; }
  .share .perm { margin-left:auto; padding:3px 9px; border-radius:99px; background:color-mix(in srgb,var(--ink) 9%,transparent); font:700 12px/1.3 var(--display); }
  .share .perm.e { background:color-mix(in srgb,var(--brand) 20%,transparent); color:var(--brand); }
  .tog { position:relative; flex:none; width:40px; height:24px; margin-left:auto; border-radius:99px; background:var(--line); animation:togbg 6s infinite; }
  .tog::after { content:""; position:absolute; left:3px; top:3px; width:18px; height:18px; border-radius:50%; background:#fff; box-shadow:0 1px 3px rgba(0,0,0,.3); animation:togk 6s infinite; }
  @keyframes togbg { 0%,30% { background:var(--line); } 40%,90% { background:var(--teal); } 100% { background:var(--line); } }
  @keyframes togk { 0%,30% { transform:none; } 40%,90% { transform:translateX(16px); } 100% { transform:none; } }
  .share .org .perm { display:none; }
  .off { position:relative; width:100%; max-width:300px; height:120px; }
  .off .p { position:absolute; left:0; right:0; display:flex; align-items:center; gap:10px; padding:12px 14px; border:2px solid var(--edge); border-radius:14px; background:var(--surface); font:700 15px/1.2 var(--display); }
  .off .p svg { width:26px; height:26px; flex:none; }
  .off .p small { display:block; font:500 13px/1.2 var(--display); color:var(--dim); }
  .off .a { top:6px; animation:offa 7s infinite; color:var(--coral); }
  .off .c { top:62px; animation:offb 7s infinite; color:var(--teal); }
  @keyframes offa { 0%,5% { opacity:0; transform:translateY(8px); } 12%,50% { opacity:1; transform:none; } 58%,100% { opacity:0; transform:translateY(-8px); } }
  @keyframes offb { 0%,55% { opacity:0; transform:translateY(8px); } 64%,92% { opacity:1; transform:none; } 100% { opacity:0; transform:translateY(-8px); } }
  .rough { stroke-dasharray:1; animation:rough 5s infinite; fill:none; stroke:var(--coral); stroke-width:4; stroke-linecap:round; stroke-linejoin:round; }
  .clean { fill:none; stroke:var(--brand); stroke-width:4; stroke-linejoin:round; animation:clean 5s infinite; }
  .snapd { animation:snapd 5s infinite; }
  @keyframes rough { 0% { stroke-dashoffset:1; opacity:1; } 36% { stroke-dashoffset:0; opacity:1; } 46% { opacity:1; } 52%,100% { stroke-dashoffset:0; opacity:0; } }
  @keyframes clean { 0%,46% { opacity:0; transform:scale(.94); } 56%,90% { opacity:1; transform:none; } 100% { opacity:0; } }
  @keyframes snapd { 0%,50% { opacity:0; transform:translateY(6px); } 58%,88% { opacity:1; transform:none; } 100% { opacity:0; } }
  .clean, .snapd { transform-box:fill-box; transform-origin:center; }
  .hw { display:flex; flex-wrap:wrap; align-items:center; justify-content:center; gap:10px 18px; }
  .hw .ink { font:700 38px/1.1 "Kalam","Caveat",cursive; color:var(--brand); transform:rotate(-3deg); }
  .hw .arr { width:44px; color:var(--dim); }
  .hw .typed { padding:8px 14px; border:2px solid var(--edge); border-radius:12px; background:var(--surface); font:600 26px/1.1 system-ui,"Noto Sans Devanagari","Nirmala UI",sans-serif; }
  .chips { display:flex; flex-wrap:wrap; gap:6px; margin-top:12px; justify-content:center; width:100%; }
  .chips i { font:700 12px/1 var(--display); font-style:normal; padding:5px 9px; border-radius:99px; border:1.5px solid var(--line); background:var(--surface); }
  .palm { width:100%; max-width:300px; }
  .pen { fill:none; stroke:var(--brand); stroke-width:4; stroke-linecap:round; stroke-dasharray:1; animation:penw 5s infinite; }
  @keyframes penw { 0% { stroke-dashoffset:1; } 60%,92% { stroke-dashoffset:0; } 100% { stroke-dashoffset:0; opacity:0; } }
  .palmx { animation:palmx 5s infinite; transform-box:fill-box; transform-origin:center; }
  @keyframes palmx { 0%,25% { opacity:0; } 30%,92% { opacity:1; } 100% { opacity:0; } }

  /* toolbox */
  .toolgrid { display:grid; gap:14px; grid-template-columns:repeat(auto-fill,minmax(min(100%,200px),1fr)); }
  .tool { display:flex; flex-direction:column; gap:10px; padding:18px; border:2px solid var(--edge); border-radius:18px; background:var(--surface); transition:transform .25s cubic-bezier(.3,1.7,.5,1), background .2s; }
  .tool:hover { transform:translateY(-5px) rotate(var(--tilt,-1.5deg)); background:color-mix(in srgb,var(--c) 12%,var(--surface)); }
  .tool:nth-child(even) { --tilt:1.5deg; }
  .tool svg { width:44px; height:44px; color:var(--c); }
  .tool h3 { margin:0; font:800 19px/1.1 var(--display); letter-spacing:-.025em; }
  .tool p { margin:0; font-size:14.5px; line-height:1.4; color:var(--dim); }

  /* devices & downloads */
  .plats { display:grid; gap:18px; grid-template-columns:repeat(auto-fill,minmax(min(100%,212px),1fr)); }
  .plat { position:relative; display:flex; flex-direction:column; gap:12px; padding:22px; border:2.5px solid var(--edge); border-radius:24px; background:var(--surface); box-shadow:6px 6px 0 var(--shade); transition:transform .2s, box-shadow .2s; }
  .plat:hover { transform:translate(-2px,-2px); box-shadow:8px 8px 0 var(--shade); }
  .plat .ic { display:grid; place-items:center; width:68px; height:68px; border-radius:18px; background:color-mix(in srgb,var(--c) 15%,transparent); color:var(--c); transition:transform .4s cubic-bezier(.3,1.7,.5,1); }
  .plat:hover .ic { transform:rotate(-7deg) scale(1.08); }
  .plat .ic svg { width:40px; height:40px; }
  .plat h3 { margin:6px 0 0; font:800 26px/1 var(--display); letter-spacing:-.035em; }
  .plat p { margin:0; color:var(--dim); font-size:15.5px; line-height:1.45; flex:1; }
  .plat .btn { width:100%; }
  .plat .badge { display:none; position:absolute; right:16px; top:16px; padding:5px 10px; border-radius:99px; background:var(--ink); color:var(--bg); font:700 11.5px/1 var(--display); transform:rotate(3deg); }
  .plat.you .badge { display:block; }
  .plat .badge.soon { display:block; background:var(--sun); color:#16141f; }
  .notes { display:grid; gap:16px; margin-top:44px; }
  @media (min-width:860px) { .notes { grid-template-columns:repeat(3,1fr); } }
  .note { padding:20px 20px 22px; border:2px dashed var(--edge); border-radius:18px; font-size:15.5px; color:var(--dim); line-height:1.5; }
  .note h4 { display:flex; align-items:center; gap:9px; margin:0 0 8px; color:var(--ink); font:800 18px/1.1 var(--display); letter-spacing:-.02em; }
  .note h4 svg { width:24px; height:24px; color:var(--brand); }
  .note b { color:var(--ink); font-weight:700; }
  .rel { margin:22px 0 0; font-size:15px; color:var(--dim); } .rel a { font-weight:650; color:var(--ink); }

  /* steps */
  .steps { position:relative; margin-top:40px; display:grid; gap:34px; counter-reset:s; list-style:none; padding:0; margin:0; }
  @media (min-width:860px) { .steps { grid-template-columns:repeat(4,1fr); gap:26px; } }
  .step { position:relative; counter-increment:s; padding-top:8px; }
  .step::before { content:counter(s); display:grid; place-items:center; width:64px; height:64px; margin-bottom:14px; border:2.5px solid var(--edge); border-radius:50%; background:var(--sun); color:#16141f; font:700 42px/1 var(--hand); box-shadow:3px 3px 0 var(--shade); transform:rotate(var(--r,-4deg)); }
  .step:nth-child(even)::before { --r:5deg; background:#ffa184; }
  .step h3 { margin:0 0 6px; font:800 24px/1.05 var(--display); letter-spacing:-.035em; }
  .step p { margin:0; color:var(--dim); font-size:16px; line-height:1.5; }
  @media (min-width:860px) { .step:not(:last-child)::after { content:""; position:absolute; top:34px; left:84px; right:-18px; height:0; border-top:3px dashed var(--line); } }

  /* finale */
  .finale { position:relative; margin:110px 0 0; padding:80px 0 90px; background:var(--ink); color:var(--bg); overflow:hidden; text-align:center; }
  .finale::before { content:""; position:absolute; inset:0; background-image:radial-gradient(color-mix(in srgb,var(--bg) 22%,transparent) 1.5px,transparent 1.8px); background-size:30px 30px; }
  .finale .wrap { position:relative; }
  .finale h2 { max-width:none; margin:0 auto 18px; color:var(--bg); font-size:clamp(46px,11vw,128px); line-height:.9; }
  .finale h2 em { color:var(--sun); }
  .finale p { margin:0 auto 34px; max-width:46ch; font-size:19px; opacity:.78; }
  .finale .cta { justify-content:center; }
  .finale .btn { --shade:var(--sun); border-color:var(--bg); }
  .finale .btn.primary { background:var(--brand); color:#fff; }
  .finale .btn:not(.primary) { background:var(--bg); color:var(--ink); }
  .doodle { position:absolute; opacity:.9; pointer-events:none; }
  footer { padding:30px 0 40px; background:var(--ink); color:var(--bg); border-top:1.5px solid color-mix(in srgb,var(--bg) 16%,transparent); font-size:14.5px; }
  footer .wrap { display:flex; flex-wrap:wrap; align-items:center; gap:12px 24px; }
  footer .brand { font-size:18px; } footer .brand svg { width:30px; height:30px; }
  footer a { text-decoration:none; opacity:.75; } footer a:hover { opacity:1; text-decoration:underline; }
  footer .sp { margin-left:auto; opacity:.6; }

  /* scroll draw-ons */
  .draw path, .draw line, .draw polyline { stroke-dasharray:1; stroke-dashoffset:1; opacity:0; transition:stroke-dashoffset 1s cubic-bezier(.6,0,.2,1) .35s, opacity .1s .35s; }
  .in .draw path, .draw.in path, .draw.in polyline { stroke-dashoffset:0; opacity:1; }
  .draw { fill:none; stroke:currentColor; stroke-width:5; stroke-linecap:round; stroke-linejoin:round; }
  .js .reveal { opacity:0; }
  .js .reveal.in { opacity:1; animation:rise .8s cubic-bezier(.2,.7,.2,1) var(--d,0s) backwards; }
  @keyframes rise { from { opacity:0; transform:translateY(30px); } }

  @media (prefers-reduced-motion: reduce) {
    html { scroll-behavior:auto; }
    *, *::before, *::after { animation:none !important; transition:none !important; }
    .js .reveal { opacity:1; }
    h1 .hl::before { transform:skewX(-6deg) scaleX(1); }
    .draw path, .draw polyline { stroke-dashoffset:0; opacity:1; }
    .tape { padding:10px 0; } .tape .band { transform:none; width:auto; flex-wrap:wrap; justify-content:center; } .tape .band div { flex-wrap:wrap; justify-content:center; } .tape .band div[aria-hidden] { display:none; }
    .rough, .pen { stroke-dashoffset:0; } .clean, .snapd, .palmx { opacity:1; } .rough { opacity:.35; }
    .off .a { opacity:0; } .off .c { opacity:1; }
    .tog { background:var(--teal); } .tog::after { transform:translateX(16px); }
  }
</style>
</head>
<body>
<svg width="0" height="0" style="position:absolute" aria-hidden="true" focusable="false">
  <defs>
    <symbol id="i-mac" viewBox="0 0 48 48"><g fill="none" stroke="currentColor" stroke-width="2.8" stroke-linecap="round" stroke-linejoin="round"><rect x="5" y="7" width="38" height="26" rx="4"/><path d="M5 27h38M17 41h14M24 33v8"/><circle cx="24" cy="17" r="3.2" fill="currentColor" stroke="none"/></g></symbol>
    <symbol id="i-win" viewBox="0 0 48 48"><path fill="currentColor" d="M5 11.5 21 9v14H5zM24 8.6 43 5.5V23H24zM5 25h16v14.2L5 36.6zM24 25h19v17.8l-19-3.2z"/></symbol>
    <symbol id="i-and" viewBox="0 0 48 48"><g fill="none" stroke="currentColor" stroke-width="2.8" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="8" width="31" height="23" rx="3.5"/><path d="M8 36h21"/><rect x="29" y="17" width="16" height="27" rx="3.5" fill="var(--surface)"/><path d="M35.5 40h3"/></g></symbol>
    <symbol id="i-lin" viewBox="0 0 48 48"><g fill="none" stroke="currentColor" stroke-width="2.8" stroke-linecap="round" stroke-linejoin="round"><rect x="4" y="8" width="40" height="32" rx="5"/><path d="M12 19l7 5.5-7 5.5M24 31h12"/></g></symbol>
    <symbol id="i-web" viewBox="0 0 48 48"><g fill="none" stroke="currentColor" stroke-width="2.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="24" cy="24" r="18.5"/><path d="M5.5 24h37M24 5.5c-9 9-9 28 0 37M24 5.5c9 9 9 28 0 37"/></g></symbol>
    <symbol id="i-lock" viewBox="0 0 24 24"><g fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><rect x="4.5" y="10.5" width="15" height="10" rx="2.5"/><path d="M8 10.5V8a4 4 0 0 1 8 0v2.5"/></g></symbol>
    <symbol id="i-down" viewBox="0 0 24 24"><g fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3.5v12M7 11l5 5 5-5M4.5 20.5h15"/></g></symbol>
    <symbol id="i-arrow" viewBox="0 0 24 24"><g fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M4 12h15M13 6l6 6-6 6"/></g></symbol>
    <symbol id="mini" viewBox="0 0 100 64"><g fill="none" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><rect x="7" y="9" width="40" height="26" rx="3" stroke="#e5562f"/><circle cx="76" cy="23" r="14" stroke="#12a594"/><path d="M12 50q8-11 16 0t16 0t16 0t16 0" stroke="#7a62ea"/><path d="M49 22h10M55 17l5 5-5 5" stroke="#3e63dd"/></g></symbol>
  </defs>
</svg>
<div id="grid" aria-hidden="true"></div>
<div id="bar" aria-hidden="true"></div>

<header class="top" id="top">
  <div class="wrap">
    <a class="brand" href="#" aria-label="Inkboard home">${LOGO}<span>Inkboard</span></a>
    <nav class="nav" aria-label="Sections">
      <a href="#together">Team boards</a>
      <a href="#tools">Tools</a>
      <a href="#download">Devices</a>
      <a href="#start">Get started</a>
    </nav>
    <a class="btn primary" href="/app">Open in your browser</a>
  </div>
</header>

<main>
<section class="hero wrap" id="top-hero">
  <div class="hero-copy">
    <p class="eyebrow"><i></i> By Kenpath Labs</p>
    <h1><span>One board.</span><span><b class="hl" style="font-weight:inherit">No edges.</b></span><span><em>Everyone<svg class="draw" viewBox="0 0 200 20" preserveAspectRatio="none" aria-hidden="true"><path pathLength="1" d="M3 13C40 4 80 17 120 8s60 2 77 0"/></svg></em> on it.</span></h1>
    <p class="lede">Inkboard is an infinite whiteboard for teams. <b>Sketch on the big panel in the meeting room</b>, keep going on your tablet, phone or laptop. Everyone's on the same board, live, with a name on every cursor and on every change.</p>
    <div class="cta">
      <a class="btn big primary" id="openweb" href="/app"><svg aria-hidden="true"><use href="#i-web"/></svg>Open in your browser</a>
      <a class="btn big" id="primary" data-auto href="/download/mac"><svg aria-hidden="true"><use href="#i-down"/></svg><span class="lbl">Download for macOS</span></a>
    </div>
    <p class="fine"><svg aria-hidden="true"><use href="#i-lock"/></svg><span>Sign in with your Kenpath Google account.</span> <a href="#download">All platforms &darr;</a></p>
  </div>

  <div class="hero-board">
    <div class="scrawl" aria-hidden="true">go on, draw something!<svg viewBox="0 0 62 56"><path d="M6 6C4 32 24 46 54 44M44 34l11 10-12 8" fill="none" stroke="currentColor" stroke-width="3.5" stroke-linecap="round" stroke-linejoin="round"/></svg></div>
    <div class="sticky y" aria-hidden="true">demo on the panel, Fri<small>Dev</small></div>
    <div class="sticky t" aria-hidden="true">love the snap &#10022;<small>Ana</small></div>
    <div class="frame">
      <div class="fbar">
        <b>Sprint kickoff</b><span class="chip"><i></i>Live</span>
        <div class="who" aria-hidden="true">
          <i data-who="ana" style="--c:#e5562f">A</i><i data-who="ben" style="--c:#0b8a7a">B</i><i data-who="chi" style="--c:#b04fd6">C</i><i data-who="dev" style="--c:#3e63dd">D</i><i class="you">You</i>
        </div>
      </div>
      <div class="boardwrap">
        <canvas id="board" role="img" aria-label="A live demo whiteboard. Draw on it with a mouse, finger or pen while four simulated teammates draw alongside you."></canvas>
        <div class="hint" id="hint" aria-hidden="true">draw a rough box, a circle or a line</div>
      </div>
      <div class="tools" id="tools" role="toolbar" aria-label="Demo tools">
        <button type="button" data-tool="pen" aria-pressed="true" aria-label="Pen" title="Pen"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 20l1-4.5L16.5 4a2.1 2.1 0 0 1 3 3L8 18.5z M14.5 6l3.5 3.5"/></svg></button>
        <button type="button" data-tool="select" aria-pressed="false" aria-label="Select: tap a shape to see who made it" title="Select: tap a shape to see who made it"><svg viewBox="0 0 24 24" fill="currentColor" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"><path d="M5 3l14 8-6 1.8L10.5 19z"/></svg></button>
        <span class="sep"></span>
        <button type="button" class="sw" data-c="ink" aria-pressed="true" aria-label="Ink" title="Ink"><i style="--c:var(--ink)"></i></button>
        <button type="button" class="sw" data-c="#e5562f" aria-pressed="false" aria-label="Coral" title="Coral"><i style="--c:#e5562f"></i></button>
        <button type="button" class="sw" data-c="#7a62ea" aria-pressed="false" aria-label="Violet" title="Violet"><i style="--c:#7a62ea"></i></button>
        <button type="button" class="sw" data-c="#0b8a7a" aria-pressed="false" aria-label="Teal" title="Teal"><i style="--c:#0b8a7a"></i></button>
        <button type="button" class="sw" data-c="#d99a00" aria-pressed="false" aria-label="Amber" title="Amber"><i style="--c:#d99a00"></i></button>
        <span class="sep"></span>
        <button type="button" class="snap" id="snap" aria-pressed="true" title="Shape recognition: rough boxes, circles and lines snap clean"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3v3M12 18v3M3 12h3M18 12h3M5.6 5.6l2 2M16.4 16.4l2 2M5.6 18.4l2-2M16.4 7.6l2-2"/></svg><b>Snap</b></button>
        <button type="button" class="push" id="undo" aria-label="Undo my last stroke" title="Undo: only undoes your own strokes"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M9 14L4 9l5-5M4 9h10a6 6 0 0 1 0 12h-3"/></svg></button>
        <button type="button" id="clear" aria-label="Clear my strokes" title="Clear my strokes (teammates' stay)"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13"/></svg></button>
      </div>
    </div>
  </div>
</section>

<div class="tape" aria-label="Tools: pen, shape recognition, handwriting to text, tables, mind maps, maths, image and PDF import, pages, export">
  <div class="band">
    <div><span>Pen</span><span>Shape recognition</span><span>Handwriting to text</span><span>Tables</span><span>Mind maps</span><span>Maths</span><span>Image &amp; PDF import</span><span>Pages</span><span>Export</span></div>
    <div aria-hidden="true"><span>Pen</span><span>Shape recognition</span><span>Handwriting to text</span><span>Tables</span><span>Mind maps</span><span>Maths</span><span>Image &amp; PDF import</span><span>Pages</span><span>Export</span></div>
  </div>
</div>

<section class="s wrap" id="together">
  <span class="kicker reveal">Team boards</span>
  <h2 class="reveal">Built for <em>the whole room,</em> not whoever holds the marker.</h2>
  <p class="sub reveal">Everyone keeps their own copy of the same board. Your view, your tools, your selection, and everybody's edits flowing in as they happen.</p>

  <div class="bento">
    <article class="b w4 reveal">
      <span class="tag" style="--tc:#6750d8">Live sync</span>
      <svg class="ill" viewBox="0 0 540 240" role="img" aria-label="A wall panel, a laptop and a phone all showing the same board, joined by a live sync line">
        <g class="dev" stroke="currentColor" stroke-width="3.2" fill="var(--surface)" stroke-linejoin="round" stroke-linecap="round">
          <rect x="14" y="22" width="224" height="140" rx="12"/><path d="M126 162v26M94 190h64" fill="none"/>
          <rect x="274" y="60" width="132" height="90" rx="8"/><path d="M256 154h168l-9 11a8 8 0 0 1-6 3H271a8 8 0 0 1-6-3z"/>
          <rect x="448" y="50" width="64" height="124" rx="13"/><path d="M472 164h16" fill="none"/>
        </g>
        <use href="#mini" x="24" y="30" width="204" height="130"/>
        <use href="#mini" x="280" y="66" width="120" height="78"/>
        <g fill="none" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><rect x="456" y="72" width="48" height="30" rx="3" stroke="#e5562f"/><circle cx="480" cy="128" r="13" stroke="#12a594"/></g>
        <path class="flow" d="M126 200C160 238 270 240 340 174"/><path class="flow" d="M352 174C410 238 450 236 480 184" style="animation-delay:-.5s"/>
        <circle class="node" cx="126" cy="196" r="6"/><circle class="node" cx="344" cy="172" r="6" style="animation-delay:-.8s"/><circle class="node" cx="480" cy="184" r="6" style="animation-delay:-1.6s"/>
      </svg>
      <h3>Every device holds the whole board</h3>
      <p>Wall panel, laptop, phone. Each keeps its <b>own copy of the same board</b>, and edits sync live between them. Draw on the panel, watch it land on your laptop.</p>
    </article>

    <article class="b w2 reveal" style="--d:.08s">
      <span class="tag" style="--tc:#e5562f">Who did what</span>
      <div class="illbox">
        <svg class="ill" viewBox="0 0 360 210" role="img" aria-label="A selected shape labelled Added by Ana, edited by Ben 2 min ago, with Ben and Chitra's named cursors nearby">
          <rect class="glow" x="46" y="82" width="150" height="86" rx="8" fill="none" stroke="#e5562f" stroke-width="3.4"/>
          <rect x="38" y="74" width="166" height="102" fill="none" stroke="#6750d8" stroke-width="1.6" stroke-dasharray="5 4"/>
          <g fill="var(--surface)" stroke="#6750d8" stroke-width="1.6"><rect x="34" y="70" width="8" height="8"/><rect x="200" y="70" width="8" height="8"/><rect x="34" y="172" width="8" height="8"/><rect x="200" y="172" width="8" height="8"/></g>
          <g><rect x="30" y="28" width="300" height="30" rx="15" fill="var(--ink)"/><text x="44" y="48" font-family="Bricolage Grotesque,system-ui,sans-serif" font-weight="700" font-size="13" fill="var(--bg)">Added by Ana · edited by Ben 2 min ago</text></g>
          <g class="cur1"><path d="M232 122l0 17 4.5-4 3.5 8 3-1.4-3.4-7.4 6.4-.4z" fill="#0b8a7a" stroke="#fff" stroke-width="1.5" stroke-linejoin="round"/><rect x="244" y="138" width="38" height="20" rx="10" fill="#0b8a7a"/><text x="252" y="152" class="svgtxt">Ben</text></g>
          <g class="cur2"><path d="M262 168l0 17 4.5-4 3.5 8 3-1.4-3.4-7.4 6.4-.4z" fill="#b04fd6" stroke="#fff" stroke-width="1.5" stroke-linejoin="round"/><rect x="274" y="184" width="54" height="20" rx="10" fill="#b04fd6"/><text x="282" y="198" class="svgtxt">Chitra</text></g>
        </svg>
      </div>
      <h3>See who did what</h3>
      <p>Named cursors and selections while you work, fresh edits flash, and any shape tells you who added it and who touched it last. <b>Undo only undoes your own changes.</b></p>
    </article>

    <article class="b w2 reveal">
      <span class="tag" style="--tc:#0b8a7a">Sharing</span>
      <div class="illbox">
        <div class="share" aria-hidden="true">
          <div class="row"><span class="av" style="--c:#e5562f">A</span><span class="nm">Ana</span><span class="perm e">can edit</span></div>
          <div class="row"><span class="av" style="--c:#3e63dd">B</span><span class="nm">Ben</span><span class="perm">can view</span></div>
          <div class="row org"><span class="av" style="--c:var(--ink);color:var(--bg)">&#9679;</span><span class="nm">Everyone at Kenpath</span><span class="tog"></span></div>
        </div>
      </div>
      <h3>Share it your way</h3>
      <p>Invite colleagues as <b>can edit</b> or <b>can view</b>, or open a board to the whole organisation. Sign-in is Google Workspace only, for Kenpath's domains.</p>
    </article>

    <article class="b w2 reveal" style="--d:.08s">
      <span class="tag" style="--tc:#e5562f">Offline</span>
      <div class="illbox">
        <div class="off" aria-hidden="true">
          <div class="p a"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M2 8.8a15 15 0 0 1 4-2.4M22 8.8A15 15 0 0 0 10.5 5M5 12.5a10 10 0 0 1 3-1.8M19 12.5a10 10 0 0 0-3-1.8M8.5 16a5 5 0 0 1 7 0M12 20h.01M3 3l18 18"/></svg><span>Offline<small>3 changes waiting</small></span></div>
          <div class="p c"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="9"/><path d="M7.5 12.5l3 3 6-6.5"/></svg><span>Back online<small>Caught up, all synced</small></span></div>
        </div>
      </div>
      <h3>Works offline</h3>
      <p>The board lives on your device. Keep drawing in the lift or on the train, and <b>changes catch up on reconnect.</b></p>
    </article>

    <article class="b w2 reveal" style="--d:.16s">
      <span class="tag" style="--tc:#6750d8">Shape recognition</span>
      <div class="illbox">
        <svg class="ill" viewBox="0 0 300 170" role="img" aria-label="A rough hand-drawn box snapping into a clean rectangle">
          <path class="rough" pathLength="1" d="M52 40C90 34 150 42 246 36C250 70 244 100 250 134C190 140 120 128 48 138C54 104 46 72 56 38"/>
          <rect class="clean" x="50" y="38" width="198" height="98" rx="3"/>
          <g class="snapd"><rect x="92" y="72" width="116" height="30" rx="15" fill="#6750d8"/><text x="107" y="92" class="svgtxt">Snapped &#10003;</text></g>
        </svg>
      </div>
      <h3>Draw rough, get clean</h3>
      <p>Sketch a wobbly box or circle and it <b>snaps into a perfect shape.</b> Whiteboard speed, slide-deck polish.</p>
    </article>

    <article class="b w3 reveal">
      <span class="tag" style="--tc:#3e63dd">Handwriting to text</span>
      <div class="illbox">
        <div class="hw" aria-hidden="true">
          <span class="ink">नमस्ते team</span>
          <svg class="arr" viewBox="0 0 44 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><path d="M3 12h36M30 4l9 8-9 8"/></svg>
          <span class="typed">नमस्ते team</span>
          <div class="chips"><i>English</i><i>हिन्दी</i><i>On Android</i></div>
        </div>
      </div>
      <h3>Write it, it types it</h3>
      <p>Handwriting turns into editable text, in <b>English and Hindi</b>, on Android. Scribble on the panel, tidy up later.</p>
    </article>

    <article class="b w3 reveal" style="--d:.08s">
      <span class="tag" style="--tc:#b04fd6">Pen &amp; palm detection</span>
      <div class="illbox">
        <svg class="ill palm" viewBox="0 0 300 170" role="img" aria-label="A resting palm being ignored while the pen keeps drawing">
          <g class="palmx"><ellipse cx="86" cy="116" rx="52" ry="38" fill="var(--ink)" opacity=".1" stroke="var(--ink)" stroke-width="2.4" stroke-dasharray="6 5"/><text x="48" y="121" class="hand" font-size="22" style="fill:var(--dim)">palm: ignored</text></g>
          <path class="pen" pathLength="1" d="M128 100C150 40 172 40 186 84S218 120 254 52"/>
          <circle cx="254" cy="52" r="6" fill="var(--brand)"/>
        </svg>
      </div>
      <h3>Made for the big touch panel</h3>
      <p>On touch panels, tablets and phones, <b>pen and palm detection</b> means you can rest your hand and just write.</p>
    </article>
  </div>
</section>

<section class="s wrap" id="tools">
  <span class="kicker reveal">Tools</span>
  <h2 class="reveal">Everything you <em>reach for</em> at a whiteboard.</h2>
  <p class="sub reveal">And a few things a whiteboard can't do.</p>
  <div class="toolgrid">
    <div class="tool reveal" style="--c:#6750d8"><svg viewBox="0 0 48 48" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M8 40l2-9L33 8a4.2 4.2 0 0 1 6 6L16 37z M29 12l7 7"/></svg><h3>Pen</h3><p>Smooth ink with pen and palm detection on touch screens.</p></div>
    <div class="tool reveal" style="--c:#e5562f;--d:.05s"><svg viewBox="0 0 48 48" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="5" y="5" width="22" height="22" rx="3"/><circle cx="31" cy="31" r="12"/></svg><h3>Shapes</h3><p>Draw it rough, and shape recognition snaps it clean.</p></div>
    <div class="tool reveal" style="--c:#3e63dd;--d:.1s"><svg viewBox="0 0 48 48" aria-hidden="true"><text x="3" y="33" font-family="Bricolage Grotesque,system-ui,sans-serif" font-weight="800" font-size="30" fill="currentColor">A</text><text x="23" y="38" font-family="'Noto Sans Devanagari','Nirmala UI',sans-serif" font-weight="700" font-size="25" fill="currentColor">अ</text></svg><h3>Handwriting to text</h3><p>English and Hindi, on Android.</p></div>
    <div class="tool reveal" style="--c:#0b8a7a;--d:.15s"><svg viewBox="0 0 48 48" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="5" y="8" width="38" height="32" rx="4"/><path d="M5 18h38M5 29h38M18 18v22M31 18v22"/></svg><h3>Tables</h3><p>Rows and columns, right on the canvas.</p></div>
    <div class="tool reveal" style="--c:#b04fd6;--d:.2s"><svg viewBox="0 0 48 48" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="24" cy="24" r="7"/><circle cx="7" cy="10" r="3.5"/><circle cx="41" cy="9" r="3.5"/><circle cx="8" cy="39" r="3.5"/><circle cx="41" cy="38" r="3.5"/><path d="M17.5 20.5L10 12M30 19.5l8-8M17.5 28L11 36M30 28l8 8"/></svg><h3>Mind maps</h3><p>Branch an idea outward and keep going.</p></div>
    <div class="tool reveal" style="--c:#d99a00;--d:.25s"><svg viewBox="0 0 48 48" aria-hidden="true"><text x="24" y="37" text-anchor="middle" font-family="'Instrument Serif',Georgia,serif" font-style="italic" font-size="42" fill="currentColor">&#8721;x&#178;</text></svg><h3>Maths</h3><p>Write equations the way you'd say them.</p></div>
    <div class="tool reveal" style="--c:#e5562f;--d:.3s"><svg viewBox="0 0 48 48" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="5" y="7" width="38" height="30" rx="4"/><circle cx="16" cy="18" r="3.4"/><path d="M5 31l10-8 8 6 8-9 12 10"/><rect x="26" y="33" width="19" height="12" rx="3" fill="var(--surface)"/><path d="M30 39h2.5a1.6 1.6 0 0 0 0-3.2H30zM30 39v3" stroke-width="2"/></svg><h3>Image &amp; PDF import</h3><p>Drop in a photo or a document and mark it up.</p></div>
    <div class="tool reveal" style="--c:#3e63dd;--d:.35s"><svg viewBox="0 0 48 48" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="14" y="4" width="28" height="34" rx="4"/><path d="M8 12v27a5 5 0 0 0 5 5h22M21 15h14M21 22h14M21 29h8"/></svg><h3>Pages</h3><p>More than one canvas per board.</p></div>
    <div class="tool reveal" style="--c:#0b8a7a;--d:.4s"><svg viewBox="0 0 48 48" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M24 31V6M14 15l10-10 10 10M7 28v13h34V28"/></svg><h3>Export</h3><p>Take the board with you when the meeting ends.</p></div>
  </div>
</section>

<section class="s wrap" id="download">
  <span class="kicker reveal">Devices</span>
  <h2 class="reveal">Wherever the team is <em>standing.</em></h2>
  <p class="sub reveal">Big touch panels in the meeting room. Android tablets and phones with pen and palm detection. Native apps on macOS, Windows and Linux. And your browser.</p>

  <div class="plats">
    <div class="plat reveal" data-p="mac" style="--c:#6750d8"><span class="badge">Your device</span><div class="ic"><svg aria-hidden="true"><use href="#i-mac"/></svg></div><h3>macOS</h3><p>Apple Silicon and Intel, macOS 11 or later.</p><a class="btn primary" href="/download/mac"><svg aria-hidden="true"><use href="#i-down"/></svg>Download .dmg</a></div>
    <div class="plat reveal" data-p="windows" style="--c:#3e63dd;--d:.06s"><span class="badge">Your device</span><div class="ic"><svg aria-hidden="true"><use href="#i-win"/></svg></div><h3>Windows</h3><p>Windows 10 or 11, 64-bit.</p><a class="btn" href="/download/windows"><svg aria-hidden="true"><use href="#i-down"/></svg>Download .exe</a></div>
    <div class="plat reveal" data-p="android" style="--c:#0b8a7a;--d:.12s"><span class="badge">Your device</span><div class="ic"><svg aria-hidden="true"><use href="#i-and"/></svg></div><h3>Android</h3><p>Meeting-room panels, tablets and phones, Android 7 or later.</p><a class="btn" href="/download/android"><svg aria-hidden="true"><use href="#i-down"/></svg>Download .apk</a></div>
    <div class="plat reveal" data-p="linux" style="--c:#e5562f;--d:.18s"><span class="badge">Your device</span><div class="ic"><svg aria-hidden="true"><use href="#i-lin"/></svg></div><h3>Linux</h3><p>64-bit, X11 or Wayland.</p><a class="btn" href="/download/linux"><svg aria-hidden="true"><use href="#i-down"/></svg>Download .tar.gz</a></div>
    <div class="plat reveal" style="--c:#b04fd6;--d:.24s"><span class="badge soon">New</span><div class="ic"><svg aria-hidden="true"><use href="#i-web"/></svg></div><h3>Web</h3><p>Nothing to install. Open a board in any modern browser.</p><a class="btn sun" href="/app"><svg aria-hidden="true"><use href="#i-arrow"/></svg>Open in your browser</a></div>
  </div>

  <div class="notes">
    <div class="note reveal"><h4><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><use href="#i-mac"/></svg>On a Mac</h4>The macOS app isn't notarised yet. The first time, right-click Inkboard in Applications and choose <b>Open</b>.</div>
    <div class="note reveal" style="--d:.06s"><h4><svg viewBox="0 0 24 24" aria-hidden="true"><use href="#i-win"/></svg>On Windows</h4>Windows may show a SmartScreen prompt. Choose <b>More info &rarr; Run anyway</b>.</div>
    <div class="note reveal" style="--d:.12s"><h4><svg viewBox="0 0 24 24" aria-hidden="true"><use href="#i-lock"/></svg>Signing in</h4>Inkboard uses your <b>Kenpath Google account</b>. Sign-in is Google Workspace only, for Kenpath's domains.</div>
  </div>
  <p class="rel reveal"><a href="${RELEASES}">All releases and notes</a></p>
</section>

<section class="s wrap" id="start">
  <span class="kicker reveal">Get started</span>
  <h2 class="reveal">Four steps, <em>zero</em> setup meetings.</h2>
  <ol class="steps">
    <li class="step reveal"><h3>Install</h3><p>Get the app for your device, or open Inkboard in your browser.</p></li>
    <li class="step reveal" style="--d:.08s"><h3>Sign in</h3><p>Account &rarr; Sign in with Google. The app shows a code to approve on your phone or computer.</p></li>
    <li class="step reveal" style="--d:.16s"><h3>Create a team board</h3><p>On the Team boards tab, or copy a board from this device.</p></li>
    <li class="step reveal" style="--d:.24s"><h3>Share it</h3><p>Invite people by email as can edit or can view, or open it to everyone in the org.</p></li>
  </ol>
</section>

<section class="finale">
  <svg class="doodle draw reveal" style="left:6%;top:14%;width:110px;color:var(--coral)" viewBox="0 0 110 110" aria-hidden="true"><path pathLength="1" d="M10 62C20 14 90 8 98 52S50 108 18 82"/></svg>
  <svg class="doodle draw reveal" style="right:7%;bottom:16%;width:120px;color:var(--teal)" viewBox="0 0 120 60" aria-hidden="true"><path pathLength="1" d="M4 40q14-34 28 0t28 0t28 0t28 0"/></svg>
  <div class="wrap">
    <h2 class="reveal">Pick up <em>a pen.</em></h2>
    <p class="reveal">One infinite board, the whole team on it.</p>
    <div class="cta reveal" style="--d:.1s">
      <a class="btn big primary" href="/app"><svg aria-hidden="true"><use href="#i-web"/></svg>Open in your browser</a>
      <a class="btn big" data-auto href="/download/mac"><svg aria-hidden="true"><use href="#i-down"/></svg><span class="lbl">Download for macOS</span></a>
    </div>
  </div>
</section>
</main>

<footer>
  <div class="wrap">
    <a class="brand" href="#">${LOGO}<span>Inkboard</span></a>
    <span>by Kenpath Labs</span>
    <a href="/app">Open in your browser</a>
    <a href="${RELEASES}">Releases</a>
    <a href="https://github.com/tejasgowda0012/inkboard">Source</a>
    <span class="sp">Draw something nice.</span>
  </div>
</footer>
<script>
(function () {
  var d = document, root = d.documentElement;
  var reduce = matchMedia("(prefers-reduced-motion: reduce)").matches;

  /* ---- 1. the main button picks the visitor's platform ---- */
  var ua = navigator.userAgent;
  var pick = /Android/i.test(ua) ? ["android", "Android"] : /Windows/i.test(ua) ? ["windows", "Windows"] : /Linux/i.test(ua) && !/CrOS/i.test(ua) ? ["linux", "Linux"] : null;
  [].forEach.call(d.querySelectorAll("[data-auto]"), function (a) {
    if (!pick) return;
    a.href = "/download/" + pick[0];
    var l = a.querySelector(".lbl");
    if (l) l.textContent = "Download for " + pick[1];
  });
  var mine = d.querySelector('.plat[data-p="' + (pick ? pick[0] : "mac") + '"]');
  if (mine) mine.classList.add("you");

  /* ---- 2. reveal on scroll, progress bar, drifting grid ---- */
  var targets = d.querySelectorAll(".reveal,.draw,.b");
  if ("IntersectionObserver" in window) {
    var io = new IntersectionObserver(function (es) {
      es.forEach(function (e) { if (e.isIntersecting) { e.target.classList.add("in"); io.unobserve(e.target); } });
    }, { threshold: 0.12, rootMargin: "0px 0px -5% 0px" });
    [].forEach.call(targets, function (el) { io.observe(el); });
  } else {
    [].forEach.call(targets, function (el) { el.classList.add("in"); });
  }
  var bar = d.getElementById("bar"), grid = d.getElementById("grid"), top = d.getElementById("top"), ticking = false;
  function onScroll() {
    if (ticking) return;
    ticking = true;
    requestAnimationFrame(function () {
      ticking = false;
      var y = window.pageYOffset, h = root.scrollHeight - innerHeight;
      bar.style.transform = "scaleX(" + (h > 0 ? Math.min(1, y / h) : 0) + ")";
      top.classList.toggle("stuck", y > 6);
      if (!reduce) grid.style.backgroundPosition = "0 " + (-y * 0.28) + "px";
    });
  }
  addEventListener("scroll", onScroll, { passive: true });
  onScroll();

  /* ---- 3. the live demo board ---- */
  var cv = d.getElementById("board");
  if (!cv || !cv.getContext) return;
  var ctx = cv.getContext("2d"), wrap = cv.parentNode;
  var W = 0, H = 0, dpr = 1, ready = false;
  var HAND = '"Caveat","Kalam","Segoe Print",cursive', UI = '"Bricolage Grotesque",system-ui,sans-serif';
  var theme = { ink: "#16141f", paper: "#fffefb" };
  var objs = [], tags = [], nextId = 1, selId = 0, tool = "pen", color = null, snapOn = true;
  var cur = null, drag = null, raf = 0, last = 0, running = false, touched = false;
  var NOTES = ["ship it", "+1", "Hindi next?", "demo Fri", "works offline", "v2 ✦", "love it", "who owns this?"];
  var ACT = [
    { k: "ana", name: "Ana", c: "#e5562f", kinds: ["box", "check", "wave", "note", "star"], at: [0.2, 0.62] },
    { k: "ben", name: "Ben", c: "#0b8a7a", kinds: ["circle", "arrow", "note", "heart"], at: [0.72, 0.72] },
    { k: "chi", name: "Chitra", c: "#b04fd6", kinds: ["wave", "circle", "check", "star", "note"], at: [0.55, 0.5] },
    { k: "dev", name: "Dev", c: "#3e63dd", kinds: ["arrow", "box", "heart", "note", "circle"], at: [0.86, 0.34] }
  ];
  function R() { return Math.random(); }
  function now() { return performance.now(); }

  function readTheme() {
    var cs = getComputedStyle(wrap);
    theme.ink = cs.getPropertyValue("--ink").trim() || theme.ink;
    theme.paper = cs.getPropertyValue("--board").trim() || theme.paper;
  }
  var mq = matchMedia("(prefers-color-scheme: dark)");
  var onTheme = function () { readTheme(); kick(); };
  if (mq.addEventListener) mq.addEventListener("change", onTheme); else if (mq.addListener) mq.addListener(onTheme);

  /* geometry helpers */
  function dist(a, b) { return Math.hypot(a.x - b.x, a.y - b.y); }
  function plen(p) { var l = 0; for (var i = 1; i < p.length; i++) l += dist(p[i], p[i - 1]); return l; }
  function resample(p, n) {
    if (p.length < 2) return p.slice();
    var tot = plen(p), out = [], seg = 0, acc = 0, i, t;
    for (i = 0; i < n; i++) {
      t = tot * i / (n - 1);
      while (seg < p.length - 2 && acc + dist(p[seg], p[seg + 1]) < t) { acc += dist(p[seg], p[seg + 1]); seg++; }
      var sl = dist(p[seg], p[seg + 1]) || 1, u = Math.min(1, Math.max(0, (t - acc) / sl));
      out.push({ x: p[seg].x + (p[seg + 1].x - p[seg].x) * u, y: p[seg].y + (p[seg + 1].y - p[seg].y) * u });
    }
    return out;
  }
  function lerpArr(a, b, u) { return a.map(function (p, i) { return { x: p.x + (b[i].x - p.x) * u, y: p.y + (b[i].y - p.y) * u }; }); }
  function bbox(o) {
    if (o.kind === "text") { ctx.font = o.size + "px " + HAND; return { x: o.x, y: o.y - o.size * 0.78, w: Math.max(12, ctx.measureText(o.text).width), h: o.size }; }
    var x0 = 1e9, y0 = 1e9, x1 = -1e9, y1 = -1e9;
    o.pts.forEach(function (p) { x0 = Math.min(x0, p.x); y0 = Math.min(y0, p.y); x1 = Math.max(x1, p.x); y1 = Math.max(y1, p.y); });
    return { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
  }
  function segDist(p, a, b) {
    var dx = b.x - a.x, dy = b.y - a.y, l2 = dx * dx + dy * dy, t = l2 ? Math.max(0, Math.min(1, ((p.x - a.x) * dx + (p.y - a.y) * dy) / l2)) : 0;
    return Math.hypot(p.x - (a.x + dx * t), p.y - (a.y + dy * t));
  }
  function hit(x, y) {
    for (var i = objs.length - 1; i >= 0; i--) {
      var o = objs[i], p = { x: x, y: y };
      if (o.dying) continue;
      if (o.kind === "text") { var b = bbox(o); if (x > b.x - 8 && x < b.x + b.w + 8 && y > b.y - 8 && y < b.y + b.h + 8) return o; continue; }
      if (o.pts.length === 1) { if (dist(o.pts[0], p) < 12) return o; continue; }
      for (var j = 1; j < o.pts.length; j++) if (segDist(p, o.pts[j - 1], o.pts[j]) < 11) return o;
    }
    return null;
  }

  /* recognise a rough stroke: line, rectangle/square, ellipse/circle */
  function recognise(pts) {
    var n = pts.length; if (n < 8) return null;
    var len = plen(pts); if (len < 60) return null;
    var a = pts[0], b = pts[n - 1], chord = dist(a, b), x0 = 1e9, y0 = 1e9, x1 = -1e9, y1 = -1e9, i;
    pts.forEach(function (p) { x0 = Math.min(x0, p.x); y0 = Math.min(y0, p.y); x1 = Math.max(x1, p.x); y1 = Math.max(y1, p.y); });
    var w = x1 - x0, h = y1 - y0;
    if (chord / len > 0.92 && chord > 50) {
      var e = { x: b.x, y: b.y }, ang = Math.atan2(b.y - a.y, b.x - a.x) * 180 / Math.PI, aa = Math.abs(ang);
      if (aa < 7 || aa > 173) e.y = a.y; else if (Math.abs(aa - 90) < 7) e.x = a.x;
      return { kind: "line", pts: [a, e], name: "line" };
    }
    if (chord > 0.25 * len || w < 28 || h < 28) return null;
    var cx = (x0 + x1) / 2, cy = (y0 + y1) / 2, ra = w / 2, rb = h / 2, eErr = 0, rErr = 0;
    pts.forEach(function (p) {
      eErr += Math.abs(Math.hypot((p.x - cx) / ra, (p.y - cy) / rb) - 1);
      rErr += Math.min(Math.abs(p.x - x0), Math.abs(p.x - x1), Math.abs(p.y - y0), Math.abs(p.y - y1));
    });
    eErr /= n; rErr = rErr / n / Math.min(ra, rb);
    if (Math.min(eErr, rErr) > 0.16) return null;
    if (eErr <= rErr) {
      var circ = Math.abs(w - h) < 0.16 * Math.max(w, h), rx = circ ? (w + h) / 4 : ra, ry = circ ? (w + h) / 4 : rb, out = [];
      for (i = 0; i <= 72; i++) out.push({ x: cx + rx * Math.cos(i / 72 * 6.2832), y: cy + ry * Math.sin(i / 72 * 6.2832) });
      return { kind: "ellipse", pts: out, name: circ ? "circle" : "ellipse" };
    }
    var sq = Math.abs(w - h) < 0.12 * Math.max(w, h), s = (w + h) / 2, hw = sq ? s / 2 : ra, hh = sq ? s / 2 : rb;
    return { kind: "rect", pts: [{ x: cx - hw, y: cy - hh }, { x: cx + hw, y: cy - hh }, { x: cx + hw, y: cy + hh }, { x: cx - hw, y: cy + hh }, { x: cx - hw, y: cy - hh }], name: sq ? "square" : "rectangle" };
  }

  /* generated, hand-wobbled strokes for the simulated teammates */
  function wob(p, amp) {
    var f1 = R() * 6, f2 = R() * 6, n = p.length;
    return p.map(function (q, i) { var t = i / n; return { x: q.x + amp * Math.sin(t * 9 + f1), y: q.y + amp * Math.sin(t * 7 + f2) }; });
  }
  function seg(a, b, n, out) { for (var i = 0; i < n; i++) { var t = i / n; out.push({ x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t }); } }
  function polyline(v, per) { var out = []; for (var i = 0; i < v.length - 1; i++) seg(v[i], v[i + 1], per, out); out.push(v[v.length - 1]); return out; }
  function shape(kind, x, y, w, h) {
    var p = [], i, t;
    if (kind === "box") {
      p = polyline([{ x: x, y: y }, { x: x + w, y: y }, { x: x + w, y: y + h }, { x: x, y: y + h }, { x: x + w * 0.02, y: y - h * 0.03 }, { x: x + w * 0.14, y: y }], 9);
      return { pts: wob(p, Math.min(w, h) * 0.035), fin: [{ x: x, y: y }, { x: x + w, y: y }, { x: x + w, y: y + h }, { x: x, y: y + h }, { x: x, y: y }] };
    }
    if (kind === "circle") {
      var fin = [];
      for (i = 0; i <= 44; i++) { t = -1.5708 + i / 44 * 6.6; p.push({ x: x + w / 2 + w / 2 * Math.cos(t), y: y + h / 2 + h / 2 * Math.sin(t) }); }
      for (i = 0; i <= 72; i++) fin.push({ x: x + w / 2 + w / 2 * Math.cos(i / 72 * 6.2832), y: y + h / 2 + h / 2 * Math.sin(i / 72 * 6.2832) });
      return { pts: wob(p, Math.min(w, h) * 0.03), fin: fin };
    }
    if (kind === "arrow") {
      var s = { x: x, y: y + h }, c = { x: x + w * 0.45, y: y - h * 0.2 }, e = { x: x + w, y: y + h * 0.25 }, dx = e.x - c.x, dy = e.y - c.y, l = Math.hypot(dx, dy) || 1, L = Math.min(22, w * 0.2);
      dx /= l; dy /= l;
      for (i = 0; i <= 18; i++) { t = i / 18; p.push({ x: (1 - t) * (1 - t) * s.x + 2 * (1 - t) * t * c.x + t * t * e.x, y: (1 - t) * (1 - t) * s.y + 2 * (1 - t) * t * c.y + t * t * e.y }); }
      var r1 = { x: e.x - L * (dx * 0.87 - dy * 0.5), y: e.y - L * (dy * 0.87 + dx * 0.5) }, r2 = { x: e.x - L * (dx * 0.87 + dy * 0.5), y: e.y - L * (dy * 0.87 - dx * 0.5) };
      seg(e, r1, 4, p); seg(r1, e, 4, p); seg(e, r2, 4, p); p.push(r2);
      return { pts: p };
    }
    if (kind === "check") return { pts: wob(polyline([{ x: x, y: y + h * 0.55 }, { x: x + w * 0.36, y: y + h }, { x: x + w, y: y }], 9), 1.2) };
    if (kind === "wave") { for (i = 0; i <= 40; i++) p.push({ x: x + w * i / 40, y: y + h / 2 + h * 0.45 * Math.sin(i / 40 * 15.7) }); return { pts: p }; }
    if (kind === "star") {
      var v = [], k; for (k = 0; k < 5; k++) { t = -1.5708 + k * 1.2566; v.push({ x: x + w / 2 + w / 2 * Math.cos(t), y: y + h / 2 + h / 2 * Math.sin(t) }); }
      return { pts: wob(polyline([v[0], v[2], v[4], v[1], v[3], v[0]], 8), 0.8) };
    }
    if (kind === "heart") {
      for (i = 0; i <= 44; i++) { t = i / 44 * 6.2832; p.push({ x: x + w / 2 + w / 34 * 16 * Math.pow(Math.sin(t), 3), y: y + h * 0.46 - h / 34 * (13 * Math.cos(t) - 5 * Math.cos(2 * t) - 2 * Math.cos(3 * t) - Math.cos(4 * t)) }); }
      return { pts: p };
    }
    return { pts: [] };
  }

  /* canvas sizing */
  function scaleAll(sx, sy) {
    function sp(a) { a.forEach(function (p) { p.x *= sx; p.y *= sy; }); }
    objs.forEach(function (o) { if (o.kind === "text") { o.x *= sx; o.y *= sy; } else sp(o.pts); o.morph = null; });
    ACT.forEach(function (a) { a.x *= sx; a.y *= sy; if (a.job) { if (a.job.pts) sp(a.job.pts); if (a.job.fin) sp(a.job.fin); if (a.job.kind === "note") { a.job.x *= sx; a.job.y *= sy; } } });
    tags = [];
  }
  function resize() {
    var r = wrap.getBoundingClientRect(), nw = Math.max(200, Math.round(r.width)), nh = Math.max(200, Math.round(r.height)), nd = Math.min(window.devicePixelRatio || 1, 2);
    if (nw === W && nh === H && nd === dpr) return;
    if (ready && W && H) scaleAll(nw / W, nh / H);
    W = nw; H = nh; dpr = nd; cv.width = Math.round(W * dpr); cv.height = Math.round(H * dpr);
    kick();
  }

  /* objects */
  function add(o) { o.id = nextId++; objs.push(o); return o; }
  function byId(id) { for (var i = 0; i < objs.length; i++) if (objs[i].id === id) return objs[i]; return null; }
  function ago(t, tn) { var m = Math.floor((tn - t) / 60000); return m < 1 ? "just now" : m + " min ago"; }
  function info(o, tn) {
    var who = o.author === "You" ? "you" : o.author;
    if (o.editor) return "Added by " + who + " · edited by " + (o.editor === "You" ? "you" : o.editor) + " " + ago(o.editAt || o.born, tn);
    return "Added by " + who + " · " + ago(o.born, tn);
  }
  function other(name) { var l = ACT.filter(function (a) { return a.name !== name; }); return l[Math.floor(R() * l.length)].name; }
  function seed() {
    var t = now(), A = ACT[0], B = ACT[1], C = ACT[2], D = ACT[3];
    function mk(a, kind, x, y, w, h, extra) {
      var s = shape(kind, x * W, y * H, w * W, h * H), o = { kind: "free", pts: s.fin || s.pts, poly: !!s.fin, color: a.c, author: a.name, born: t - (2 + Math.floor(R() * 5)) * 60000, w: 3.2, flash: 0 };
      for (var k in extra) o[k] = extra[k];
      return add(o);
    }
    mk(A, "box", 0.07, 0.15, 0.4, 0.24, { editor: "Ben", editAt: t - 2 * 60000 - 4000 });
    add({ kind: "text", text: "Launch plan", size: Math.max(28, Math.min(40, W * 0.07)), x: 0.115 * W, y: 0.295 * H, color: B.c, author: B.name, born: t - 6 * 60000, flash: 0 });
    mk(C, "circle", 0.64, 0.12, 0.22, 0.27, { editor: "Dev", editAt: t - 3 * 60000 - 9000 });
    mk(C, "check", 0.7, 0.2, 0.1, 0.1, {});
    mk(D, "arrow", 0.5, 0.2, 0.11, 0.09, {});
    mk(A, "wave", 0.08, 0.44, 0.3, 0.035, {});
    add({ kind: "text", text: "ship it", size: Math.max(24, Math.min(32, W * 0.055)), x: 0.1 * W, y: 0.56 * H, color: D.c, author: D.name, born: t - 3 * 60000, flash: 0 });
    ACT.forEach(function (a, i) { a.x = a.at[0] * W; a.y = a.at[1] * H; a.ph = i * 1.7; a.state = "idle"; a.next = t + 1100 + i * 1500; a.last = ""; });
  }

  /* actors: travel, draw, snap, tag */
  function freeSpot(w, h) {
    var pad = 14, x, y, i;
    for (i = 0; i < 16; i++) {
      x = pad + R() * Math.max(1, W - w - 2 * pad); y = 52 + R() * Math.max(1, H - h - 52 - 54);
      var clash = objs.some(function (o) { if (o.dying) return false; var b = bbox(o); return x < b.x + b.w + 12 && x + w + 12 > b.x && y < b.y + b.h + 12 && y + h + 12 > b.y; });
      if (!clash) break;
    }
    return { x: x, y: y };
  }
  function begin(a, tn) {
    var kind = a.kinds[Math.floor(R() * a.kinds.length)];
    if (kind === a.last) kind = a.kinds[(a.kinds.indexOf(kind) + 1) % a.kinds.length];
    a.last = kind;
    var s = Math.min(W, H), w, h, text = null;
    if (kind === "box") { w = s * (0.2 + R() * 0.14); h = w * (0.55 + R() * 0.3); }
    else if (kind === "circle") { w = s * (0.15 + R() * 0.1); h = w * (0.85 + R() * 0.25); }
    else if (kind === "arrow") { w = s * (0.18 + R() * 0.14); h = s * (0.07 + R() * 0.08); }
    else if (kind === "check") { w = s * 0.08; h = s * 0.08; }
    else if (kind === "wave") { w = s * (0.18 + R() * 0.12); h = s * 0.04; }
    else if (kind === "star") { w = s * 0.11; h = w; }
    else if (kind === "heart") { w = s * 0.095; h = w * 0.92; }
    else { text = NOTES[Math.floor(R() * NOTES.length)]; ctx.font = "30px " + HAND; w = ctx.measureText(text).width; h = 30; }
    var sp = freeSpot(w, h), job;
    if (text) job = { kind: "note", text: text, x: sp.x, y: sp.y + h * 0.8, idx: 0 };
    else {
      var sh = shape(kind, sp.x, sp.y, w, h), pts = sh.pts, n = Math.max(8, Math.ceil(plen(pts) / 5));
      job = { kind: kind, pts: resample(pts, n), fin: sh.fin || null, idx: 0 };
    }
    a.job = job; a.speed = 190 + R() * 110;
    a.state = "travel"; a.t0 = tn; a.from = { x: a.x, y: a.y }; a.to = text ? { x: job.x, y: job.y } : job.pts[0];
    a.dur = 450 + dist(a.from, a.to) * 1.1;
  }
  function busy(a, on) { var el = d.querySelector('[data-who="' + a.k + '"]'); if (el) el.classList.toggle("busy", on); }
  function startObj(a, tn) {
    var j = a.job, o = { kind: j.kind === "note" ? "text" : "free", color: a.c, author: a.name, editor: R() < 0.35 ? other(a.name) : null, born: tn, flash: 0, w: 3.2, drawing: true };
    if (o.editor) o.editAt = tn;
    if (o.kind === "text") { o.text = ""; o.size = 30; o.x = j.x; o.y = j.y; } else o.pts = [j.pts[0]];
    a.obj = add(o); busy(a, true);
  }
  function finish(a, tn) {
    var o = a.obj, j = a.job; o.drawing = false;
    if (o.kind === "text") o.text = j.text;
    if (j.fin) { o.morph = { from: resample(o.pts, 73), to: resample(j.fin, 73), t0: tn, dur: 340 }; o.pts = j.fin; o.poly = true; }
    o.flash = tn;
    var b = bbox(o); tags.push({ text: "Added by " + a.name, color: a.c, x: b.x - 4, y: b.y - 30, from: tn, until: tn + 2300 });
    a.state = "idle"; a.next = tn + 1600 + R() * 2600; a.obj = null; busy(a, false);
    var live = objs.filter(function (q) { return !q.mine && !q.dying; });
    if (live.length > 12) live[0].dying = tn;
  }
  function stepActor(a, tn, dt) {
    var j = a.job, n;
    if (a.state === "idle") {
      a.x += Math.sin(tn / 700 + a.ph) * 0.3; a.y += Math.cos(tn / 900 + a.ph) * 0.25;
      if (tn >= a.next) begin(a, tn);
    } else if (a.state === "travel") {
      var u = Math.min(1, (tn - a.t0) / a.dur), e = u < 0.5 ? 2 * u * u : 1 - Math.pow(-2 * u + 2, 2) / 2;
      a.x = a.from.x + (a.to.x - a.from.x) * e; a.y = a.from.y + (a.to.y - a.from.y) * e - Math.sin(u * 3.1416) * 16;
      if (u >= 1) { a.state = "draw"; startObj(a, tn); }
    } else if (a.state === "draw") {
      if (j.kind === "note") {
        j.idx += dt / 1000 * 11; n = Math.min(j.text.length, Math.floor(j.idx)); a.obj.text = j.text.slice(0, n);
        ctx.font = "30px " + HAND; a.x = j.x + ctx.measureText(a.obj.text).width + 4; a.y = j.y - 6;
        if (n >= j.text.length) finish(a, tn);
      } else {
        j.idx += dt / 1000 * a.speed / 5; n = Math.min(j.pts.length, Math.floor(j.idx) + 1);
        a.obj.pts = j.pts.slice(0, n); a.x = j.pts[n - 1].x; a.y = j.pts[n - 1].y;
        if (n >= j.pts.length) finish(a, tn);
      }
    }
  }

  /* drawing */
  function rr(x, y, w, h, r) { ctx.beginPath(); ctx.moveTo(x + r, y); ctx.arcTo(x + w, y, x + w, y + h, r); ctx.arcTo(x + w, y + h, x, y + h, r); ctx.arcTo(x, y + h, x, y, r); ctx.arcTo(x, y, x + w, y, r); ctx.closePath(); }
  function pill(x, y, text, bg, fg, al) {
    ctx.font = "700 12px " + UI; var w = ctx.measureText(text).width + 18, h = 24;
    x = Math.max(4, Math.min(W - w - 4, x)); y = Math.max(4, Math.min(H - h - 4, y));
    ctx.save(); ctx.globalAlpha = al; ctx.fillStyle = bg; rr(x, y, w, h, 12); ctx.fill(); ctx.fillStyle = fg; ctx.textBaseline = "middle"; ctx.fillText(text, x + 9, y + h / 2 + 0.5); ctx.restore();
  }
  function trace(p, poly) {
    ctx.beginPath(); ctx.moveTo(p[0].x, p[0].y);
    if (poly || p.length < 3) { for (var i = 1; i < p.length; i++) ctx.lineTo(p[i].x, p[i].y); return; }
    for (var k = 1; k < p.length - 1; k++) ctx.quadraticCurveTo(p[k].x, p[k].y, (p[k].x + p[k + 1].x) / 2, (p[k].y + p[k + 1].y) / 2);
    ctx.lineTo(p[p.length - 1].x, p[p.length - 1].y);
  }
  function renderObj(o, tn) {
    var al = 1; if (o.dying) { al = 1 - (tn - o.dying) / 700; if (al <= 0) return; }
    var col = o.color || theme.ink, f = o.flash && !reduce ? Math.max(0, 1 - (tn - o.flash) / 1100) : 0;
    ctx.save(); ctx.globalAlpha = al; ctx.strokeStyle = col; ctx.fillStyle = col; ctx.lineWidth = o.w || 3.2;
    if (f > 0) { ctx.shadowColor = col; ctx.shadowBlur = 24 * f; ctx.lineWidth += 2.4 * f; }
    if (o.kind === "text") { ctx.font = o.size + "px " + HAND; ctx.textBaseline = "alphabetic"; ctx.fillText(o.text, o.x, o.y); }
    else if (o.pts.length) {
      var p = o.pts, poly = o.poly;
      if (o.morph) { var u = Math.min(1, (tn - o.morph.t0) / o.morph.dur); if (u >= 1 || reduce) o.morph = null; else { u = 1 - Math.pow(1 - u, 3); p = lerpArr(o.morph.from, o.morph.to, u); poly = true; } }
      if (p.length === 1) { ctx.beginPath(); ctx.arc(p[0].x, p[0].y, (o.w || 3.2) / 1.1, 0, 6.2832); ctx.fill(); } else { trace(p, poly); ctx.stroke(); }
    }
    ctx.restore();
  }
  function cursor(a) {
    ctx.save(); ctx.translate(a.x, a.y); ctx.fillStyle = a.c; ctx.strokeStyle = "#fff"; ctx.lineWidth = 1.6; ctx.lineJoin = "round";
    ctx.beginPath(); ctx.moveTo(0, 0); ctx.lineTo(0, 17); ctx.lineTo(4.6, 13); ctx.lineTo(8, 20.5); ctx.lineTo(11, 19.2); ctx.lineTo(7.7, 11.8); ctx.lineTo(13, 11.4); ctx.closePath(); ctx.fill(); ctx.stroke();
    ctx.font = "700 12px " + UI; var w = ctx.measureText(a.name).width + 16;
    ctx.fillStyle = a.c; rr(12, 17, w, 21, 10.5); ctx.fill(); ctx.fillStyle = "#fff"; ctx.textBaseline = "middle"; ctx.fillText(a.name, 20, 28);
    ctx.restore();
  }
  function draw(tn) {
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0); ctx.clearRect(0, 0, W, H); ctx.lineCap = "round"; ctx.lineJoin = "round";
    objs.forEach(function (o) { renderObj(o, tn); });
    var s = selId ? byId(selId) : null;
    if (s && !s.dying) {
      var b = bbox(s); ctx.save(); ctx.strokeStyle = "#6750d8"; ctx.fillStyle = theme.paper; ctx.lineWidth = 1.6; ctx.setLineDash([5, 4]); ctx.strokeRect(b.x - 8, b.y - 8, b.w + 16, b.h + 16); ctx.setLineDash([]);
      [[b.x - 8, b.y - 8], [b.x + b.w + 8, b.y - 8], [b.x - 8, b.y + b.h + 8], [b.x + b.w + 8, b.y + b.h + 8]].forEach(function (c) { ctx.fillRect(c[0] - 4, c[1] - 4, 8, 8); ctx.strokeRect(c[0] - 4, c[1] - 4, 8, 8); });
      ctx.restore();
      var below = b.y + b.h + 20; pill(b.x - 8, below + 26 > H ? b.y - 40 : below, info(s, tn), theme.ink, theme.paper, 1);
    }
    tags = tags.filter(function (t) { return tn < t.until; });
    tags.forEach(function (t) { var u = Math.min(1, (tn - t.from) / 220, (t.until - tn) / 300); pill(t.x, t.y - (1 - u) * 6, t.text, t.color, "#fff", Math.max(0, u)); });
    ACT.forEach(cursor);
  }

  /* loop */
  function frame(t) {
    raf = 0; var dt = Math.min(t - last, 48); last = t;
    if (running) {
      ACT.forEach(function (a) { stepActor(a, t, dt); });
      objs = objs.filter(function (o) { return !(o.dying && t - o.dying > 700); });
      if (selId && !byId(selId)) selId = 0;
    }
    draw(t);
    if (running) raf = requestAnimationFrame(frame);
  }
  function kick() { if (!raf && ready) raf = requestAnimationFrame(frame); }
  function setRunning(on) { on = on && !reduce && !d.hidden; if (on === running) return; running = on; if (on) { last = now(); } kick(); }
  if ("IntersectionObserver" in window) new IntersectionObserver(function (es) { setRunning(es[0].isIntersecting); }, { threshold: 0.05 }).observe(cv);
  d.addEventListener("visibilitychange", function () { setRunning(!d.hidden); });

  /* user input */
  function pt(e) { var r = cv.getBoundingClientRect(); return { x: e.clientX - r.left, y: e.clientY - r.top }; }
  var hint = d.getElementById("hint");
  function dismissHint() { if (!touched) { touched = true; hint.classList.add("gone"); } }
  cv.addEventListener("pointerdown", function (e) {
    if (e.pointerType === "mouse" && e.button !== 0) return;
    var p = pt(e), tn = now(); dismissHint(); if (reduce) tags = [];
    cv.setPointerCapture(e.pointerId); e.preventDefault();
    if (tool === "select") {
      var o = hit(p.x, p.y); selId = o ? o.id : 0;
      if (o) drag = { o: o, x: p.x, y: p.y, moved: false };
      kick(); return;
    }
    selId = 0;
    cur = add({ kind: "free", pts: [p], color: color, author: "You", editor: null, born: tn, flash: 0, w: 3.4, mine: true });
    kick();
  });
  cv.addEventListener("pointermove", function (e) {
    var p = pt(e);
    if (cur) { var l = cur.pts[cur.pts.length - 1]; if (dist(l, p) > 1.8) { cur.pts.push(p); kick(); } }
    else if (drag) {
      var dx = p.x - drag.x, dy = p.y - drag.y; if (!dx && !dy) return; drag.moved = true; drag.x = p.x; drag.y = p.y;
      var o = drag.o; if (o.kind === "text") { o.x += dx; o.y += dy; } else { o.pts.forEach(function (q) { q.x += dx; q.y += dy; }); o.morph = null; }
      kick();
    } else if (tool === "select") cv.style.cursor = hit(p.x, p.y) ? "pointer" : "default";
  });
  function release() {
    var tn = now();
    if (drag) { if (drag.moved) { drag.o.editor = "You"; drag.o.editAt = tn; } drag = null; kick(); return; }
    if (!cur) return;
    var o = cur; cur = null;
    if (snapOn && o.pts.length > 7) {
      var r = recognise(o.pts);
      if (r) {
        if (reduce) { o.pts = r.pts; } else { o.morph = { from: resample(o.pts, 73), to: resample(r.pts, 73), t0: tn, dur: 320 }; o.pts = r.pts; o.flash = tn; }
        o.poly = true;
        var b = bbox(o); tags.push({ text: "Snapped to " + r.name + " ✓", color: color || theme.ink, x: b.x, y: b.y - 32, from: tn, until: reduce ? 1e12 : tn + 2000 });
        if (!color) tags[tags.length - 1].color = "#6750d8";
      }
    }
    kick();
  }
  cv.addEventListener("pointerup", release);
  cv.addEventListener("pointercancel", release);

  d.getElementById("tools").addEventListener("click", function (e) {
    var b = e.target.closest("button"); if (!b) return;
    var tn = now();
    if (b.dataset.tool) {
      tool = b.dataset.tool; [].forEach.call(d.querySelectorAll("[data-tool]"), function (x) { x.setAttribute("aria-pressed", String(x === b)); });
      cv.className = tool === "select" ? "sel" : ""; cv.style.cursor = ""; if (tool === "pen") selId = 0;
    } else if (b.dataset.c) {
      color = b.dataset.c === "ink" ? null : b.dataset.c; [].forEach.call(d.querySelectorAll("[data-c]"), function (x) { x.setAttribute("aria-pressed", String(x === b)); });
      if (tool !== "pen") d.querySelector('[data-tool="pen"]').click();
    } else if (b.id === "snap") { snapOn = !snapOn; b.setAttribute("aria-pressed", String(snapOn)); }
    else if (b.id === "undo") {
      for (var i = objs.length - 1; i >= 0; i--) if (objs[i].mine && !objs[i].dying) { if (reduce) objs.splice(i, 1); else objs[i].dying = tn; break; }
    } else if (b.id === "clear") {
      objs = objs.filter(function (o) { if (!o.mine) return true; if (!reduce) { o.dying = o.dying || tn; return true; } return false; });
    }
    kick();
  });

  /* go */
  readTheme(); resize();
  seed(); ready = true;
  if (reduce) { ACT.forEach(function (a) { busy(a, false); }); }
  if (window.ResizeObserver) new ResizeObserver(resize).observe(wrap); else addEventListener("resize", resize);
  if (d.fonts && d.fonts.ready) d.fonts.ready.then(function () { kick(); });
  kick();
})();
</script>
</body>
</html>`;
