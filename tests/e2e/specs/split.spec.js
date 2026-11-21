import { test, expect } from '@playwright/test';

test.beforeEach(async ({ page }) => {
  await page.goto('/split.html');
  await page.waitForFunction(() => window.left && window.right);
  await page.evaluate(() => window.model.setValue('alpha\nbeta\ngamma'));
});

test('typing in one editor shows in the other and moves its cursor', async ({ page }) => {
  await page.evaluate(() => {
    window.right.setSelections([{ anchor: 11, head: 11 }]); // start of "gamma"
    window.left.setSelections([{ anchor: 0, head: 0 }]);
    window.left.focus();
  });
  await page.keyboard.type('>> ');
  expect(await page.evaluate(() => window.right.getValue())).toBe('>> alpha\nbeta\ngamma');
  expect(await page.evaluate(() => window.right.getSelections())).toEqual([{ anchor: 14, head: 14 }]);
  expect(await page.evaluate(() => window.left.getSelections())).toEqual([{ anchor: 3, head: 3 }]);
});

test('each editor keeps its own selection', async ({ page }) => {
  await page.evaluate(() => {
    window.left.setSelections([{ anchor: 0, head: 5 }]);
    window.right.setSelections([{ anchor: 6, head: 10 }]);
  });
  expect(await page.evaluate(() => window.left.getSelections())).toEqual([{ anchor: 0, head: 5 }]);
  expect(await page.evaluate(() => window.right.getSelections())).toEqual([{ anchor: 6, head: 10 }]);
});

test('undo in the other editor reverts the change, and both listeners see one model', async ({ page }) => {
  await page.evaluate(() => {
    window.changes = 0;
    window.model.onDidChangeContent(() => window.changes++);
    window.left.setSelections([{ anchor: 5, head: 5 }]);
    window.left.focus();
  });
  await page.keyboard.type('!');
  await page.evaluate(() => window.right.focus());
  await page.keyboard.press(process.platform === 'darwin' ? 'Meta+z' : 'Control+z');
  expect(await page.evaluate(() => window.left.getValue())).toBe('alpha\nbeta\ngamma');
  expect(await page.evaluate(() => window.changes)).toBe(2);
});
