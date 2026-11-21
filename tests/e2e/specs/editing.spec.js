import { test, expect } from '@playwright/test';

const mod = process.platform === 'darwin' ? 'Meta' : 'Control';

async function open(page, value) {
  await page.goto('/');
  await page.waitForFunction(() => window.editor);
  await page.evaluate((v) => { window.editor.setValue(v); window.editor.focus(); }, value);
}

const value = (page) => page.evaluate(() => window.editor.getValue());
const selections = (page) => page.evaluate(() => window.editor.getSelections());
const setCursor = (page, ...offsets) =>
  page.evaluate((o) => window.editor.setSelections(o.map((p) => ({ anchor: p, head: p }))), offsets);

/** Canvas-relative point of the caret at `offset`, taken from the editor's own layout. */
async function caretPoint(page, offset) {
  return page.evaluate((o) => {
    const e = window.editor;
    e.setSelections([{ anchor: o, head: o }]);
    e.core.render(true);
    return { x: e.core.caret_x() + 1, y: e.core.caret_y() + e.core.line_height() / 2 };
  }, offset);
}

test('typing, then undo removes the typed word in one step', async ({ page }) => {
  await open(page, '');
  await page.keyboard.type('hello');
  await page.keyboard.press('Enter');
  await page.keyboard.type('world');
  expect(await value(page)).toBe('hello\nworld');
  await page.keyboard.press(`${mod}+z`);
  expect(await value(page)).toBe('hello\n');
  await page.keyboard.press(`${mod}+z`);
  await page.keyboard.press(`${mod}+z`);
  expect(await value(page)).toBe('');
  await page.keyboard.press(`${mod}+Shift+z`);
  expect(await value(page)).toBe('hello');
});

test('Enter keeps the indentation of the current line', async ({ page }) => {
  await open(page, '    let x = 1;');
  await page.keyboard.press('End');
  await page.keyboard.press('Enter');
  await page.keyboard.type('y');
  expect(await value(page)).toBe('    let x = 1;\n    y');
});

test('typing with several cursors', async ({ page }) => {
  await open(page, 'a\nb\nc');
  await setCursor(page, 1, 3, 5);
  await page.keyboard.type('!');
  expect(await value(page)).toBe('a!\nb!\nc!');
  expect((await selections(page)).length).toBe(3);
});

test('keyboard adds a cursor below', async ({ page }) => {
  await open(page, 'abc\nabc');
  await setCursor(page, 1);
  await page.keyboard.press(`${mod}+Alt+ArrowDown`);
  await page.keyboard.type('X');
  expect(await value(page)).toBe('aXbc\naXbc');
});

test('word navigation and deletion', async ({ page }) => {
  await open(page, 'let value = compute();');
  await setCursor(page, 0);
  await page.keyboard.press(`${process.platform === 'darwin' ? 'Alt' : 'Control'}+ArrowRight`);
  await page.keyboard.press(`${process.platform === 'darwin' ? 'Alt' : 'Control'}+ArrowRight`);
  expect((await selections(page))[0].head).toBe(9);
  await page.keyboard.press(`${process.platform === 'darwin' ? 'Alt' : 'Control'}+Backspace`);
  expect(await value(page)).toBe('let  = compute();');
});

test('click places the caret, double-click selects a word, shift-click extends', async ({ page }) => {
  await open(page, 'alpha beta gamma\nsecond line');
  const canvas = page.locator('canvas');
  const at7 = await caretPoint(page, 7);
  const at2 = await caretPoint(page, 2);
  const at20 = await caretPoint(page, 20);

  await canvas.click({ position: at7 });
  expect(await selections(page)).toEqual([{ anchor: 7, head: 7 }]);

  await canvas.dblclick({ position: at7 });
  expect(await selections(page)).toEqual([{ anchor: 6, head: 10 }]);

  await canvas.click({ position: at2 });
  await canvas.click({ position: at20, modifiers: ['Shift'] });
  expect(await selections(page)).toEqual([{ anchor: 2, head: 20 }]);
});

test('drag selects text', async ({ page }) => {
  await open(page, 'alpha beta gamma');
  const box = await page.locator('canvas').boundingBox();
  const from = await caretPoint(page, 1);
  const to = await caretPoint(page, 9);
  await page.mouse.move(box.x + from.x, box.y + from.y);
  await page.mouse.down();
  await page.mouse.move(box.x + to.x, box.y + to.y, { steps: 5 });
  await page.mouse.up();
  expect(await selections(page)).toEqual([{ anchor: 1, head: 9 }]);
});

