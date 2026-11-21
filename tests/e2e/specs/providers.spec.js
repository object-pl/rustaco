import { test, expect } from '@playwright/test';

async function open(page, value, language = 'demo') {
  await page.goto('/');
  await page.waitForFunction(() => window.editor?.ready);
  await page.evaluate(async ([v, l]) => {
    const e = window.editor;
    e.setValue(v);
    await e.getModel().setLanguage(l);
    e.focus();
  }, [value, language]);
}

const value = (page) => page.evaluate(() => window.editor.getValue());

test('completion from a JS provider: fuzzy filter, docs, snippet with tab stops', async ({ page }) => {
  await open(page, '');
  await page.evaluate(async () => {
    const { languages, CompletionItemKind } = await import('/rustaco.js');
    window.disposeCompletion = languages.registerCompletionItemProvider('demo', {
      provideCompletionItems: () => [
        { label: 'forEach', kind: CompletionItemKind.Method, detail: 'array method', documentation: 'Calls a function for each item.' },
        { label: 'format', kind: CompletionItemKind.Function },
        { label: 'for loop', kind: CompletionItemKind.Snippet, insertText: 'for (let ${1:i} = 0; $1 < ${2:n}; $1++) {\n\t$0\n}', isSnippet: true, filterText: 'for' },
      ],
    });
  });
  await page.keyboard.type('fE');
  const list = page.locator('[role="listbox"][aria-label="Suggestions"]');
  await expect(list).toBeVisible();
  await expect(list.locator('[role="option"]')).toHaveCount(1);
  await expect(list).toContainText('Calls a function for each item.');
  await page.keyboard.press('Enter');
  expect(await value(page)).toBe('forEach');
  await expect(list).toBeHidden();

  await page.evaluate(() => window.editor.setValue(''));
  await page.keyboard.type('for');
  await expect(list.locator('[role="option"]').first()).toContainText('for loop');
  await page.keyboard.press('Enter');
  expect(await value(page)).toBe('for (let i = 0; i < n; i++) {\n    \n}');
  await page.keyboard.type('k');
  expect(await value(page)).toBe('for (let k = 0; k < n; k++) {\n    \n}');
  await page.keyboard.press('Tab');
  await page.keyboard.type('10');
  await page.keyboard.press('Tab');
  await page.keyboard.type('body');
  expect(await value(page)).toBe('for (let k = 0; k < 10; k++) {\n    body\n}');
  await page.evaluate(() => window.disposeCompletion.dispose());
});

test('without a provider, completion offers the words of the file', async ({ page }) => {
  await open(page, 'alphabet alpine\n', 'plaintext');
  await page.evaluate(() => window.editor.setSelections([{ anchor: 16, head: 16 }]));
  await page.keyboard.type('alp');
  const options = page.locator('[role="listbox"] [role="option"]');
  await expect(options).toHaveCount(2);
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('Tab');
  expect(await value(page)).toMatch(/^alphabet alpine\nal(phabet|pine)$/);
});

test('choice stops of a snippet show their options', async ({ page }) => {
  await open(page, '');
  await page.evaluate(() => window.editor.insertSnippet('let x = ${1|true,false|};$0'));
  const options = page.locator('[role="listbox"] [role="option"]');
  await expect(options).toHaveCount(2);
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('Enter');
  expect(await value(page)).toBe('let x = false;');
});
