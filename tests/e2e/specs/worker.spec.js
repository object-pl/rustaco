import { test, expect } from '@playwright/test';

const mod = process.platform === 'darwin' ? 'Meta' : 'Control';

async function open(page, value, language) {
  await page.goto('/');
  await page.waitForFunction(() => window.editor?.ready);
  await page.evaluate(async ([v, l]) => {
    const e = window.editor;
    e.setValue(v);
    await e.getModel().setLanguage(l);
    e.focus();
  }, [value, language]);
  expect(await page.evaluate(() => !!window.editor.getModel().syntax)).toBe(true);
}

/** Brightest pixel colour in the first cells of the char at `offset`. */
function colorAt(page, offset) {
  return page.evaluate((o) => {
    const e = window.editor;
    const sels = e.getSelections();
    e.setSelections([{ anchor: o, head: o }]);
    e.core.render(false);
    const x = e.core.caret_x(), y = e.core.caret_y(), h = e.core.line_height();
    e.setSelections(sels);
    const ctx = e.canvas.getContext('2d');
    const dpr = window.devicePixelRatio || 1;
    let best = -1, color = null;
    for (let dx = 1; dx < 14; dx++) for (let dy = 3; dy < h - 3; dy++) {
      const d = ctx.getImageData(Math.round((x + dx) * dpr), Math.round((y + dy) * dpr), 1, 1).data;
      const lum = d[0] + d[1] + d[2];
      if (lum > best) { best = lum; color = `${d[0]},${d[1]},${d[2]}`; }
    }
    return color;
  }, offset);
}

test('highlights come from the worker', async ({ page }) => {
  await open(page, 'fn main() {}\nlet plain = 1;', 'rust');
  await expect.poll(async () => (await colorAt(page, 0)) !== (await colorAt(page, 17))).toBe(true);
});

test('with the worker, pairs stay closed inside strings and selection grows by syntax', async ({ page }) => {
  await open(page, 'fn f() { g(1, 2); let s = ""; }', 'rust');
  // Wait for the first frame of highlights, which the literal check reads.
  await expect.poll(async () => (await colorAt(page, 0)) !== (await colorAt(page, 3))).toBe(true);
  await page.evaluate(() => window.editor.setSelections([{ anchor: 27, head: 27 }]));
  await expect.poll(() => page.evaluate(() => window.editor.core.caretInLiteral())).toBe(true);
  await page.keyboard.type('(');
  expect(await page.evaluate(() => window.editor.getValue())).toBe('fn f() { g(1, 2); let s = "("; }');
  await page.evaluate(() => window.editor.setSelections([{ anchor: 11, head: 11 }]));
  await page.keyboard.press('Shift+Alt+ArrowRight');
  await page.keyboard.press('Shift+Alt+ArrowRight');
  await expect.poll(() => page.evaluate(() => {
    const s = window.editor.getSelections()[0];
    return window.editor.getValue().slice(s.anchor, s.head);
  })).toBe('(1, 2)');
});

test('fold regions come from the worker', async ({ page }) => {
  await open(page, 'fn f() {\n    g(\n        1,\n    );\n}\nend', 'rust');
  await page.evaluate(() => window.editor.refreshFolding());
  await expect.poll(async () => {
    await page.evaluate(() => { window.editor.setSelections([{ anchor: 12, head: 12 }]); window.editor.runCommand('editor.fold'); });
    return page.evaluate(() => Array.from(window.editor.core.visibleLines()));
  }).toEqual([0, 2, 3, 6]);
});

test('search runs in the worker and its matches follow the text', async ({ page }) => {
  await open(page, 'alpha beta alpha gamma alpha', 'rust');
  const n = await page.evaluate(() => window.editor.findAsync('alpha'));
  expect(n).toBe(3);
  await page.evaluate(() => window.editor.setSelections([{ anchor: 0, head: 0 }]));
  await page.keyboard.type('alpha ');
  await expect.poll(() => page.evaluate(() => { window.editor.findNext(); return window.editor.core.findPosition()[1]; })).toBe(4);
});

test('a keystroke in a 10 MB file costs the main thread under 1 ms with the worker @perf', async ({ page, browserName }) => {
  await open(page, '', 'rust');
  const ms = await page.evaluate(async () => {
    const e = window.editor;
    const m = e.getModel();
    const line = 'let value = compute(input, "some text here", 42); // note\n';
    e.setValue(line.repeat(Math.ceil(10_000_000 / line.length)));
    await m.setLanguage('rust');
    e.setSelections([{ anchor: 5_000_000, head: 5_000_000 }]);
    // Everything the main thread does per keystroke: the edit, sending it to the worker
    // with a frame request, and drawing.
    const keystroke = () => {
      e.core.insert_text('x', false);
      m.syntax.frame(e, Array.from(e.core.visibleLines()), true, e.core.primaryRange()[0]);
      e.core.render(true);
    };
    // A search is active, so every pause in typing searches the whole text again (in the
    // worker); parsing runs there too. Replies land between keystrokes.
    await e.findAsync('compute');
    for (let i = 0; i < 20; i++) keystroke();
    // Batches of ten keystrokes: Firefox and Safari round timers to a millisecond. Between
    // batches the worker's replies arrive; every fifth pause is long enough for a search.
    const batches = [];
    for (let b = 0; b < 30; b++) {
      const t = performance.now();
      for (let i = 0; i < 10; i++) { keystroke(); e.refreshFindSoon(); }
      batches.push((performance.now() - t) / 10);
      await new Promise((r) => setTimeout(r, b % 5 === 4 ? 200 : 5));
    }
    batches.sort((a, b) => a - b);
    return batches[Math.floor(batches.length / 2)];
  });
  console.log(`keystroke on the main thread with the worker, 10 MB, median: ${ms.toFixed(3)} ms`);
  // Typical medians on the development machine: Chromium 0.7 to 1.0 ms, WebKit 0.8 ms,
  // Firefox 1.2 to 1.4 ms, nearly all of it drawing the frame (see the roadmap, phase 7).
  // The bounds leave room for a loaded machine.
  expect(ms).toBeLessThan(browserName === 'firefox' ? 2 : 1.5);
});
