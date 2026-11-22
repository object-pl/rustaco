import { test, expect } from '@playwright/test';

const mod = process.platform === 'darwin' ? 'Meta' : 'Control';
const PROGRAM = 'fn add(a, b) {\n    let sum = a + b;\n    print(sum);\n}\nlet total = add(1, 2);\nvar old = 1;\n';

async function open(page, value = PROGRAM) {
  await page.goto('/');
  await page.waitForFunction(() => window.editor?.ready);
  await page.evaluate(async (v) => {
    const { registerDemoLanguage } = await import('/demo-language.js');
    window.demo ??= registerDemoLanguage();
    const e = window.editor;
    e.setValue(v);
    await e.getModel().setLanguage('demo');
    e.focus();
    // Code lenses open space above lines; let them land before measuring positions.
    await e.features.refreshCodeLens();
  }, value);
}

const value = (page) => page.evaluate(() => window.editor.getValue());
const setCursor = (page, offset) => page.evaluate((o) => window.editor.setSelections([{ anchor: o, head: o }]), offset);
const point = (page, offset) => page.evaluate((o) => {
  const e = window.editor;
  const p = e.core.positionOf(o);
  return { x: p[0] + 3, y: p[1] + p[2] / 2 };
}, offset);

test('hover shows the declaration', async ({ page }) => {
  await open(page);
  const at = PROGRAM.indexOf('add(1');
  await page.evaluate(() => window.editor.core.render(false));
  const p = await point(page, at + 1);
  const box = await page.locator('canvas').first().boundingBox();
  await page.mouse.move(box.x + p.x, box.y + p.y);
  const hover = page.locator('[role="tooltip"][aria-label="Hover"]');
  await expect(hover).toContainText('fn add(a, b)');
  await expect(hover).toContainText('Declared on line 1.');
});

test('parameter hints follow the argument', async ({ page }) => {
  await open(page, PROGRAM);
  await setCursor(page, PROGRAM.length);
  await page.keyboard.type('add(');
  const hints = page.locator('[role="tooltip"][aria-label="Parameter hints"]');
  await expect(hints).toContainText('add(a, b)');
  await expect(hints.locator('b')).toHaveText('a');
  await page.keyboard.type('1, ');
  await expect(hints.locator('b')).toHaveText('b');
  await page.keyboard.press('Escape');
  await expect(hints).toBeHidden();
});

test('go to definition with F12 and Ctrl+click; references in a peek view', async ({ page }) => {
  await open(page);
  const use = PROGRAM.indexOf('add(1');
  await setCursor(page, use + 1);
  await page.keyboard.press('F12');
  await expect.poll(() => page.evaluate(() => window.editor.getSelections()[0])).toEqual({ anchor: 3, head: 6 });

  const sumUse = PROGRAM.indexOf('print(sum') + 6;
  const p = await point(page, sumUse + 1);
  const box = await page.locator('canvas').first().boundingBox();
  await page.keyboard.down(mod);
  await page.mouse.click(box.x + p.x, box.y + p.y);
  await page.keyboard.up(mod);
  const sumDecl = PROGRAM.indexOf('sum');
  await expect.poll(() => page.evaluate(() => window.editor.getSelections()[0])).toEqual({ anchor: sumDecl, head: sumDecl + 3 });

  await page.keyboard.press('Shift+F12');
  const peek = page.locator('[role="listbox"][aria-label="References"]');
  await expect(peek).toContainText('2 references');
  await peek.locator('[role="option"]').nth(1).dispatchEvent('mousedown');
  await expect(peek).toBeHidden();
  expect((await page.evaluate(() => window.editor.getSelections()[0])).anchor).toBe(sumUse);
});

