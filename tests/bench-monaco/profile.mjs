// CPU profile of typing in one editor: prints the functions with the most self time.
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { extname, join, normalize } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const root = fileURLToPath(new URL('.', import.meta.url));
const types = { '.html': 'text/html', '.js': 'text/javascript', '.wasm': 'application/wasm', '.css': 'text/css' };
const server = createServer(async (req, res) => {
  const file = join(root, normalize(decodeURIComponent(new URL(req.url, 'http://x').pathname)).replace(/^[/\\]+/, ''));
  const body = await readFile(file).catch(() => null);
  if (body) res.writeHead(200, { 'content-type': types[extname(file)] || 'application/octet-stream' }).end(body); else res.writeHead(404).end();
}).listen(47901, '127.0.0.1');

const name = process.argv[2] || 'rustaco';
const lines = Number(process.argv[3] || 100000);
const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
await page.goto(`http://127.0.0.1:47901/${name}.html`);
await page.waitForFunction(() => window.benchReady);
await page.evaluate(async (n) => {
  const parts = [];
  for (let i = 0; i < n; i++) parts.push(i % 20 === 0 ? `fn f${i}() {` : i % 20 === 19 ? '}' : `    let value_${i} = compute(${i}, "text ${i}"); // note ${i}`);
  const text = parts.join('\n');
  window.textLength = text.length;
  await window.bench.load(text);
  window.bench.setCaret(Math.floor(text.length / 2));
}, lines);
await page.waitForTimeout(3000);
const cdp = await page.context().newCDPSession(page);
await cdp.send('Profiler.enable');
await cdp.send('Profiler.setSamplingInterval', { interval: 100 });
await cdp.send('Profiler.start');
for (let i = 0; i < 60; i++) { await page.keyboard.press('x'); await page.waitForTimeout(40); }
await page.waitForTimeout(1000);
const { profile } = await cdp.send('Profiler.stop');
const self = new Map();
const dt = new Map();
for (let i = 0; i < profile.samples.length; i++) dt.set(profile.samples[i], (dt.get(profile.samples[i]) || 0) + (profile.timeDeltas[i] || 0));
for (const node of profile.nodes) {
  const f = node.callFrame;
  const key = `${f.functionName || '(anonymous)'} ${f.url.split('/').pop()}:${f.lineNumber}`;
  self.set(key, (self.get(key) || 0) + (dt.get(node.id) || 0) / 1000);
}
const top = [...self.entries()].filter(([k]) => !k.startsWith('(idle)') && !k.startsWith('(program)')).sort((a, b) => b[1] - a[1]).slice(0, 25);
for (const [k, ms] of top) console.log(`${(ms / 60).toFixed(2)} ms/key  ${k}`);
// Who calls the hottest native functions.
const parent = new Map();
for (const node of profile.nodes) for (const c of node.children ?? []) parent.set(c, node);
for (const want of ['decode', '(garbage collector)', 'fillText']) {
  const callers = new Map();
  for (const node of profile.nodes) {
    if (node.callFrame.functionName !== want) continue;
    let p = parent.get(node.id);
    const chain = [];
    for (let i = 0; i < 4 && p; i++, p = parent.get(p.id)) chain.push(`${p.callFrame.functionName || '(anon)'}:${p.callFrame.lineNumber}`);
    const key = chain.join(' < ');
    callers.set(key, (callers.get(key) || 0) + (dt.get(node.id) || 0) / 1000);
  }
  console.log(`-- ${want}`);
  for (const [k, ms] of [...callers.entries()].sort((a, b) => b[1] - a[1]).slice(0, 4)) console.log(`  ${(ms / 60).toFixed(2)} ms/key  ${k}`);
}
await browser.close();
server.close();
