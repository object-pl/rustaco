import { test, expect } from '@playwright/test';

// The language modules run as language servers in the worker (`rustaco-worker.js`), and the
// editor reaches them through LSP: highlighting through `rustaco/highlight`, the rest
// through the standard requests.

async function open(page, value, language) {
  await page.goto('/');
  await page.waitForFunction(() => window.editor?.ready);
  await page.evaluate(async ([v, l]) => {
    const e = window.editor;
    e.setValue(v);
    await e.getModel().setLanguage(l);
    e.focus();
  }, [value, language]);
  expect(await page.evaluate(() => !!window.editor.getModel().syntax?.client)).toBe(true);
}

test('rust: outline, definition, references and rename within the file', async ({ page }) => {
  const src = 'fn main() {\n    let count = 1;\n    count + 1;\n}\nfn other(count: i32) {}\n';
  await open(page, src, 'rust');
  const r = await page.evaluate(async (s) => {
    const model = window.editor.getModel();
    const link = model.syntax;
    const use = s.indexOf('count + 1');
    return {
      symbols: (await link.symbols()).map((x) => x.name),
      definition: await link.client.ask('definition', model, use),
      references: await link.client.ask('references', model, use),
      rename: await link.client.ask('rename', model, use, 0, 'total'),
      badName: await link.client.ask('rename', model, use, 0, 'two words'),
      completion: (await link.client.ask('completion', model, use + 2)).map((i) => i.label),
    };
  }, src);
  const decl = src.indexOf('count');
  expect(r.symbols).toEqual(['main', 'other']);
  expect(r.definition).toEqual([{ start: decl, end: decl + 5 }]);
  // The parameter of `other` is another `count`.
  expect(r.references).toHaveLength(2);
  expect(r.rename.own.map((e) => e.text)).toEqual(['total', 'total']);
  expect(r.badName?.own ?? []).toEqual([]);
  expect(r.completion).toEqual(expect.arrayContaining(['count', 'main', 'let', 'match']));
});

test('rust: F2 renames through the built-in provider', async ({ page }) => {
  await open(page, 'fn f() {\n    let a = 1;\n    a + a;\n}\n', 'rust');
  // The caret at the end of the `a` in `a + a`.
  await page.evaluate(() => window.editor.setSelections([{ anchor: 29, head: 29 }]));
  await page.keyboard.press('F2');
  const box = page.locator('input[aria-label="Rename"]');
  await expect(box).toHaveValue('a');
  await box.fill('b');
  await box.press('Enter');
  await expect.poll(() => page.evaluate(() => window.editor.getValue())).toBe('fn f() {\n    let b = 1;\n    b + b;\n}\n');
});

test('mermaid: colours and completion from its service', async ({ page }) => {
  const src = 'graph LR\n  start[Go] --> check{Ok?}\n  check -->|no| start\n  ';
  await open(page, src, 'mermaid');
  const r = await page.evaluate(async (s) => {
    const model = window.editor.getModel();
    const link = model.syntax;
    const quads = await link.highlight(0, 3);
    const after = s.indexOf('graph ') + 6;
    return {
      tree: link.hasTree,
      styled: quads.length / 4,
      nodes: (await link.client.ask('completion', model, s.length)).map((i) => i.label).slice(0, 2),
      directions: (await link.client.ask('completion', model, after)).map((i) => i.label),
    };
  }, src);
  expect(r.tree).toBe(false);
  expect(r.styled).toBeGreaterThan(8);
  expect(r.nodes).toEqual(['start', 'check']);
  expect(r.directions).toEqual(expect.arrayContaining(['TD', 'LR']));
});