test('rename, format and a quick fix edit the text as one step each', async ({ page }) => {
  await open(page, 'let  x  =  1;\t\nprint(x);\nvar y = x;\n');
  await setCursor(page, 6);
  await page.keyboard.press('F2');
  const box = page.locator('input[aria-label="Rename"]');
  await expect(box).toHaveValue('x');
  await box.fill('count');
  await box.press('Enter');
  await expect.poll(() => value(page)).toBe('let  count  =  1;\t\nprint(count);\nvar y = count;\n');
  await page.keyboard.press('Shift+Alt+F');
  await expect.poll(() => value(page)).toBe('let  count = 1;\nprint(count);\nvar y = count;\n');
  await page.keyboard.press(`${mod}+z`);
  expect(await value(page)).toBe('let  count  =  1;\t\nprint(count);\nvar y = count;\n');

  await setCursor(page, (await value(page)).indexOf('var') + 1);
  await page.keyboard.press(`${mod}+.`);
  const menu = page.locator('[role="dialog"][aria-label="Code actions"]');
  await expect(menu).toContainText('Change var to let');
  await page.keyboard.press('Enter');
  await expect.poll(() => value(page)).toContain('let y = count;');
});

test('document highlights, folding ranges and quick outline come from providers', async ({ page }) => {
  await open(page);
  await setCursor(page, PROGRAM.indexOf('sum') + 1);
  // The provider marks both uses of `sum`; the built-in word highlight is off meanwhile.
  await expect.poll(() => page.evaluate(() => window.editor.occurrencesFromProvider)).toBe(true);

  await page.evaluate(() => window.editor.refreshFolding());
  await expect.poll(async () => {
    await page.evaluate(() => { window.editor.setSelections([{ anchor: 2, head: 2 }]); window.editor.runCommand('editor.fold'); });
    return page.evaluate(() => Array.from(window.editor.core.visibleLines()));
  }).toEqual([0, 1, 3, 7]);
  await page.evaluate(() => window.editor.runCommand('editor.unfoldAll'));

  await page.keyboard.press(`${mod}+Shift+o`);
  const outline = page.locator('[role="dialog"][aria-label="Go to symbol"]');
  await expect(outline.locator('[role="option"]')).toHaveCount(3);
  await page.keyboard.type('total');
  await page.keyboard.press('Enter');
  expect((await page.evaluate(() => window.editor.getSelections()[0])).head).toBe(PROGRAM.indexOf('total'));
});

test('code lenses sit above their lines and run their command', async ({ page }) => {
  await open(page);
  await page.evaluate(() => window.editor.features.refreshCodeLens());
  const lens = page.locator('[role="group"][aria-label="Code lens"]');
  await expect(lens).toHaveText('1 reference');
  await lens.locator('span').dispatchEvent('mousedown');
  expect(await page.evaluate(() => window.editor.getSelections()[0])).toEqual({ anchor: 3, head: 6 });
});

test('without a symbol provider the outline comes from the syntax tree', async ({ page }) => {
  await page.goto('/');
  await page.waitForFunction(() => window.editor?.ready);
  const names = await page.evaluate(async () => {
    const e = window.editor;
    e.setValue('struct P;\nimpl P {\n    fn a() {}\n    fn b() {}\n}\nfn main() {}\n');
    await e.getModel().setLanguage('rust');
    const symbols = await e.getDocumentSymbols();
    const walk = (list) => list.flatMap((s) => [s.name, ...walk(s.children)]);
    return walk(symbols);
  });
  expect(names).toEqual(['P', 'P', 'a', 'b', 'main']);
});

