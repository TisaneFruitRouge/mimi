// Drives the web UI in a headless Chromium, so UI changes can be checked without
// touching anyone's screen.
//
//   node scripts/ui-check.mjs <url> <steps.json> <chrome-profile-dir>
//
// Open a login link first (POST /v1/web/login-link) with the same profile dir so the
// session cookie is set. Cookies are per host, not per port: signing in to a second
// instance on 127.0.0.1 signs the profile out of the first.
//
// Steps, run in order:
//   {"wait": ms}
//   {"eval": "js expression"}                      prints the JSON result
//   {"click": {"css": "button", "text": "Save"}}   first match containing text
//   {"key": {"key": "k", "code": "KeyK", "vk": 75, "ctrl": true}}
//   {"type": "text"}                               into the focused element
//   {"shot": "/path/out.png"}
import { spawn } from "node:child_process";
import { readFileSync } from "node:fs";
const [url, stepsFile, profile] = process.argv.slice(2);
const steps = JSON.parse(readFileSync(stepsFile, "utf8"));
const port = 9300 + Math.floor(Math.random() * 500);
const chrome = spawn("chromium", ["--headless=new", "--disable-gpu", "--hide-scrollbars", "--no-first-run",
  `--user-data-dir=${profile}`, `--remote-debugging-port=${port}`, "--window-size=1440,900", "about:blank"], { stdio: "ignore" });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let target;
for (let i = 0; i < 50 && !target; i++) {
  await sleep(200);
  try { target = (await (await fetch(`http://127.0.0.1:${port}/json`)).json()).find((t) => t.type === "page"); } catch {}
}
const ws = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((r) => (ws.onopen = r));
let id = 0; const pending = new Map();
ws.onmessage = (m) => { const d = JSON.parse(m.data); if (d.id && pending.has(d.id)) { pending.get(d.id)(d); pending.delete(d.id); } };
const send = (method, params = {}) => new Promise((r) => { const i = ++id; pending.set(i, r); ws.send(JSON.stringify({ id: i, method, params })); });
const evaluate = async (expr) => (await send("Runtime.evaluate", { expression: expr, returnByValue: true, awaitPromise: true })).result?.result?.value;
await send("Page.enable");
await send("Emulation.setDeviceMetricsOverride", { width: 1440, height: 900, deviceScaleFactor: 1, mobile: false });
await send("Page.navigate", { url });
await sleep(2500);
for (const s of steps) {
  if (s.wait) await sleep(s.wait);
  if (s.eval) console.log(JSON.stringify(await evaluate(s.eval)));
  if (s.click) {
    const box = await evaluate(`(() => { const el = [...document.querySelectorAll(${JSON.stringify(s.click.css ?? "button")})].find(e => !${JSON.stringify(s.click.text ?? null)} || e.textContent.includes(${JSON.stringify(s.click.text ?? "")})); if (!el) return null; const r = el.getBoundingClientRect(); return {x: r.x + r.width/2, y: r.y + r.height/2}; })()`);
    if (!box) { console.log("click target not found", JSON.stringify(s.click)); continue; }
    for (const type of ["mouseMoved", "mousePressed", "mouseReleased"])
      await send("Input.dispatchMouseEvent", { type, x: box.x, y: box.y, button: "left", clickCount: 1 });
    await sleep(400);
  }
  if (s.key) {
    const mods = (s.key.ctrl ? 2 : 0) | (s.key.shift ? 8 : 0);
    await send("Input.dispatchKeyEvent", { type: "keyDown", key: s.key.key, code: s.key.code, windowsVirtualKeyCode: s.key.vk, modifiers: mods });
    await send("Input.dispatchKeyEvent", { type: "keyUp", key: s.key.key, code: s.key.code, windowsVirtualKeyCode: s.key.vk, modifiers: mods });
    await sleep(400);
  }
  if (s.type) { await send("Input.insertText", { text: s.type }); await sleep(300); }
  if (s.shot) {
    const { result } = await send("Page.captureScreenshot", { format: "png" });
    (await import("node:fs")).writeFileSync(s.shot, Buffer.from(result.data, "base64"));
  }
}
ws.close(); chrome.kill();
