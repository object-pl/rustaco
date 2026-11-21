// "demo": a toy language implemented only with JS providers, to exercise every provider
// feature of the editor. Its programs look like this:
//
//   fn add(a, b) {
//     let sum = a + b;
//     print(sum);
//   }
//   let total = add(1, 2);
//
// Declarations are `fn NAME(PARAMS) {` and `let NAME =`; everything else is a use.

import { languages } from './rustaco.js';

const { CompletionItemKind, SymbolKind } = languages;
const KEYWORDS = ['fn', 'let', 'print', 'return', 'if', 'else'];

/** Declarations: `{ name, kind, start, end, params?, line }`, `start..end` the name. */
function declarations(model) {
  const text = model.getValue();
  const out = [];
  for (const m of text.matchAll(/\bfn\s+([A-Za-z_]\w*)\s*\(([^)]*)\)/g)) {
    const start = m.index + m[0].indexOf(m[1]);
    out.push({ name: m[1], kind: 'fn', start, end: start + m[1].length, params: m[2].split(',').map((p) => p.trim()).filter(Boolean) });
  }
  for (const m of text.matchAll(/\blet\s+([A-Za-z_]\w*)/g)) {
    const start = m.index + m[0].indexOf(m[1]);
    out.push({ name: m[1], kind: 'let', start, end: start + m[1].length });
  }
  return out.sort((a, b) => a.start - b.start);
}

/** The identifier around `offset`: `{ name, start, end }` or null. */
function wordAt(model, offset) {
  const { line, column } = model.positionAt(offset);
  const text = model.getLineContent(line);
  let a = column;
  let b = column;
  while (a > 0 && /\w/.test(text[a - 1])) a--;
  while (b < text.length && /\w/.test(text[b])) b++;
  if (a === b) return null;
  const base = offset - column;
  return { name: text.slice(a, b), start: base + a, end: base + b };
}

/** Every whole-word use of `name`, as ranges. */
function uses(model, name) {
  const text = model.getValue();
  const out = [];
  for (const m of text.matchAll(new RegExp(`\\b${name}\\b`, 'g'))) out.push({ start: m.index, end: m.index + name.length });
  return out;
}