test('semantic tokens colour a language without a grammar; inlay hints push text right', async ({ page }) => {
  await open(page);
  const colorAt = (offset) => page.evaluate((o) => {
    const e = window.editor;
    e.core.render(false);
    const p = e.core.positionOf(o);
    const ctx = e.snapshot().getContext('2d');
    const dpr = window.devicePixelRatio || 1;
    let best = -1, color = null;
    for (let dx = 1; dx < 14; dx++) for (let dy = 3; dy < p[2] - 3; dy++) {
      const d = ctx.getImageData(Math.round((p[0] + dx) * dpr), Math.round((p[1] + dy) * dpr), 1, 1).data;
      if (d[0] + d[1] + d[2] > best) { best = d[0] + d[1] + d[2]; color = `${d[0]},${d[1]},${d[2]}`; }
    }
    return color;
  }, offset);
  const plain = await colorAt(PROGRAM.indexOf('old'));
  await page.evaluate(() => window.editor.features.refreshSemanticTokens((name) => window.rustacoStyleId(name)));
  await expect.poll(() => colorAt(0)).not.toBe(plain);

  await page.evaluate(() => window.editor.setValue('let n = 5;'));
  const eq = 'let n '.length;
  const before = await page.evaluate((o) => { window.editor.core.render(false); return window.editor.core.positionOf(o)[0]; }, eq);
  await page.evaluate(() => window.editor.features.refreshInlayHints());
  await expect.poll(() => page.evaluate((o) => { window.editor.core.render(false); return window.editor.core.positionOf(o)[0]; }, eq)).toBeGreaterThan(before);
});

test('provider text never becomes HTML; oversized pastes are refused', async ({ page }) => {
  await page.goto('/');
  await page.waitForFunction(() => window.editor?.ready);
  await page.evaluate(async () => {
    const { languages } = await import('/rustaco.js');
    languages.registerHoverProvider('html-probe', {
      provideHover: () => ({ contents: ['<img src=x onerror="window.injected=1"> **bold** <b>tag</b>'] }),
    });
    const e = window.editor;
    e.setValue('word');
    await e.getModel().setLanguage('html-probe');
    e.focus();
    e.setSelections([{ anchor: 1, head: 1 }]);
    e.runCommand('editor.action.showHover');
  });
  const hover = page.locator('[role="tooltip"][aria-label="Hover"]');
  await expect(hover).toContainText('<img src=x');
  expect(await hover.locator('img').count()).toBe(0);
  expect(await page.evaluate(() => window.injected)).toBeUndefined();

  const refused = await page.evaluate(() => {
    const e = window.editor;
    e.setOptions({ maxPasteLength: 10 });
    let seen = 0;
    e.onDidRefusePaste((n) => { seen = n; });
    const input = document.querySelector('textarea');
    const data = new DataTransfer();
    data.setData('text/plain', 'x'.repeat(11));
    input.dispatchEvent(new ClipboardEvent('paste', { clipboardData: data, bubbles: true, cancelable: true }));
    return { seen, value: e.getValue() };
  });
  // Firefox ignores data in synthetic clipboard events; there nothing is pasted either way.
  if (refused.seen) expect(refused.seen).toBe(11);
  expect(refused.value).toBe('word');
});

test('a newer request cancels the token of the one before; its late answer is dropped', async ({ page }) => {
  await page.goto('/');
  await page.waitForFunction(() => window.editor?.ready);
  const result = await page.evaluate(async () => {
    const { languages } = await import('/rustaco.js');
    const tokens = [];
    languages.registerHoverProvider('slow-probe', {
      provideHover: async (model, offset, token) => {
        tokens.push(token);
        const n = tokens.length;
        await new Promise((r) => setTimeout(r, n === 1 ? 300 : 10));
        return { contents: [`answer ${n}`] };
      },
    });
    const e = window.editor;
    e.setValue('word');
    await e.getModel().setLanguage('slow-probe');
    e.focus();
    e.setSelections([{ anchor: 1, head: 1 }]);
    e.runCommand('editor.action.showHover');
    await new Promise((r) => setTimeout(r, 50));
    e.runCommand('editor.action.showHover');
    await new Promise((r) => setTimeout(r, 500));
    return { first: tokens[0].isCancellationRequested, second: tokens[1].isCancellationRequested };
  });
  expect(result).toEqual({ first: true, second: false });
  await expect(page.locator('[role="tooltip"][aria-label="Hover"]')).toContainText('answer 2');
  await expect(page.locator('[role="tooltip"][aria-label="Hover"]')).not.toContainText('answer 1');
});
