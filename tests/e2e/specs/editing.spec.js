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
  const canvas = page.locator('canvas').first();
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
  const box = await page.locator('canvas').first().boundingBox();
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

test('a keystroke in a 200k-line file stays fast @perf', async ({ page }) => {
  await open(page, '');
  const ms = await page.evaluate(() => {
    const e = window.editor;
    e.setValue(Array.from({ length: 200_000 }, (_, i) => `line ${i} with some text`).join('\n'));
    e.setSelections([{ anchor: 2_000_000, head: 2_000_000 }]);
    // The best of three rounds: the browsers of the other projects run at the same time.
    let best = Infinity;
    for (let round = 0; round < 3; round++) {
      const t = performance.now();
      for (let i = 0; i < 100; i++) { e.core.insert_text('x'); e.core.render(true); }
      best = Math.min(best, (performance.now() - t) / 100);
    }
    return best;
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
  const box = await page.locator('canvas').first().boundingBox();
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
  await page.evaluate(() => window.editor.refreshFolding()); // otherwise computed once typing pauses
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

test('decorations draw backgrounds, gutter bars and overview marks, and move with edits', async ({ page }) => {
  await open(page, 'alpha beta gamma\nsecond line');
  const px = (x, y) => page.evaluate(([x, y]) => {
    const e = window.editor;
    e.core.render(false);
    const dpr = window.devicePixelRatio || 1;
    const d = e.snapshot().getContext('2d').getImageData(Math.round(x * dpr), Math.round(y * dpr), 1, 1).data;
    return [d[0], d[1], d[2]];
  }, [x, y]);
  const { start, lh } = await page.evaluate(async () => {
    const { defineDecorationType } = await import('/rustaco.js');
    const type = await defineDecorationType({ background: '#ff0000', gutter: '#00ff00', overviewRuler: '#0000ff', underline: 'wavy' });
    const model = window.editor.getModel();
    window.decorationType = type;
    window.decorations = model.createDecorationsCollection([{ start: 6, end: 10, type }]);
    return { start: 6, lh: window.editor.core.line_height() };
  });
  const beta = await caretPoint(page, start + 1);
  expect(await px(beta.x, beta.y - lh / 2 + 3)).toEqual([255, 0, 0]);
  expect(await px(3, lh / 2)).toEqual([0, 255, 0]);
  // The mark sits under the scrollbar thumb at the top; look for it down the strip below.
  const blueRows = await page.evaluate(() => {
    const e = window.editor;
    e.setValue(e.getValue() + '\n'.repeat(300) + 'end');
    window.decorations.set([{ start: e.getValue().length - 3, end: e.getValue().length, type: window.decorationType }]);
    e.core.render(false);
    const dpr = window.devicePixelRatio || 1;
    const x = Math.round((e.canvas.clientWidth - 5) * dpr);
    const d = e.snapshot().getContext('2d').getImageData(x, 0, 1, e.canvas.height).data;
    let n = 0;
    for (let i = 0; i < d.length; i += 4) if (d[i] === 0 && d[i + 1] === 0 && d[i + 2] === 255) n++;
    window.decorations.set([{ start: 6, end: 10, type: window.decorationType }]);
    return n;
  });
  expect(blueRows).toBeGreaterThan(0);
  // Typing before the decoration moves it along.
  await setCursor(page, 0);
  await page.keyboard.type('xx');
  const moved = await caretPoint(page, start + 3);
  expect(await px(moved.x, moved.y - lh / 2 + 3)).toEqual([255, 0, 0]);
  await page.evaluate(() => window.decorations.clear());
  expect(await px(3, lh / 2)).not.toEqual([0, 255, 0]);
});

test('the minimap shows the document and scrolls it on click and drag', async ({ page }) => {
  await open(page, Array.from({ length: 2000 }, (_, i) => `let value${i} = ${i};`).join('\n'));
  await page.evaluate(() => window.editor.setOptions({ minimap: { enabled: true } }));
  const box = await page.locator('canvas').first().boundingBox();
  // A click low on the minimap scrolls down.
  await page.mouse.click(box.x + box.width - 40, box.y + box.height - 10);
  const afterClick = await page.evaluate(() => window.editor.getScrollTop());
  expect(afterClick).toBeGreaterThan(1000);
  // Dragging the slider up scrolls back.
  const before = await page.evaluate(() => window.editor.getScrollTop());
  await page.mouse.move(box.x + box.width - 40, box.y + box.height - 10);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width - 40, box.y + 5, { steps: 5 });
  await page.mouse.up();
  expect(await page.evaluate(() => window.editor.getScrollTop())).toBeLessThan(before);
  await page.evaluate(() => window.editor.setOptions({ minimap: { enabled: false } }));
});

test('sticky scroll pins the enclosing headers; a click on one goes there', async ({ page }) => {
  const body = Array.from({ length: 80 }, (_, i) => `        step${i}();`).join('\n');
  await open(page, `impl Thing {\n    fn run() {\n${body}\n    }\n}`);
  await page.evaluate(() => {
    const e = window.editor;
    e.refreshFolding();
    e.setOptions({ stickyScroll: { enabled: true } });
    e.setScrollTop(40 * e.core.line_height());
    e.core.render(false);
  });
  const box = await page.locator('canvas').first().boundingBox();
  const lh = await page.evaluate(() => window.editor.core.line_height());
  // The second pinned row is `fn run() {` on line 1.
  await page.mouse.click(box.x + 200, box.y + lh * 1.5);
  expect((await selections(page))[0].head).toBe('impl Thing {\n    '.length);
  expect(await page.evaluate(() => window.editor.getScrollTop())).toBe(0);
});

test('view zones open space between lines; content widgets follow their position', async ({ page }) => {
  await open(page, 'one\ntwo\nthree');
  const info = await page.evaluate(() => {
    const e = window.editor;
    const lh = e.core.line_height();
    const zone = document.createElement('div');
    zone.textContent = 'zone';
    e.addViewZone({ afterLine: 0, height: 40, domNode: zone });
    const widget = { domNode: document.createElement('div'), position: 5, preference: 'below' };
    widget.domNode.textContent = 'hover';
    e.addContentWidget(widget);
    e.setSelections([{ anchor: 4, head: 4 }]);
    e.core.render(false);
    window.zoneNode = zone;
    window.widget = widget;
    return { lh, lineTwoY: e.core.caret_y() };
  });
  expect(info.lineTwoY).toBe(info.lh + 40);
  await page.waitForFunction(() => window.zoneNode.style.display !== 'none' && window.widget.domNode.style.top !== '');
  const placed = await page.evaluate(() => ({
    zoneTop: parseFloat(window.zoneNode.style.top),
    widgetTop: parseFloat(window.widget.domNode.style.top),
  }));
  expect(placed.zoneTop).toBe(info.lh);
  expect(placed.widgetTop).toBe(info.lh + 40 + info.lh);
});

test('IME composition shows inline and commits once', async ({ page }) => {
  await open(page, 'ab');
  await setCursor(page, 1);
  await page.evaluate(() => {
    const input = document.querySelector('textarea');
    const fire = (type, data) => input.dispatchEvent(new CompositionEvent(type, { data, bubbles: true }));
    fire('compositionstart', '');
    fire('compositionupdate', 'k');
    fire('compositionupdate', 'ka');
    window.during = window.editor.getValue();
    fire('compositionupdate', 'か');
    fire('compositionend', 'か');
  });
  expect(await page.evaluate(() => window.during)).toBe('akab');
  expect(await value(page)).toBe('aかb');
  await page.keyboard.press(`${mod}+z`);
  expect(await value(page)).toBe('ab');
});

test('the textarea mirrors the caret line for screen readers', async ({ page }) => {
  await open(page, 'first line\nsecond line');
  await setCursor(page, 14);
  await page.keyboard.press('Shift+ArrowRight');
  // The mirror follows a moment after the caret moves.
  await expect.poll(() => page.evaluate(() => {
    const t = document.querySelector('textarea');
    return [t.value, t.selectionStart, t.selectionEnd, t.getAttribute('aria-label')];
  })).toEqual(['second line', 3, 4, 'Editor content']);
  // beforeinput from a virtual keyboard deletes through the editor.
  await page.evaluate(() => document.querySelector('textarea').dispatchEvent(
    new InputEvent('beforeinput', { inputType: 'deleteContentBackward', cancelable: true, bubbles: true })));
  expect(await value(page)).toBe('first line\nsecnd line');
});

test('a multi-cursor copy pastes back one piece per cursor', async ({ page, browserName }) => {
  test.skip(browserName === 'firefox', 'Firefox drops clipboardData from synthetic clipboard events');
  await open(page, 'aa bb\ncc dd\nee ff');
  const result = await page.evaluate(() => {
    const e = window.editor;
    const input = document.querySelector('textarea');
    e.setSelections([{ anchor: 0, head: 2 }, { anchor: 6, head: 8 }]);
    const data = new DataTransfer();
    input.dispatchEvent(new ClipboardEvent('copy', { clipboardData: data, bubbles: true, cancelable: true }));
    const copied = { text: data.getData('text/plain'), html: data.getData('text/html') };
    e.setSelections([{ anchor: 5, head: 5 }, { anchor: 11, head: 11 }]);
    input.dispatchEvent(new ClipboardEvent('paste', { clipboardData: data, bubbles: true, cancelable: true }));
    return { ...copied, value: e.getValue() };
  });
  expect(result.text).toBe('aa\ncc');
  expect(result.html).toContain('<pre');
  expect(result.value).toBe('aa bbaa\ncc ddcc\nee ff');
});

test('scrollbar drag, auto-scroll past the edge, and dragging text to move it', async ({ page }) => {
  await open(page, Array.from({ length: 300 }, (_, i) => `line ${i}`).join('\n'));
  // Leave room below the editor: some browsers deliver no events outside the page.
  await page.evaluate(() => { document.getElementById('root').style.bottom = '200px'; });
  await page.waitForFunction(() => window.editor.canvas.clientHeight < window.innerHeight - 150);
  const box = await page.locator('canvas').first().boundingBox();
  // Drag the scrollbar thumb down.
  await page.mouse.move(box.x + box.width - 5, box.y + 5);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width - 5, box.y + 105, { steps: 4 });
  await page.mouse.up();
  const afterThumb = await page.evaluate(() => window.editor.getScrollTop());
  expect(afterThumb).toBeGreaterThan(500);
  // Selecting past the bottom edge keeps scrolling.
  await page.evaluate(() => window.editor.setScrollTop(0));
  await page.mouse.move(box.x + 100, box.y + 10);
  await page.mouse.down();
  await page.mouse.move(box.x + 100, box.y + box.height + 60, { steps: 3 });
  await page.waitForTimeout(300);
  await page.mouse.up();
  expect(await page.evaluate(() => window.editor.getScrollTop())).toBeGreaterThan(100);

  // Drag "line 1" (selected) to the start of line 3.
  await open(page, 'line 0\nline 1\nline 2\nline 3');
  await page.evaluate(() => window.editor.setSelections([{ anchor: 7, head: 13 }]));
  const from = await caretPoint(page, 9);
  await page.evaluate(() => window.editor.setSelections([{ anchor: 7, head: 13 }]));
  const to = await caretPoint(page, 21);
  await page.evaluate(() => window.editor.setSelections([{ anchor: 7, head: 13 }]));
  await page.mouse.move(box.x + from.x, box.y + from.y);
  await page.mouse.down();
  await page.mouse.move(box.x + to.x, box.y + to.y, { steps: 5 });
  await page.mouse.up();
  expect(await value(page)).toBe('line 0\n\nline 2\nline 1line 3');
});

test('a right click keeps the selection and puts the textarea under the pointer', async ({ page }) => {
  await open(page, 'alpha beta gamma');
  await page.evaluate(() => window.editor.setSelections([{ anchor: 6, head: 10 }]));
  const p = await caretPoint(page, 8);
  await page.evaluate(() => window.editor.setSelections([{ anchor: 6, head: 10 }]));
  const box = await page.locator('canvas').first().boundingBox();
  await page.mouse.click(box.x + p.x, box.y + p.y, { button: 'right' });
  expect(await selections(page)).toEqual([{ anchor: 6, head: 10 }]);
  const textarea = await page.evaluate(() => {
    const t = document.querySelector('textarea');
    return { events: t.style.pointerEvents, selected: t.value.slice(t.selectionStart, t.selectionEnd) };
  });
  expect(textarea.selected).toBe('beta');
  await page.keyboard.press('Escape');
});

test('the find widget searches, counts, steps and replaces', async ({ page }) => {
  await open(page, 'foo bar foo baz foo');
  await page.keyboard.press(`${mod}+f`);
  const widget = page.locator('[role="dialog"][aria-label="Find"]');
  await expect(widget).toBeVisible();
  await page.keyboard.type('foo');
  await expect(widget.locator('[data-count]')).toHaveText('1 of 3');
  await page.keyboard.press('Enter');
  await expect(widget.locator('[data-count]')).toHaveText('2 of 3');
  expect((await selections(page))[0]).toEqual({ anchor: 8, head: 11 });
  await page.keyboard.press('Shift+Enter');
  await expect(widget.locator('[data-count]')).toHaveText('1 of 3');
  await page.keyboard.press('Alt+r');
  await page.locator('[data-field="find"]').fill('ba.');
  await expect(widget.locator('[data-count]')).toHaveText('1 of 2');
  await page.keyboard.press('Escape');
  await expect(widget).toBeHidden();
  // Ctrl+H opens the replace row.
  await page.keyboard.press(mod === 'Meta' ? 'Meta+Alt+f' : 'Control+h');
  await page.locator('[data-field="find"]').fill('foo');
  await page.locator('[data-field="replace"]').fill('X');
  await page.locator('[data-action="replaceAll"]').click();
  expect(await value(page)).toBe('X bar X baz X');
  await expect(widget.locator('[data-count]')).toHaveText('No results');
});

test('setOptions changes the font: cells are measured again and clicks land on the right char', async ({ page }) => {
  await open(page, 'abcdefghij\nsecond line\nthird line\n');
  const before = await page.evaluate(() => ({ cw: window.editor.core.charWidth(), lh: window.editor.core.line_height() }));
  await page.evaluate(() => window.editor.setOptions({ fontFamily: '"Courier New", monospace', fontSize: 24 }));
  await expect.poll(() => page.evaluate(() => window.editor.core.line_height())).toBe(36);
  const after = await page.evaluate(() => window.editor.core.charWidth());
  expect(after).toBeGreaterThan(before.cw);
  // A click in the middle of the sixth cell of line 2 puts the caret after five chars.
  const box = await page.locator('canvas').first().boundingBox();
  const gutter = await page.evaluate(() => { window.editor.setSelections([{ anchor: 0, head: 0 }]); window.editor.core.render(false); return window.editor.core.caret_x(); });
  await page.mouse.click(box.x + gutter + after * 5.5, box.y + 36 * 1.5);
  expect(await selections(page)).toEqual([{ anchor: 16, head: 16 }]);
});