test('clipboard events copy, cut and paste through the editor', async ({ page, browserName }) => {
  // Firefox keeps the DataTransfer of an untrusted ClipboardEvent read-only, and real Ctrl+C/V
  // there would touch the system clipboard of the machine running the tests.
  test.skip(browserName === 'firefox', 'synthetic clipboard events are read-only in Firefox');
  await open(page, 'one two');
  await page.evaluate(() => window.editor.setSelections([{ anchor: 0, head: 3 }]));
  const copied = await page.evaluate(() => {
    const dt = new DataTransfer();
    window.editor.input.dispatchEvent(new ClipboardEvent('copy', { clipboardData: dt, bubbles: true, cancelable: true }));
    return dt.getData('text/plain');
  });
  expect(copied).toBe('one');

  await page.evaluate(() => {
    const dt = new DataTransfer();
    window.editor.input.dispatchEvent(new ClipboardEvent('cut', { clipboardData: dt, bubbles: true, cancelable: true }));
  });
  expect(await value(page)).toBe(' two');

  await setCursor(page, 4);
  await page.evaluate(() => {
    const dt = new DataTransfer();
    dt.setData('text/plain', '!');
    window.editor.input.dispatchEvent(new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true }));
  });
  expect(await value(page)).toBe(' two!');
});

test('change listener fires once per edit', async ({ page }) => {
  await open(page, '');
  await page.evaluate(() => { window.changes = 0; window.editor.onDidChangeContent(() => window.changes++); });
  await page.keyboard.type('abc');
  await page.keyboard.press('ArrowLeft');
  expect(await page.evaluate(() => window.changes)).toBe(3);
});

test('a keystroke in a 200k-line file stays fast', async ({ page }) => {
  await open(page, '');
  const ms = await page.evaluate(() => {
    const e = window.editor;
    e.setValue(Array.from({ length: 200_000 }, (_, i) => `line ${i} with some text`).join('\n'));
    e.setSelections([{ anchor: 2_000_000, head: 2_000_000 }]);
    const t = performance.now();
    for (let i = 0; i < 100; i++) { e.core.insert_text('x'); e.core.render(true); }
    return (performance.now() - t) / 100;
  });
  expect(ms).toBeLessThan(5);
});

test('keymap commands: comments, line moves, auto-closing, Ctrl+D', async ({ page }) => {
  await open(page, 'foo bar\nfoo baz');
  await page.evaluate(async () => { await window.editor.getModel().setLanguage('rust'); window.editor.focus(); });
  await setCursor(page, 0);
  await page.keyboard.press(`${mod}+/`);
  expect(await value(page)).toBe('// foo bar\nfoo baz');
  await page.keyboard.press(`${mod}+/`);
  await page.keyboard.press('Alt+ArrowDown');
  expect(await value(page)).toBe('foo baz\nfoo bar');
  await setCursor(page, 0);
  await page.keyboard.press(`${mod}+d`);
  await page.keyboard.press(`${mod}+d`);
  await page.keyboard.type('qux');
  expect(await value(page)).toBe('qux baz\nqux bar');
  await page.keyboard.press('End');
  await page.keyboard.type('(');
  expect(await value(page)).toBe('qux baz()\nqux bar()');
  await page.keyboard.type(')');
  expect(await value(page)).toBe('qux baz()\nqux bar()');
});

test('chords and custom keybindings', async ({ page }) => {
  await open(page, 'a  \nb\t');
  await page.keyboard.press(`${mod}+k`);
  await page.keyboard.press(`${mod}+x`);
  expect(await value(page)).toBe('a\nb');
  await page.evaluate(() => window.editor.addKeybindings([{ key: 'ctrl+shift+u', command: 'editor.action.transformToUppercase' }]));
  await page.evaluate(() => window.editor.setSelections([{ anchor: 0, head: 3 }]));
  await page.keyboard.press('Control+Shift+u');
  expect(await value(page)).toBe('A\nB');
});

