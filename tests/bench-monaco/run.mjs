// Rustaco against Monaco on the same synthetic Rust files, in the same Chromium:
// load time (text in, highlighted, two frames drawn), keystroke latency (keydown to the end
// of the next frame), and scroll latency. Prints a JSON report.
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { extname, join, normalize } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const root = fileURLToPath(new URL('.', import.meta.url));
const types = { '.html': 'text/html', '.js': 'text/javascript', '.mjs': 'text/javascript', '.wasm': 'application/wasm', '.css': 'text/css', '.ttf': 'font/ttf' };
const server = createServer(async (req, res) => {
  const path = normalize(decodeURIComponent(new URL(req.url, 'http://x').pathname)).replace(/^[/\\]+/, '');
  const file = join(root, path);
  try {
    res.writeHead(200, { 'content-type': types[extname(file)] || 'application/octet-stream' }).end(await readFile(file));
  } catch {
    res.writeHead(404).end();
  }
}).listen(47900, '127.0.0.1');

const SIZES = [5_000, 100_000];
const EDITORS = ['rustaco', 'monaco'];

function stats(xs) {
  const s = [...xs].sort((a, b) => a - b);
  const q = (p) => s[Math.min(s.length - 1, Math.floor(p * s.length))];
  return { median: +q(0.5).toFixed(2), p95: +q(0.95).toFixed(2), mean: +(s.reduce((a, b) => a + b, 0) / s.length).toFixed(2) };
}

const browser = await chromium.launch();
const report = { browser: browser.version(), results: [] };
for (const lines of SIZES) {
  for (const name of EDITORS) {
    const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
    await page.goto(`http://127.0.0.1:47900/${name}.html`);
    await page.waitForFunction(() => window.benchReady, null, { timeout: 60_000 });
    const cdp = await page.context().newCDPSession(page);
    await cdp.send('Performance.enable');
    const busy = async () => {
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
    // Main-thread work per keystroke: all tasks between the first key and a settled page,
    // including background work the key starts (tokenizing, rendering), over 60 keys.
    const before = await busy();
    for (let i = 0; i < 60; i++) {
      await page.keyboard.press('x');
      await page.waitForTimeout(40);
    }
    await page.waitForTimeout(1500);
    const idle = await busy();
    await page.waitForTimeout(1500);
    const idleBaseline = (await busy()) - idle;
    const perKey = (idle - before - idleBaseline * (60 * 40 + 1500) / 1500) / 60;
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
    report.results.push({ editor: name, lines, chars: await page.evaluate(() => window.textLength), loadMs: +load.toFixed(1), typed, mainThreadMsPerKey: +perKey.toFixed(2), keyToFrameEnd: stats(keys), scroll: stats(scroll) });
    await page.close();
  }
}
await browser.close();
server.close();
console.log(JSON.stringify(report, null, 2));
