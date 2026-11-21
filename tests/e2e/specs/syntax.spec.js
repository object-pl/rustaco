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
}

/** Most common non-background colour around the char at `offset`, as "r,g,b". */
async function colorAt(page, offset) {
  return page.evaluate((o) => {
    const e = window.editor;
    const sels = e.getSelections();
    e.setSelections([{ anchor: 0, head: 0 }]);
    e.getModel().updateHighlighter(1e9);
    const [first, end] = e.core.visibleLines();
    e.core.setHighlights(first, end, e.getModel().highlighter.highlight(first, end, false));
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
      if (lum < 200) continue; // background and current-line band
      // The most covered pixel is the brightest; edges are blended with the background.
      if (lum > best) { best = lum; color = `${d[0]},${d[1]},${d[2]}`; }
    }
    return color;
  }, offset);
}

test('language modules load on demand and colour tokens', async ({ page }) => {
  await open(page, 'fn main() {}\n// note', 'rust');
  const keyword = await colorAt(page, 0);
  const comment = await colorAt(page, 14);
  expect(keyword).not.toBeNull();
  expect(comment).not.toBeNull();
  expect(keyword).not.toBe(comment);
  const loaded = await page.evaluate(() =>
    performance.getEntriesByType('resource').map((r) => r.name).filter((n) => n.includes('/syntax-')));
  expect(loaded.some((n) => n.includes('syntax-rust/'))).toBe(true);
  expect(loaded.some((n) => n.includes('syntax-python/'))).toBe(false);
});

test('highlights follow typing', async ({ page }) => {
  await open(page, 'x', 'rust');
  const plain = await colorAt(page, 0);
  await page.evaluate(() => window.editor.setSelections([{ anchor: 0, head: 0 }]));
  await page.keyboard.type('let ');
  expect(await colorAt(page, 0)).not.toBe(plain);
});

test('the language module keeps the same text through every kind of edit', async ({ page }) => {
  await open(page, 'fn a() {\r\n    1\r\n}\r\n', 'rust');
  await page.evaluate(() => window.editor.setSelections([{ anchor: 3, head: 3 }, { anchor: 14, head: 14 }]));
  await page.keyboard.type('é😀 ');
  await page.keyboard.press('Enter');
  await page.keyboard.press('Backspace');
  await page.keyboard.press(`${mod}+z`);
  await page.keyboard.press(`${mod}+Shift+z`);
  await page.evaluate(() => {
    const dt = new DataTransfer();
    dt.setData('text/plain', 'x\r\ny');
    window.editor.input.dispatchEvent(new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true }));
  });
  await page.keyboard.press(`${mod}+a`);
  await page.keyboard.press('Delete');
  await page.keyboard.type('struct S;');
  const [model, module] = await page.evaluate(() => {
    const m = window.editor.getModel();
    m.syncHighlighter();
    return [m.getValue(), m.highlighter.text()];
  });
  expect(module).toBe(model);
});

test('setValue resets the language module, unknown languages are refused', async ({ page }) => {
  await open(page, 'a', 'json');
  await page.evaluate(() => window.editor.setValue('{"k": 1}'));
  const text = await page.evaluate(() => { const m = window.editor.getModel(); m.syncHighlighter(); return m.highlighter.text(); });
  expect(text).toBe('{"k": 1}');
  expect(await page.evaluate(() => window.editor.getModel().setLanguage('klingon'))).toBe(false);
  expect(await page.evaluate(() => window.editor.getModel().setLanguage(''))).toBe(true);
});