test('find highlights matches, F3 moves between them, replace works with regex groups', async ({ page }) => {
  await open(page, 'let a = 1;\nlet b = 22;\nconst c = 333;');
  expect(await page.evaluate(() => window.editor.find('let'))).toBe(2);
  await setCursor(page, 0);
  await page.keyboard.press('F3');
  expect(await selections(page)).toEqual([{ anchor: 0, head: 3 }], 'the match at the cursor comes first');
  await page.keyboard.press('F3');
  expect(await selections(page)).toEqual([{ anchor: 11, head: 14 }]);
  await page.keyboard.press('F3');
  expect(await selections(page)).toEqual([{ anchor: 0, head: 3 }], 'wraps around');
  await page.keyboard.press('Shift+F3');
  expect(await selections(page)).toEqual([{ anchor: 11, head: 14 }]);

  expect(await page.evaluate(() => window.editor.find('(\\w) = (\\d+)', { regex: true }))).toBe(3);
  expect(await page.evaluate(() => window.editor.replaceAll('$2 = $1'))).toBe(3);
  expect(await value(page)).toBe('let 1 = a;\nlet 22 = b;\nconst 333 = c;');
  await page.keyboard.press(`${mod}+z`);
  expect(await value(page)).toBe('let a = 1;\nlet b = 22;\nconst c = 333;', 'replace all is one undo step');

  expect(await page.evaluate(() => window.editor.find('(', { regex: true }))).toBe(-1);
  expect(await page.evaluate(() => { window.editor.find('let'); return window.editor.selectAllMatches(); })).toBe(2);
  expect((await selections(page)).length).toBe(2);
});

test('shift+alt drag selects a column, ctrl+u undoes cursor changes', async ({ page }) => {
  await open(page, 'abcdef\nab\nabcdef');
  const box = await page.locator('canvas').boundingBox();
  const from = await caretPoint(page, 1);
  const to = await caretPoint(page, 14); // third line, column 4
  await page.keyboard.down('Shift');
  await page.keyboard.down('Alt');
  await page.mouse.move(box.x + from.x, box.y + from.y);
  await page.mouse.down();
  await page.mouse.move(box.x + to.x, box.y + to.y, { steps: 5 });
  await page.mouse.up();
  await page.keyboard.up('Alt');
  await page.keyboard.up('Shift');
  const sels = await selections(page);
  expect(sels.length).toBe(3);
  expect(sels[0]).toEqual({ anchor: 11, head: 14 }, 'the primary selection is on the head line');
  expect(sels).toContainEqual({ anchor: 8, head: 9 }, 'the short line is clipped at its end');
  await page.keyboard.type('X');
  expect(await value(page)).toBe('aXef\naX\naXef');

  await setCursor(page, 0);
  await page.keyboard.press(`${mod}+d`);
  await page.keyboard.press(`${mod}+u`);
  expect(await selections(page)).toEqual([{ anchor: 0, head: 0 }]);
});

test('folding hides lines, carets step over folds, the gutter marker unfolds', async ({ page }) => {
  await open(page, 'fn a() {\n    one\n    two\n}\nlast');
  await page.waitForTimeout(400); // folding ranges are computed once typing pauses
  await setCursor(page, 12);
  if (mod === 'Control') await page.keyboard.press('Control+Shift+BracketLeft');
  else await page.evaluate(() => window.editor.runCommand('editor.fold'));
  expect(await page.evaluate(() => Array.from(window.editor.core.visibleLines()))).toEqual([0, 1, 3, 5]);
  expect((await selections(page))[0].head).toBe(8);
  await page.keyboard.press('ArrowDown');
  const line3 = 'fn a() {\n    one\n    two\n'.length;
  expect((await selections(page))[0].head).toBe(line3 + 1);

  const marker = await page.evaluate(() => {
    const e = window.editor;
    e.setSelections([{ anchor: 0, head: 0 }]);
    e.core.render(true);
    const x0 = e.core.caret_x();
    e.setSelections([{ anchor: 1, head: 1 }]);
    e.core.render(true);
    const cw = e.core.caret_x() - x0;
    return { x: x0 - 1.5 * cw, y: e.core.line_height() / 2 };
  });
  const box = await page.locator('canvas').first().boundingBox();
  await page.mouse.click(box.x + marker.x, box.y + marker.y);
  expect(await page.evaluate(() => Array.from(window.editor.core.visibleLines()))).toEqual([0, 5]);
});

test('word wrap shows a long line as rows and moves by rows', async ({ page }) => {
  const line = Array.from({ length: 60 }, (_, i) => `w${i}`).join(' ');
  await open(page, `${line}\nnext`);
  await page.evaluate(() => window.editor.setOptions({ wordWrap: 'on' }));
  await setCursor(page, 0);
  await page.keyboard.press('ArrowDown');
  const head = (await selections(page))[0].head;
  expect(head).toBeGreaterThan(0);
  expect(head).toBeLessThan(line.length);
  const y0 = (await caretPoint(page, 0)).y;
  const y1 = (await caretPoint(page, head)).y;
  expect(y1).toBeGreaterThan(y0);
  await page.evaluate(() => window.editor.setOptions({ wordWrap: 'off' }));
  await setCursor(page, 0);
  await page.keyboard.press('ArrowDown');
  expect((await selections(page))[0].head).toBe(line.length + 1);
});
