// Captures the mock UI ("npm run dev" in app/, port 5173) in headless Chrome at a realistic ~60 fps,
// for reviewing the face-in-fog visuals. One browser per call; prints fps / face presence per shot.
//
// usage: node scripts/capture-face.mjs <outDir> [female|male] [on|off] [seconds "7,11,19"]
//   env BROWSER=<path to chrome.exe>   (default: Google Chrome)
//   env NOSHOT=1                       only log fps / presence, no screenshots
//
// Pitfalls this script works around (found 2026-11-21):
// - Headless Edge throttles the page like a background tab once the conversation starts
//   (rAF stops, timers ~1 Hz); screenshots then show a stale, slow-motion simulation. Use Chrome.
// - An unthrottled headless page runs at 600-1200 fps, which looks nothing like a 60 Hz display:
//   requestAnimationFrame is gated to one slot per 1/60 s (shared by all callbacks of a frame).
// - A running screencast keeps the compositor producing frames between screenshots.
import { spawn } from 'node:child_process';
import { mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const [outDir, gender = 'female', face = 'on', timesArg = '7,11,19'] = process.argv.slice(2);
if (!outDir) {
  console.error('usage: node scripts/capture-face.mjs <outDir> [female|male] [on|off] [seconds]');
  process.exit(2);
}
const times = timesArg.split(',').map(Number);
const port = 9300 + Math.floor(Math.random() * 400);
const prof = join(outDir, `.chrome-profile-${port}`);
mkdirSync(prof, { recursive: true });
const browser = spawn(process.env.BROWSER || 'C:/Program Files/Google/Chrome/Application/chrome.exe', [
  '--headless=new', `--remote-debugging-port=${port}`, `--user-data-dir=${prof}`, '--window-size=1600,900',
  '--ignore-gpu-blocklist', '--disable-background-timer-throttling', '--disable-renderer-backgrounding',
  '--disable-backgrounding-occluded-windows', '--disable-features=IntensiveWakeUpThrottling,CalculateNativeWinOcclusion',
  '--autoplay-policy=no-user-gesture-required', 'about:blank',
], { stdio: 'ignore' });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

let ws;
try {
  let list;
  for (let i = 0; i < 50 && !list; i++) {
    await sleep(200);
    try { list = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json(); } catch { /* not up yet */ }
  }
  ws = new WebSocket(list.find((t) => t.type === 'page').webSocketDebuggerUrl);
  await new Promise((r) => ws.addEventListener('open', r, { once: true }));
  let id = 0;
  const pending = new Map();
  ws.addEventListener('message', (ev) => {
    const m = JSON.parse(ev.data);
    if (m.id && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); }
    if (m.method === 'Page.screencastFrame') {
      ws.send(JSON.stringify({ id: ++id, method: 'Page.screencastFrameAck', params: { sessionId: m.params.sessionId } }));
    }
  });
  const send = (method, params = {}) => new Promise((r) => { const i = ++id; pending.set(i, r); ws.send(JSON.stringify({ id: i, method, params })); });
  const evaluate = async (expression) => {
    const r = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
    if (r.result?.exceptionDetails) throw new Error(JSON.stringify(r.result.exceptionDetails));
    return r.result?.result?.value;
  };

  await send('Page.enable');
  await send('Page.startScreencast', { format: 'jpeg', quality: 5, maxWidth: 160, maxHeight: 90, everyNthFrame: 1 });
  await send('Emulation.setDeviceMetricsOverride', { width: 1600, height: 900, deviceScaleFactor: 1, mobile: false });
  await send('Page.addScriptToEvaluateOnNewDocument', { source: `(() => {
    const raf = window.requestAnimationFrame.bind(window); let next = 0, slot = -1;
    window.requestAnimationFrame = (cb) => raf(function tick(t) {
      if (t !== slot) { if (t < next) { raf(tick); return; } slot = t; next = Math.max(next + 1000 / 60, t - 4); }
      cb(t);
    });
  })();` });
  await send('Page.navigate', { url: 'http://localhost:5173/?mock&debug' });

  // dismiss the headphone hint, pick the head, press Start
  let started = false;
  for (let i = 0; i < 60 && !started; i++) {
    await sleep(500);
    started = await evaluate(`(() => {
      const hint = document.querySelector('.overlay.hint.is-visible button'); if (hint) { hint.click(); return false; }
      const h = window.__haru; if (!h) return false;
      h.setFace(${face === 'on'}, '${gender === 'male' ? 'male' : 'female'}'); if (h.fpsEl) h.fpsEl.style.display = 'none';
      const btn = document.querySelector('.overlay.start.is-visible .start-btn:not([disabled])');
      if (!btn) return false; btn.click(); return true; })()`);
  }
  if (!started) throw new Error('could not start the mock conversation (is the dev server on :5173 running?)');

  const t0 = Date.now();
  for (const t of times) {
    await sleep(Math.max(0, t * 1000 - (Date.now() - t0)));
    const stats = await evaluate(`JSON.stringify({ fps: Math.round(__haru.fpsValue), presence: +(__haru.face?.presence ?? 0).toFixed(2),
      modelRms: +(__haru.audio?.modelRms ?? 0).toFixed(3), quality: __haru.level })`);
    if (process.env.NOSHOT) { console.log(`t=${t}s`, stats); continue; }
    const shot = await send('Page.captureScreenshot', { format: 'png' });
    const file = join(outDir, `${gender}-${face}-t${t}.png`);
    writeFileSync(file, Buffer.from(shot.result.data, 'base64'));
    console.log(file, stats);
  }
} finally {
  try { ws?.close(); } catch { /* ignore */ }
  browser.kill();
  await sleep(500);
  try { rmSync(prof, { recursive: true, force: true }); } catch { /* the browser may still hold files */ }
}
