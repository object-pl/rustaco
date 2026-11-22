import { test, expect } from '@playwright/test';

async function open(page, original, modified, inline = false) {
  await page.goto('/diff.html');
  await page.waitForFunction(() => window.diff);
  await page.evaluate(async ([a, b, inl]) => {
    const { createDiffEditor } = await import('/diff.js');
    window.diff.dispose();
    window.diff = await createDiffEditor(document.getElementById('root'), { original: a, modified: b, inline: inl });
  }, [original, modified, inline]);
}

const ORIGINAL = 'a\nb\nc\nd\ne\n';
const MODIFIED = 'a\nB\nc\nx\ny\nd\ne\n';

test('side by side: changes, fillers that align the sides, synchronised scrolling', async ({ page }) => {
  await open(page, ORIGINAL, MODIFIED);
  const changes = await page.evaluate(() => window.diff.changes.map((c) => [c.original, c.modified]));
  expect(changes).toEqual([
    [{ start: 1, end: 2 }, { start: 1, end: 2 }],
    [{ start: 3, end: 3 }, { start: 3, end: 5 }],
  ]);
  // `d` sits at the same height on both sides thanks to the filler on the left.
  const tops = await page.evaluate(() => {
    const d = window.diff;
    const y = (e, offset) => { e.core.render(false); return e.core.positionOf(offset)[1]; };
    return [y(d.originalEditor, 'a\nb\nc\n'.length), y(d.modifiedEditor, 'a\nB\nc\nx\ny\n'.length)];
  });
  expect(tops[0]).toBe(tops[1]);
  // The changed char inside line 2 is marked on both sides.
  expect(await page.evaluate(() => window.diff.changes[0].inner)).toEqual([{ original: { start: 2, end: 3 }, modified: { start: 2, end: 3 } }]);

  await page.evaluate(() => window.diff.modifiedEditor.setScrollTop(30));
  await expect.poll(() => page.evaluate(() => window.diff.originalEditor.getScrollTop())).toBe(
    await page.evaluate(() => window.diff.modifiedEditor.getScrollTop()));
});

test('edits recompute the diff; navigation and reverting a change', async ({ page }) => {
  await open(page, ORIGINAL, MODIFIED);
  await page.evaluate(() => window.diff.revertChange(0));
  await expect.poll(() => page.evaluate(() => window.diff.changes.length)).toBe(1);
  expect(await page.evaluate(() => window.diff.modifiedModel.getValue())).toBe('a\nb\nc\nx\ny\nd\ne\n');
  await page.evaluate(() => window.diff.goToChange(true));
  const head = await page.evaluate(() => window.diff.modifiedEditor.getSelections()[0].head);
  expect(head).toBe('a\nb\nc\n'.length);
  await page.evaluate(() => window.diff.revertChange(0));
  await expect.poll(() => page.evaluate(() => window.diff.changes.length)).toBe(0);
  expect(await page.evaluate(() => window.diff.modifiedModel.getValue())).toBe(ORIGINAL);
});

test('inline view shows deleted lines between the lines', async ({ page }) => {
  await open(page, 'keep\nold one\nold two\nend\n', 'keep\nnew\nend\n', true);
  const deleted = page.locator('[role="region"][aria-label="Deleted lines"]');
  await expect(deleted).toHaveText('old oneold two');
  expect(await page.evaluate(() => window.diff.originalEditor)).toBeUndefined();
});

test('unchanged stretches collapse', async ({ page }) => {
  const lines = Array.from({ length: 40 }, (_, i) => `line ${i}`);
  const changed = lines.map((l, i) => (i === 20 ? 'changed' : l));
  await open(page, `${lines.join('\n')}\n`, `${changed.join('\n')}\n`);
  await page.evaluate(() => window.diff.collapseUnchanged(3));
  const visible = await page.evaluate(() => Array.from(window.diff.modifiedEditor.core.visibleLines()));
  // Line 0 heads the first fold; 17..23 stay around the change; 24 heads the last fold.
  expect(visible).toEqual([0, 1, 17, 25]);
});

test('a diff of two 100k-line files completes in the worker under a second @perf', async ({ page }) => {
  await page.goto('/diff.html');
  await page.waitForFunction(() => window.diff);
  const ms = await page.evaluate(async () => {
    const { createModel } = await import('/rustaco.js');
    const a = Array.from({ length: 100_000 }, (_, i) => `let value_${i} = compute(${i});`).join('\n');
    const b = Array.from({ length: 100_000 }, (_, i) => (i % 100 === 7 ? `let changed_${i} = other(${i});` : `let value_${i} = compute(${i});`)).join('\n');
    const ma = await createModel(a);
    const mb = await createModel(b);
    const la = ma.workerLink();
    const lb = mb.workerLink();
    await la.diff(lb, { budget: 5000 }); // the first call also sends both texts over
    const t = performance.now();
    const r = await la.diff(lb, { budget: 5000 });
    window.bigDiffChanges = r.flat[1];
    return performance.now() - t;
  });
  console.log(`diff of two 100k-line files in the worker: ${ms.toFixed(0)} ms`);
  expect(await page.evaluate(() => window.bigDiffChanges)).toBe(1000);
  expect(ms).toBeLessThan(1000);
});

test('Alt+F5 and Shift+Alt+F5 step through the changes; addCommand binds host commands', async ({ page }) => {
  await open(page, ORIGINAL, MODIFIED);
  const line = () => page.evaluate(() => {
    const e = window.diff.modifiedEditor;
    return window.diff.modifiedModel.positionAt(e.core.selections()[1]).line;
  });
  await page.evaluate(() => {
    const e = window.diff.modifiedEditor;
    e.setSelections([{ anchor: 0, head: 0 }]);
    e.focus();
  });
  await page.keyboard.press('Alt+F5');
  expect(await line()).toBe(1);
  await page.keyboard.press('Alt+F5');
  expect(await line()).toBe(3);
  await page.keyboard.press('Shift+Alt+F5');
  expect(await line()).toBe(1);

  const ran = await page.evaluate(() => {
    const e = window.diff.modifiedEditor;
    let n = 0;
    const d = e.addCommand('host.count', () => { n++; });
    e.runCommand('host.count');
    d.dispose();
    e.runCommand('host.count');
    return n;
  });
  expect(ran).toBe(1);
});
