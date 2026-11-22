// Rustaco against Monaco on the same synthetic Rust files, in the same Chromium:
// load time (text in, highlighted, two frames drawn), keystroke latency (keydown to the end
// of the next frame), and scroll latency. Prints a JSON report.
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { extname, join, normalize } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as playwright from 'playwright';

const root = fileURLToPath(new URL('.', import.meta.url));
const types = { '.html': 'text/html', '.js': 'text/javascript', '.mjs': 'text/javascript', '.wasm': 'application/wasm', '.css': 'text/css', '.ttf': 'font/ttf' };
const server = createServer(async (req, res) => {
  const path = normalize(decodeURIComponent(new URL(req.url, 'http://x').pathname)).replace(/^[/\\]+/, '');
  const file = join(root, path);
  // Read first: a missing file (a source map, a favicon) must get its 404 before any header.
  const body = await readFile(file).catch(() => null);
  if (body) res.writeHead(200, { 'content-type': types[extname(file)] || 'application/octet-stream' }).end(body);
  else res.writeHead(404).end();
}).listen(47900, '127.0.0.1');

// SIZES=5000 EDITORS=rustaco,rustaco?text=dom BROWSER=firefox node run.mjs
const SIZES = (process.env.SIZES ?? '5000,100000').split(',').map(Number);
const EDITORS = (process.env.EDITORS ?? 'rustaco,rustaco?text=dom,rustaco?text=webgl,monaco').split(',');
const BROWSER = process.env.BROWSER ?? 'chromium';
// Runs per editor and size; the report keeps every run, the summary the median.
const REPEAT = Number(process.env.REPEAT ?? 1);
// Extra browser flags, e.g. ARGS="--use-angle=d3d11 --enable-gpu" for WebGL on the GPU.
const ARGS = (process.env.ARGS ?? '').split(' ').filter(Boolean);

function stats(xs) {
  const s = [...xs].sort((a, b) => a - b);
  const q = (p) => s[Math.min(s.length - 1, Math.floor(p * s.length))];
  return { median: +q(0.5).toFixed(2), p95: +q(0.95).toFixed(2), mean: +(s.reduce((a, b) => a + b, 0) / s.length).toFixed(2) };
}

const browser = await playwright[BROWSER].launch({ args: ARGS });
const report = { browser: `${BROWSER} ${browser.version()}`, results: [] };
for (const lines of SIZES) {
  for (let run = 0; run < REPEAT; run++) for (const name of EDITORS) {
    const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
    const [file, query] = name.split('?');
    await page.goto(`http://127.0.0.1:47900/${file}.html${query ? `?${query}` : ''}`);
    await page.waitForFunction(() => window.benchReady, null, { timeout: 60_000 });
    // Chromium reports its main-thread task time; the other browsers only the latency below.
    const cdp = BROWSER === 'chromium' ? await page.context().newCDPSession(page) : null;
    await cdp?.send('Performance.enable');
    const busy = async () => {
      if (!cdp) return 0;
      const { metrics } = await cdp.send('Performance.getMetrics');
      return metrics.find((m) => m.name === 'TaskDuration').value * 1000;
    };
    // Warm up: language modules, workers and JIT, so load measures the text, not the setup.
    await page.evaluate(() => window.bench.load('fn warm() {}\n'));
    await page.waitForTimeout(1000);
    const load = await page.evaluate(async (n) => {
      const parts = [];
      for (let i = 0; i < n; i++) {
        parts.push(i % 20 === 0 ? `fn f${i}() {` : i % 20 === 19 ? '}' : `    let value_${i} = compute(${i}, "text ${i}"); // note ${i}`);
      }
      const text = parts.join('\n');
      window.textLength = text.length;
      return window.bench.load(text);
    }, lines);
    // Let background work settle (parsing, tokenization) before typing.
    await page.waitForTimeout(3000);
    await page.evaluate(() => {
      window.lat = [];
      window.addEventListener('keydown', () => {
        const t0 = performance.now();
        requestAnimationFrame(() => setTimeout(() => window.lat.push(performance.now() - t0)));
      }, true);
      window.bench.setCaret(Math.floor(window.textLength / 2));
    });
    for (let i = 0; i < 10; i++) {
      await page.keyboard.press('x');
      await page.waitForTimeout(40);
    }
    await page.waitForTimeout(1000);
    // Main-thread work per keystroke (Chromium): all tasks between the first key and a
    // settled page, including background work the key starts (tokenizing, rendering), over
    // 60 keys, with the idle rate subtracted.
    const typeKeys = async () => {
      for (let i = 0; i < 60; i++) {
        await page.keyboard.press('x');
        await page.waitForTimeout(40);
      }
      await page.waitForTimeout(1500);
    };
    const before = await busy();
    await typeKeys();
    const idle = await busy();
    await page.waitForTimeout(1500);
    const idleBaseline = (await busy()) - idle;
    const perKey = (idle - before - idleBaseline * (60 * 40 + 1500) / 1500) / 60;
    // In every browser, a second 60 keys: a probe posts messages to itself back to back and
    // sums the gaps longer than 2 ms, the time the main thread was busy with something else;
    // the same probe on an idle page is subtracted.
    const probe = () => page.evaluate(() => {
      window.blocked = 0;
      window.probing = true;
      const ch = new MessageChannel();
      let last = performance.now();
      ch.port1.onmessage = () => {
        const now = performance.now();
        if (now - last > 2) window.blocked += now - last;
        last = now;
        if (window.probing) ch.port2.postMessage(0);
      };
      ch.port2.postMessage(0);
    });
    const stopProbe = () => page.evaluate(() => { window.probing = false; return window.blocked; });
    await probe();
    await typeKeys();
    const blockedTyping = await stopProbe();
    await probe();
    await page.waitForTimeout(60 * 40 + 1500);
    const blockedIdle = await stopProbe();
    const blockedPerKey = (blockedTyping - blockedIdle) / 60;
    const typed = await page.evaluate(() => window.bench.value().length - window.textLength);
    const keys = await page.evaluate(() => window.lat.slice(10));
    const scroll = await page.evaluate(async () => {
      const out = [];
      for (let i = 0; i < 40; i++) {
        const t0 = performance.now();
        window.bench.scrollBy(400);
        await new Promise((r) => requestAnimationFrame(() => setTimeout(r)));
        out.push(performance.now() - t0);
      }
      return out.slice(5);
    });
    report.results.push({ editor: name, lines, chars: await page.evaluate(() => window.textLength), loadMs: +load.toFixed(1), typed, mainThreadMsPerKey: cdp ? +perKey.toFixed(2) : null, blockedMsPerKey: +blockedPerKey.toFixed(2), keyToFrameEnd: stats(keys), scroll: stats(scroll) });
    await page.close();
  }
}
await browser.close();
server.close();
console.log(JSON.stringify(report, null, 2));