export function registerDemoLanguage(id = 'demo') {
  const disposables = [
    languages.registerCompletionItemProvider(id, {
      triggerCharacters: ['.'],
      provideCompletionItems(model) {
        const names = declarations(model).map((d) => ({
          label: d.name,
          kind: d.kind === 'fn' ? CompletionItemKind.Function : CompletionItemKind.Variable,
          detail: d.kind === 'fn' ? `fn ${d.name}(${d.params.join(', ')})` : `let ${d.name}`,
        }));
        return [
          ...KEYWORDS.map((k) => ({ label: k, kind: CompletionItemKind.Keyword })),
          { label: 'fn', kind: CompletionItemKind.Snippet, detail: 'function', insertText: 'fn ${1:name}(${2:args}) {\n\t$0\n}', isSnippet: true, sortText: 'zfn' },
          ...names,
        ];
      },
    }),
    languages.registerHoverProvider(id, {
      provideHover(model, offset) {
        const w = wordAt(model, offset);
        const d = w && declarations(model).find((x) => x.name === w.name);
        if (!d) return null;
        const line = model.positionAt(d.start).line + 1;
        const signature = d.kind === 'fn' ? `fn ${d.name}(${d.params.join(', ')})` : `let ${d.name}`;
        return { contents: [`\`\`\`demo\n${signature}\n\`\`\``, `Declared on line ${line}.`], range: { start: w.start, end: w.end } };
      },
    }),
    languages.registerSignatureHelpProvider(id, {
      signatureHelpTriggerCharacters: ['(', ','],
      provideSignatureHelp(model, offset) {
        // The innermost unclosed call before the caret.
        const before = model.getTextRange(Math.max(0, offset - 2000), offset);
        let depth = 0;
        let commas = 0;
        for (let i = before.length - 1; i >= 0; i--) {
          const c = before[i];
          if (c === ')') depth++;
          else if (c === '(' && depth > 0) depth--;
          else if (c === ',' && depth === 0) commas++;
          else if (c === '(' && depth === 0) {
            const name = /([A-Za-z_]\w*)\s*$/.exec(before.slice(0, i))?.[1];
            const d = declarations(model).find((x) => x.kind === 'fn' && x.name === name);
            if (!d) return null;
            return {
              signatures: [{ label: `${d.name}(${d.params.join(', ')})`, parameters: d.params.map((p) => ({ label: p })) }],
              activeSignature: 0,
              activeParameter: Math.min(commas, Math.max(0, d.params.length - 1)),
            };
          }
        }
        return null;
      },
    }),
    languages.registerDefinitionProvider(id, {
      provideDefinition(model, offset) {
        const w = wordAt(model, offset);
        const d = w && declarations(model).find((x) => x.name === w.name);
        return d ? { start: d.start, end: d.end } : null;
      },
    }),
    languages.registerReferenceProvider(id, {
      provideReferences(model, offset) {
        const w = wordAt(model, offset);
        return w ? uses(model, w.name) : null;
      },
    }),
    languages.registerDocumentHighlightProvider(id, {
      provideDocumentHighlights(model, offset) {
        const w = wordAt(model, offset);
        return w && !KEYWORDS.includes(w.name) ? uses(model, w.name) : null;
      },
    }),
    languages.registerRenameProvider(id, {
      resolveRenameLocation(model, offset) {
        const w = wordAt(model, offset);
        if (!w) return { rejectReason: 'Nothing to rename here.' };
        if (KEYWORDS.includes(w.name)) return { rejectReason: `${w.name} is a keyword.` };
        return { range: { start: w.start, end: w.end }, text: w.name };
      },
      provideRenameEdits(model, offset, newName) {
        const w = wordAt(model, offset);
        return w ? uses(model, w.name).map((range) => ({ range, text: newName })) : [];
      },
    }),
    languages.registerDocumentFormattingEditProvider(id, {
      // Trailing spaces go, tabs become four spaces, `x  =  1` becomes `x = 1`.
      provideDocumentFormattingEdits(model) {
        const edits = [];
        for (let line = 0; line < model.getLineCount(); line++) {
          const text = model.getLineContent(line);
          const formatted = text.replace(/\t/g, '    ').replace(/\s*=\s*/g, ' = ').replace(/\s+$/, '');
          if (formatted !== text) {
            const start = model.offsetAt({ line, column: 0 });
            edits.push({ range: { start, end: start + text.length }, text: formatted });
          }
        }
        return edits;
      },
    }),
    languages.registerCodeActionProvider(id, {
      provideCodeActions(model, range) {
        const { line } = model.positionAt(range.start);
        const text = model.getLineContent(line);
        const at = text.indexOf('var ');
        if (at < 0) return [];
        const start = model.offsetAt({ line, column: at });
        return [{ title: 'Change var to let', edits: [{ range: { start, end: start + 3 }, text: 'let' }] }];
      },
    }),
    languages.registerFoldingRangeProvider(id, {
      provideFoldingRanges(model) {
        const out = [];
        const open = [];
        for (let line = 0; line < model.getLineCount(); line++) {
          const text = model.getLineContent(line);
          if (text.trimEnd().endsWith('{')) open.push(line);
          if (text.trim().startsWith('}') && open.length) {
            const start = open.pop();
            if (line - 1 > start) out.push({ start, end: line - 1 });
          }
        }
        return out;
      },
    }),
    languages.registerSelectionRangeProvider(id, {
      // The word, then the line, then the whole text.
      provideSelectionRanges(model, offset) {
        const w = wordAt(model, offset);
        const { line } = model.positionAt(offset);
        const start = model.offsetAt({ line, column: 0 });
        const ranges = [];
        if (w) ranges.push({ start: w.start, end: w.end });
        ranges.push({ start, end: start + model.getLineContent(line).length });
        ranges.push({ start: 0, end: model.getValue().length });
        return ranges;
      },
    }),
    languages.registerDocumentSymbolProvider(id, {
      provideDocumentSymbols(model) {
        const ds = declarations(model);
        const fns = ds.filter((d) => d.kind === 'fn');
        const symbol = (d) => ({
          name: d.name,
          kind: d.kind === 'fn' ? SymbolKind.Function : SymbolKind.Variable,
          range: { start: d.start, end: d.end },
          detail: d.kind,
        });
        // Variables declared inside a function's braces are its children.
        const text = model.getValue();
        const bodyEnd = (f) => {
          let depth = 0;
          for (let i = text.indexOf('{', f.end); i >= 0 && i < text.length; i++) {
            if (text[i] === '{') depth++;
            if (text[i] === '}' && --depth === 0) return i;
          }
          return text.length;
        };
        const roots = [];
        const bodies = fns.map((f) => ({ f, end: bodyEnd(f), symbol: { ...symbol(f), children: [] } }));
        for (const d of ds) {
          if (d.kind === 'fn') { roots.push(bodies.find((b) => b.f === d).symbol); continue; }
          const owner = bodies.find((b) => d.start > b.f.end && d.start < b.end);
          (owner ? owner.symbol.children : roots).push(symbol(d));
        }
        return roots;
      },
    }),
    languages.registerCodeLensProvider(id, {
      provideCodeLenses(model) {
        return declarations(model).filter((d) => d.kind === 'fn').map((d) => {
          const n = uses(model, d.name).length - 1;
          return {
            line: model.positionAt(d.start).line,
            title: `${n} reference${n === 1 ? '' : 's'}`,
            command: (editor) => editor.setSelections([{ anchor: d.start, head: d.end }]),
          };
        });
      },
    }),
    languages.registerDocumentSemanticTokensProvider(id, {
      provideSemanticTokens(model) {
        const ds = declarations(model);
        const tokens = [];
        for (const d of ds) {
          const style = d.kind === 'fn' ? 'function' : 'variable';
          for (const u of uses(model, d.name)) tokens.push({ ...u, style });
          for (const p of d.params ?? []) for (const u of uses(model, p)) tokens.push({ ...u, style: 'variable.parameter' });
        }
        const text = model.getValue();
        for (const k of KEYWORDS) for (const m of text.matchAll(new RegExp(`\\b${k}\\b`, 'g'))) tokens.push({ start: m.index, end: m.index + k.length, style: 'keyword' });
        return tokens;
      },
    }),
    languages.registerInlayHintsProvider(id, {
      // The type of `let x = <number or string literal>`, after the name.
      provideInlayHints(model) {
        const text = model.getValue();
        const out = [];
        for (const m of text.matchAll(/\blet\s+([A-Za-z_]\w*)\s*=\s*("[^"]*"|\d+)/g)) {
          const end = m.index + m[0].indexOf(m[1]) + m[1].length;
          out.push({ offset: end, label: m[2].startsWith('"') ? ': string' : ': number' });
        }
        return out;
      },
    }),
  ];
  return { dispose() { for (const d of disposables) d.dispose(); } };
}
