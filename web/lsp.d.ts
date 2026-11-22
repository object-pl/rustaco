// Type definitions for the browser LSP client (lsp.js).
import type { RustacoModel } from './rustaco';

/** Carries JSON-RPC messages as strings, one per message. */
export interface Transport {
  send(message: string): void;
  onMessage(listener: (message: string) => void): void;
  close?(): void;
}

/** A transport over a WebSocket carrying one JSON-RPC message per text frame. */
export function webSocketTransport(url: string): Transport;

export interface ConnectOptions {
  transport: Transport;
  /** The editor language whose providers the server backs. */
  languageId: string;
  rootUri?: string | null;
  /** Edits for documents no attached model holds, for the host to apply (LSP's own ranges). */
  onApplyEdit?: (uri: string, edits: Array<{ range: unknown; newText: string }>) => void;
}

export class LspClient {
  /** The capabilities the server announced. */
  capabilities: Record<string, unknown>;
  /**
   * Opens `model` on the server as `uri` (in `languageId`, by default the client's) and
   * keeps it in sync; returns a detach function.
   */
  attach(model: RustacoModel, uri: string, languageId?: string): () => void;
  detach(model: RustacoModel): void;
  /** Any request; resolves to the result as the server sent it. */
  request(method: string, params: unknown, token?: unknown): Promise<unknown>;
  notify(method: string, params: unknown): void;
  /** Asks about an attached model by editor feature (`'foldingRange'`, `'documentSymbol'`, ...). */
  ask(kind: string, model: RustacoModel, a?: number, b?: number, text?: string): Promise<unknown>;
  /** Registers providers for `languageId`, limited to the capability names in `only`. */
  registerProviders(languageId?: string, only?: Set<string> | null): void;
  /** Called for notifications the client does not handle itself. */
  onNotification: ((method: string, params: unknown) => void) | null;
  /** Rejects every waiting request. */
  abort(reason?: string): void;
  /** `shutdown`, `exit`, and the providers go away. */
  dispose(): Promise<void>;
}

/** Initializes a server and registers editor providers for its capabilities. */
export function connectLanguageServer(options: ConnectOptions): Promise<LspClient>;

/** `{ line, character }` (UTF-16) of a char offset, and back. */
export function toPosition(model: RustacoModel, offset: number): { line: number; character: number };
export function toOffset(model: RustacoModel, position: { line: number; character: number }): number;
