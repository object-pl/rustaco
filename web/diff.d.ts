// Types for `rustaco/diff`: the diff editor, side by side or inline.
// Only what is declared here is API (see rustaco.d.ts); the `LspClient` constructor and
// anything else lsp.js or diff.js export without a declaration are internal.
import type { EditorOptions, RustacoEditor, RustacoModel, Subscription } from './rustaco';

/** Lines `start` (inclusive) to `end` (exclusive), 0-based. */
export interface LineRange {
  start: number;
  end: number;
}

/** One changed region: its lines on each side, and the changed chars inside them. */
export interface DiffChange {
  original: LineRange;
  modified: LineRange;
  /** Index of the change this block moved from or to, or null. */
  moved: number | null;
  /** Changed char ranges (offsets) inside the lines, when the lines are similar enough. */
  inner: Array<{ original: { start: number; end: number }; modified: { start: number; end: number } }>;
}

export interface DiffEditorOptions extends EditorOptions {
  /** Texts or models to compare. */
  original?: string | RustacoModel;
  modified?: string | RustacoModel;
  language?: string;
  /** One editor with the deleted lines between the lines, instead of two side by side. */
  inline?: boolean;
  ignoreWhitespace?: boolean;
}

/**
 * Creates a diff editor in `container`. Alt+F5 and Shift+Alt+F5 go to the next and
 * previous change.
 */
export function createDiffEditor(container: HTMLElement, options?: DiffEditorOptions): Promise<RustacoDiffEditor>;

export class RustacoDiffEditor {
  readonly originalModel: RustacoModel;
  readonly modifiedModel: RustacoModel;
  /** The left editor; absent in the inline view. */
  readonly originalEditor?: RustacoEditor;
  readonly modifiedEditor: RustacoEditor;
  /** The changes of the last diff. */
  readonly changes: DiffChange[];
  /** Diffs now (it also runs on its own a moment after each edit). */
  computeDiff(): Promise<DiffChange[]>;
  onDidUpdateDiff(fn: (changes: DiffChange[]) => void): Subscription;
  /** Moves the modified editor's caret to the next (or previous) change; false without changes. */
  goToChange(forward?: boolean): boolean;
  /** Puts the original text of change `index` back into the modified model. */
  revertChange(index: number): void;
  /** Folds unchanged stretches longer than `2 * context` lines on both sides. */
  collapseUnchanged(context?: number): void;
  dispose(): void;
}