test('highlighting keeps a keystroke fast in a 100k-line file', async ({ page }) => {
  await open(page, '', 'rust');
  const ms = await page.evaluate(async () => {
    const e = window.editor;
    const m = e.getModel();
    e.setValue(Array.from({ length: 100_000 }, (_, i) => `let v${i} = compute(${i}, "text"); // c`).join('\n'));
    await m.setLanguage('rust');
    e.setSelections([{ anchor: 1_500_000, head: 1_500_000 }]);
    const frame = () => {
      m.updateHighlighter(4);
      const [a, b] = e.core.visibleLines();
      e.core.setHighlights(a, b, m.highlighter.highlight(a, b, true));
      e.core.render(true);
    };
    m.updateHighlighter(1e9);
    frame();
    const t = performance.now();
    for (let i = 0; i < 50; i++) { e.core.insert_text('x'); frame(); }
    return (performance.now() - t) / 50;
  });
  console.log(`keystroke with highlighting: ${ms.toFixed(2)} ms`);
  // Parsing gets 4 ms per frame; the rest is edit transfer, highlighting and drawing. One
  // frame at 60 Hz is the bound until parsing moves to a worker (roadmap phase 7).
  expect(ms).toBeLessThan(16);
});

test('a long parse finishes over several frames and then colours the text', async ({ page }) => {
  await open(page, '', '');
  const result = await page.evaluate(async () => {
    const e = window.editor;
    const m = e.getModel();
    e.setValue(Array.from({ length: 50_000 }, (_, i) => `fn f${i}() -> u32 { ${i} }`).join('\n'));
    await m.setLanguage('rust');
    let frames = 0;
    while (!m.updateHighlighter(4)) frames++;
    const quads = m.highlighter.highlight(0, 5, false);
    return { frames, colouredSpans: quads.length / 4 };
  });
  expect(result.frames).toBeGreaterThan(1);
  expect(result.colouredSpans).toBeGreaterThan(5);
});

test('scripts inside HTML are coloured as JavaScript', async ({ page }) => {
  await open(page, '<script>\nconst x = 1;\n</script>', 'html');
  const plainText = '212,212,212';
  const keyword = await colorAt(page, 9);
  expect(keyword).not.toBeNull();
  expect(keyword).not.toBe(plainText);
});

test('SQL, Kotlin and Dockerfile are coloured by the TextMate module', async ({ page }) => {
  const plainText = '212,212,212';
  await open(page, "SELECT name FROM users; -- c", 'sql');
  expect(await colorAt(page, 0)).not.toBe(plainText);
  const loaded = await page.evaluate(() =>
    performance.getEntriesByType('resource').map((r) => r.name).filter((n) => n.includes('/syntax-')));
  expect(loaded.some((n) => n.includes('syntax-textmate/'))).toBe(true);

  for (const [lang, text] of [['kotlin', 'fun main() {}'], ['dockerfile', 'FROM rust:1']]) {
    expect(await page.evaluate(([l, t]) => { window.editor.setValue(t); return window.editor.getModel().setLanguage(l); }, [lang, text])).toBe(true);
    expect(await colorAt(page, 0)).not.toBe(plainText);
  }
});

test('themes change the editor surface and token colours', async ({ page }) => {
  await open(page, 'fn main() {}', 'rust');
  const background = () => page.evaluate(() => {
    const e = window.editor;
    e.core.render(false);
    const d = e.canvas.getContext('2d').getImageData(e.canvas.width - 40, e.canvas.height - 40, 1, 1).data;
    return `${d[0]},${d[1]},${d[2]}`;
  });
  expect(await background()).toBe('30,30,30');
  await page.evaluate(() => window.rustacoSetTheme('light'));
  expect(await background()).toBe('255,255,255');
  await page.evaluate(() => window.rustacoSetTheme('high-contrast'));
  expect(await background()).toBe('0,0,0');
  await page.evaluate(() => window.rustacoSetTheme({
    type: 'dark',
    colors: { 'editor.background': '#102030' },
    tokenColors: [{ scope: 'keyword', settings: { foreground: '#ff0000' } }],
  }));
  expect(await background()).toBe('16,32,48');
  expect(await colorAt(page, 0)).toBe('255,0,0');
  await page.evaluate(() => window.rustacoSetTheme('dark'));
});

