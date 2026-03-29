/**
 * Novim Core - TypeScript wrapper for Rust native module
 *
 * Provides a typed interface to the Rust native module exposed via Neon FFI.
 */

import { createRequire } from 'module';
import { fileURLToPath } from 'url';
import { dirname, join } from 'path';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const require = createRequire(import.meta.url);

const nativePath = join(__dirname, '../native/novim_core.node');

interface NovimNative {
  hello(): string;
  echo(input: string): string;
  runTerminal(): void;
  runTerminalWithFile(filePath: string): void;
  runTerminalMode(): void;
  runAttachSession(name: string): void;
  listSessions(): string[];
}

let cached: NovimNative | null = null;

function loadNative(): NovimNative {
  if (cached) return cached;

  try {
    cached = require(nativePath) as NovimNative;
    return cached;
  } catch (error) {
    const err = error as Error;
    throw new Error(
      `Failed to load native module from ${nativePath}.\n` +
      `Run: cargo build --release && pnpm run copy:native\n` +
      `Error: ${err.message}`
    );
  }
}

/**
 * Core API for interacting with the Rust native module.
 */
export class NovimCore {
  private native: NovimNative;

  constructor() {
    this.native = loadNative();
  }

  /** Test the connection to the native module */
  test(): string {
    return this.native.hello();
  }

  /** Echo a message through the native module */
  echo(message: string): string {
    return this.native.echo(message);
  }

  /** Run the terminal UI with an empty buffer */
  runTerminal(): void {
    this.native.runTerminal();
  }

  /** Run the terminal UI with a file loaded */
  runTerminalWithFile(filePath: string): void {
    this.native.runTerminalWithFile(filePath);
  }

  /** Run with a shell terminal pane (like tmux) */
  runTerminalMode(): void {
    this.native.runTerminalMode();
  }

  /** Attach to a saved session */
  attachSession(name: string): void {
    this.native.runAttachSession(name);
  }

  /** List saved sessions */
  listSessions(): string[] {
    return this.native.listSessions();
  }
}
