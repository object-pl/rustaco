import { test, expect } from '@playwright/test';

test('works under a strict Content Security Policy', async ({ page }) => {
  await page.goto('/csp.html');
  await page.waitForFunction(() => window.editor);
  await page.evaluate(() => window.editor.focus());
  await page.keyboard.type('// ok');
  expect(await page.evaluate(() => window.editor.getValue())).toBe('// okfn main() {}\n');
  await page.keyboard.press('Control+f');
  await page.keyboard.type('main');
  await expect(page.locator('[data-count]')).toHaveText('1 of 1');
  expect(await page.evaluate(() => window.violations)).toEqual([]);
});

test('works inside Shadow DOM', async ({ page }) => {
  await page.goto('/shadow.html');
  await page.waitForFunction(() => window.editor);
  const box = await page.locator('#host').boundingBox();
  await page.mouse.click(box.x + 300, box.y + 10);
  await page.keyboard.press('End');
  await page.keyboard.type('!');
  expect(await page.evaluate(() => window.editor.getValue())).toBe('one two one!\n');
  // The textarea inside the shadow root mirrors the line for screen readers.
  const mirror = await page.evaluate(() => {
    const t = document.getElementById('host').shadowRoot.querySelector('textarea');
    return [t.value, t.selectionStart];
  });
  expect(mirror).toEqual(['one two one!', 12]);
  await page.keyboard.press('Control+f');
  await page.keyboard.type('one');
  await expect(page.locator('#host [data-count]')).toHaveText(/of 2/);
});