test('expand selection follows the syntax tree; no auto-closing inside strings', async ({ page }) => {
  await open(page, 'fn f() { g(1, 2); let s = ""; }', 'rust');
  await page.evaluate(() => { window.editor.getModel().updateHighlighter(1e9); window.editor.setSelections([{ anchor: 11, head: 11 }]); });
  await page.keyboard.press('Shift+Alt+ArrowRight');
  const first = await page.evaluate(() => window.editor.getSelections()[0]);
  expect(first).toEqual({ anchor: 11, head: 12 });
  const selected = () => page.evaluate(() => { const s = window.editor.getSelections()[0]; return window.editor.getValue().slice(s.anchor, s.head); });
  await page.keyboard.press('Shift+Alt+ArrowRight');
  expect(await selected()).toBe('(1, 2)');
  await page.keyboard.press('Shift+Alt+ArrowRight');
  expect(await selected()).toBe('g(1, 2)');
  await page.keyboard.press('Shift+Alt+ArrowLeft');
  await page.keyboard.press('Shift+Alt+ArrowLeft');
  expect(await page.evaluate(() => window.editor.getSelections()[0])).toEqual({ anchor: 11, head: 12 });

  await page.evaluate(() => window.editor.setSelections([{ anchor: 27, head: 27 }]));
  await page.keyboard.type('(');
  expect(await page.evaluate(() => window.editor.getValue())).toContain('let s = "(";');
});

test('matching brackets: jump with the tree and by text, depth colours', async ({ page }) => {
  const jump = `${process.platform === 'darwin' ? 'Meta' : 'Control'}+Shift+Backslash`;
  await open(page, 'fn f() { g(h(1), ")"); }', 'rust');
  await page.evaluate(() => { const e = window.editor; e.getModel().updateHighlighter(1e9); e.setSelections([{ anchor: 10, head: 10 }]); e.core.render(true); });
  await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
  await page.keyboard.press(jump);
  expect(await page.evaluate(() => window.editor.getSelections()[0].head)).toBe(20);
  const styles = await page.evaluate(() => {
    const q = window.editor.getModel().highlighter.highlight(0, 1, true);
    const at = (col) => { for (let i = q.length - 4; i >= 0; i -= 4) if (q[i + 1] === col) return q[i + 3]; return -1; };
    return { outer: at(7), mid: at(10), inner: at(12) };
  });
  expect(styles.outer).not.toBe(styles.mid);
  expect(styles.mid).not.toBe(styles.inner);

  await page.evaluate(async () => { const e = window.editor; e.setValue('(a [b] c)'); await e.getModel().setLanguage(''); e.setSelections([{ anchor: 0, head: 0 }]); });
  await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
  await page.keyboard.press(jump);
  expect(await page.evaluate(() => window.editor.getSelections()[0].head)).toBe(8);
});

test('rulers and indentation guides are drawn', async ({ page }) => {
  await open(page, 'fn f() {\n    x\n}', 'rust');
  const pixel = await page.evaluate(() => {
    const e = window.editor;
    e.setOptions({ rulers: [20] });
    e.core.render(false);
    const dpr = window.devicePixelRatio || 1;
    const gutter = e.core.caret_x(); // caret at offset 0 sits at the start of the text area
    const cw = (() => { e.setSelections([{ anchor: 1, head: 1 }]); e.core.render(false); return e.core.caret_x() - gutter; })();
    const x = Math.floor((gutter + 20 * cw) * dpr);
    const d = e.canvas.getContext('2d').getImageData(x, Math.round(200 * dpr), 1, 1).data;
    return `${d[0]},${d[1]},${d[2]}`;
  });
  expect(pixel).toBe('90,90,90');
});

test('fold regions come from the syntax tree', async ({ page }) => {
  await open(page, 'fn f() {\n    g(\n        1,\n    );\n}\nend', 'rust');
  await page.waitForTimeout(400);
  await page.evaluate(() => { window.editor.setSelections([{ anchor: 12, head: 12 }]); window.editor.runCommand('editor.fold'); });
  // The cursor is on line 1, which heads the call's arguments; only line 2 is hidden.
  expect(await page.evaluate(() => Array.from(window.editor.core.visibleLines()))).toEqual([0, 2, 3, 6]);
});
