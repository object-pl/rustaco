import { test, expect } from '@playwright/test';

const mod = process.platform === 'darwin' ? 'Meta' : 'Control';

/** An editor attached over WebSocket to tests/e2e/lsp-server.mjs as `languageId`. */
async function open(page, value, languageId) {
  await page.goto('/');
  await page.waitForFunction(() => window.editor?.ready);
  await page.evaluate(async ([v, id]) => {
    const { connectLanguageServer, webSocketTransport } = await import('/lsp.js');
    const e = window.editor;
    e.setValue(v);
    await e.getModel().setLanguage(id);
    window.lsp = await connectLanguageServer({ transport: webSocketTransport('ws://127.0.0.1:47817'), languageId: id });
    window.lsp.attach(e.getModel(), `file:///${id}-${Math.random().toString(36).slice(2)}.txt`);
    e.focus();
  }, [value, languageId]);
}

const value = (page) => page.evaluate(() => window.editor.getValue());

test('a server over WebSocket: sync, diagnostics, hover, completion, definition, rename', async ({ page }, info) => {
  // Each browser gets its own language id, so their providers and documents stay apart.
  const id = `lsp-${info.project.name}`;
  await open(page, 'alpha beta\nalpha TODO\n', id);

  // Diagnostics arrive as squiggles; their message joins the hover.
  await expect.poll(() => page.evaluate(() => window.lsp.docs.get(window.editor.getModel()).diagnostics?.length)).toBe(1);

  // Edits reach the server incrementally: a new TODO is reported.
  await page.evaluate(() => window.editor.setSelections([{ anchor: 0, head: 0 }]));
  await page.keyboard.type('TODO ');
  await expect.poll(() => page.evaluate(() => window.lsp.docs.get(window.editor.getModel()).diagnostics?.length)).toBe(2);

  await page.evaluate(() => window.editor.setSelections([{ anchor: 6, head: 6 }]));
  await page.keyboard.press(`${mod}+k`);
  await page.keyboard.press(`${mod}+i`);
  await expect(page.locator('[role="tooltip"][aria-label="Hover"]')).toContainText('alpha has 5 letters');

  await page.evaluate(() => window.editor.setSelections([{ anchor: 0, head: 0 }]));
  await page.keyboard.press('Escape');
  await page.keyboard.press(`${mod}+End`);
  await page.keyboard.type('serv');
  const list = page.locator('[role="listbox"][aria-label="Suggestions"]');
  await expect(list).toContainText('serverItem');
  await expect(list).toContainText('From the server.');
  await page.keyboard.press('Enter');
  await expect.poll(() => value(page)).toBe('TODO alpha beta\nalpha TODO\nserverItem()');

  // Definition: the first `alpha`; rename changes every one.
  const second = 'TODO alpha beta\n'.length + 2;
  await page.evaluate((o) => window.editor.setSelections([{ anchor: o, head: o }]), second);
  await page.keyboard.press('F12');
  await expect.poll(() => page.evaluate(() => window.editor.getSelections()[0])).toEqual({ anchor: 5, head: 10 });
  await page.keyboard.press('F2');
  const box = page.locator('input[aria-label="Rename"]');
  await box.fill('gamma');
  await box.press('Enter');
  await expect.poll(() => value(page)).toBe('TODO gamma beta\ngamma TODO\nserverItem()');
  await page.evaluate(() => window.lsp.dispose());
});
