// Type definitions for the Rustaco editor's JavaScript API (rustaco.js).
// Offsets are char positions (Unicode scalar values) from the start of the text.

/** A VS Code colour theme: the parsed JSON of a `*-color-theme.json` file. */
export interface VsCodeTheme {
  type?: 'dark' | 'light' | 'hc' | 'hcDark' | 'hcLight';
  colors?: Record<string, string>;
  tokenColors?: Array<{
    scope?: string | string[];
    settings?: { foreground?: string; fontStyle?: string };
  }>;
}

/** Sets the theme of every editor on the page. */
export function setTheme(theme: 'dark' | 'light' | 'high-contrast' | VsCodeTheme): Promise<void>;

/** Creates a text model that several editors can show. `language` is an id such as `"rust"`. */
export function createModel(value?: string, language?: string): Promise<RustacoModel>;

/** Creates an editor in `container`. */
export function createEditor(container: HTMLElement, options?: EditorOptions): Promise<RustacoEditor>;

/** Language id for a file name and its first line, or `""`. */
export function detectLanguage(path: string, firstLine?: string): Promise<string>;

/** Ids of every language the editor knows. */
export function knownLanguages(): Promise<string[]>;

/**
 * Whether models created from now on parse and search in a Web Worker (the default where
 * workers are available) or on the main thread in time slices.
 */
export function setWorkersEnabled(on: boolean): void;

export interface DecorationTypeOptions {
  /** CSS colour behind the decorated text, or behind whole lines with `wholeLine`. */
  background?: string;
  underline?: 'solid' | 'wavy' | 'dotted';
  underlineColor?: string;
  wholeLine?: boolean;
  /** CSS colour of a bar beside the line numbers. */
  gutter?: string;
  /** CSS colour of a mark on the scrollbar. */
  overviewRuler?: string;
}

/** Defines how a kind of decoration draws; the result goes into `Decoration.type`. */
export function defineDecorationType(options?: DecorationTypeOptions): Promise<number>;

export interface Decoration {
  start: number;
  end: number;
  type: number;
}

/** Decorations replaced together. They move with edits and show in every editor of the model. */
export interface DecorationsCollection {
  set(decorations: Decoration[]): void;
  clear(): void;
}

export class RustacoModel {
  getValue(): string;
  setValue(text: string): void;
  /** Sets the language by id (`""` turns highlighting off); loads its module on first use. */
  setLanguage(id: string): Promise<boolean>;
  getLanguage(): string;
  /** Moves this model's language module into the worker or back to the main thread. */
  setWorkerEnabled(on: boolean): Promise<boolean>;
  createDecorationsCollection(decorations?: Decoration[]): DecorationsCollection;
  /** Calls `fn` after every text change; returns a function that removes the listener. */
  onDidChangeContent(fn: () => void): () => void;
}

export interface EditorOptions extends ViewOptions {
  /** Initial text of a new model; ignored with `model`. */
  value?: string;
  language?: string;
  /** Show an existing model, for example in a split view. */
  model?: RustacoModel;
  keymap?: 'default' | 'jetbrains';
  fontFamily?: string;
  fontSize?: number;
  /** Label screen readers announce for the editor. */
  ariaLabel?: string;
}

export interface ViewOptions {
  /** Columns of vertical rulers. */
  rulers?: number[];
  renderWhitespace?: 'none' | 'all';
  /** Indentation guides. */
  guides?: boolean;
  occurrencesHighlight?: boolean;
  bracketPairColorization?: boolean;
  wordWrap?: 'off' | 'on';
  minimap?: { enabled: boolean };
  stickyScroll?: { enabled: boolean; maxLineCount?: number };
}

export interface Selection {
  anchor: number;
  head: number;
}

export interface Keybinding {
  /** Keys as VS Code writes them: `"ctrl+k ctrl+c"`, `"shift+alt+down"`. */
  key: string;
  /** A command id; a leading `-` removes the binding. */
  command: string;
  when?: string;
}

export interface FindOptions {
  regex?: boolean;
  caseSensitive?: boolean;
  wholeWord?: boolean;
}

export interface ViewZone {
  /** 0-based line the zone follows; -1 for above the first line. */
  afterLine: number;
  height: number;
  domNode: HTMLElement;
}

export interface ContentWidget {
  domNode: HTMLElement;
  /** Char offset the widget stays next to. */
  position: number;
  preference?: 'above' | 'below';
}

export interface OverlayWidget {
  domNode: HTMLElement;
  position?: 'top-right' | 'bottom-right' | 'top-center';
}

export class RustacoEditor {
  getModel(): RustacoModel;
  getValue(): string;
  setValue(text: string): void;
  focus(): void;
  setOptions(options: ViewOptions): void;
  setKeymap(preset: 'default' | 'jetbrains'): void;
  addKeybindings(bindings: Keybinding[]): void;
  /** Runs a command by id (`editor.action.commentLine`, `cursorWordLeft`, ...). */
  runCommand(id: string): boolean;

  /** Selections, the primary one first. */
  getSelections(): Selection[];
  setSelections(selections: Selection[]): void;
  getScrollTop(): number;
  setScrollTop(top: number): void;

  /** Highlights every match; returns the count, or -1 for an invalid regular expression. */
  find(pattern: string, options?: FindOptions): number;
  /** Like `find`, with the search in the worker; resolves to the count (-1: invalid pattern). */
  findAsync(pattern: string, options?: FindOptions): Promise<number>;
  findNext(): boolean;
  findPrevious(): boolean;
  /** Replaces the selected match and selects the next. `$1` refers to regex groups. */
  replace(replacement: string): boolean;
  replaceAll(replacement: string): number;
  selectAllMatches(): number;
  clearFind(): void;

  /** Recomputes the foldable regions now instead of after the typing pause. */
  refreshFolding(): void;

  addViewZone(zone: ViewZone): number;
  removeViewZone(id: number): void;
  addContentWidget(widget: ContentWidget): void;
  layoutContentWidget(widget: ContentWidget): void;
  removeContentWidget(widget: ContentWidget): void;
  addOverlayWidget(widget: OverlayWidget): void;
  removeOverlayWidget(widget: OverlayWidget): void;

  /** Calls `fn` after every text change, from this or any other editor on the model. */
  onDidChangeContent(fn: () => void): () => void;
  /** Detaches the editor from its model and removes its DOM. */
  dispose(): void;
}
