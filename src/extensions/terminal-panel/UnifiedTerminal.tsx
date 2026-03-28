/**
 * TerminalInstance — Manages one xterm.js terminal connected to a PTY.
 *
 * This is an imperative class (not a React component) that owns the terminal
 * lifecycle. The terminal's DOM element can be attached to and detached from
 * any container without losing state — the xterm buffer preserves all content
 * regardless of DOM attachment. PTY output continues to be buffered while
 * detached and is visible immediately on reattach + refresh.
 */

import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { PtyOutputEvent } from "../../types/events";

export interface KeybindingHandler {
  eventToKeyString(e: KeyboardEvent): string;
  hasBinding(key: string): boolean;
}

export class TerminalInstance {
  readonly terminal: Terminal;
  readonly fitAddon: FitAddon;
  readonly element: HTMLDivElement;
  private unlisteners: Array<() => void> = [];
  private disposed = false;
  private webglAddon: WebglAddon | null = null;

  constructor(
    private sessionId: string,
    private keybindingHandler: KeybindingHandler,
  ) {
    // Wrapper element — reparented between containers on tab switch
    this.element = document.createElement("div");
    this.element.style.cssText =
      "width:100%;height:100%;overflow:hidden;background:var(--terminal-bg,#1e1e1e);";

    // Read theme from CSS custom properties
    const cs = getComputedStyle(document.documentElement);
    const css = (prop: string, fb: string) =>
      cs.getPropertyValue(prop).trim() || fb;

    this.terminal = new Terminal({
      cursorBlink: true,
      cursorStyle: "bar",
      disableStdin: false,
      fontSize: 14,
      fontFamily:
        "SF Mono, JetBrains Mono, Menlo, Monaco, Consolas, monospace",
      theme: {
        background: css("--terminal-bg", "#1e1e1e"),
        foreground: css("--terminal-fg", "#e5e5e5"),
        cursor: css("--terminal-cursor", "#e5e5e5"),
        selectionBackground: css(
          "--terminal-selection",
          "rgba(255,255,255,0.15)",
        ),
      },
      allowProposedApi: true,
      scrollback: 10000,
    });

    this.fitAddon = new FitAddon();
    this.terminal.loadAddon(this.fitAddon);
    this.terminal.loadAddon(new WebLinksAddon());

    // Open into wrapper — may be detached from DOM, buffer still works
    this.terminal.open(this.element);
    this.loadWebGL();

    // Intercept app keybindings before xterm processes them
    this.terminal.attachCustomKeyEventHandler((e: KeyboardEvent) => {
      if (e.key === "Escape" && e.type === "keydown") {
        invoke("set_session_state", {
          sessionId,
          newState: "paused",
        }).catch(() => {});
      }
      return !this.keybindingHandler.hasBinding(
        this.keybindingHandler.eventToKeyString(e),
      );
    });

    // Keyboard input → PTY
    // Suppress focus-in/out escape sequences briefly after mount
    const mountTime = Date.now();
    this.terminal.onData((data) => {
      if (
        Date.now() - mountTime < 500 &&
        (data === "\x1b[I" || data === "\x1b[O")
      )
        return;
      this.sendToPty(data);
    });

    // PTY output → terminal buffer
    this.setupPtyListeners();
  }

  private loadWebGL() {
    try {
      this.webglAddon = new WebglAddon();
      this.webglAddon.onContextLoss(() => {
        this.webglAddon?.dispose();
        this.webglAddon = null;
      });
      this.terminal.loadAddon(this.webglAddon);
    } catch {
      this.webglAddon = null;
    }
  }

  private async sendToPty(data: string) {
    try {
      const bytes = Array.from(new TextEncoder().encode(data));
      await invoke("write_to_pty", { sessionId: this.sessionId, data: bytes });
    } catch (err) {
      console.error("Failed to write to PTY:", err);
    }
  }

  private async setupPtyListeners() {
    const decode = (b64: string): Uint8Array => {
      const bin = atob(b64);
      const out = new Uint8Array(bin.length);
      for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
      return out;
    };

    const u1 = await listen<PtyOutputEvent>("pty-output", (e) => {
      if (this.disposed) return;
      if (e.payload.session_id === this.sessionId) {
        this.terminal.write(decode(e.payload.data));
      }
    });
    if (this.disposed) {
      u1();
      return;
    }
    this.unlisteners.push(u1);

    const u2 = await listen<{ session_id: string }>("pty-exit", (e) => {
      if (this.disposed) return;
      if (e.payload.session_id === this.sessionId) {
        this.terminal.writeln("\r\n[Process exited]");
      }
    });
    if (this.disposed) {
      u2();
      return;
    }
    this.unlisteners.push(u2);
  }

  /** Fit terminal to its current container and sync PTY dimensions. */
  fit() {
    if (this.disposed) return;

    const prevRows = this.terminal.rows;
    const prevCols = this.terminal.cols;

    this.fitAddon.fit();

    // Only resize PTY if dimensions actually changed — avoids unnecessary SIGWINCH
    if (this.terminal.rows !== prevRows || this.terminal.cols !== prevCols) {
      invoke("resize_pty", {
        sessionId: this.sessionId,
        rows: this.terminal.rows,
        cols: this.terminal.cols,
      }).catch(console.error);
    }
  }

  /** Attach to a visible container, fit to its dimensions, and focus. */
  activate(container: HTMLDivElement) {
    if (this.disposed) return;

    // Move element into the visible container
    container.appendChild(this.element);

    // Wait one frame for layout, then fit and focus
    requestAnimationFrame(() => {
      if (this.disposed) return;

      this.fit();

      // Reload WebGL if context was lost while detached
      if (!this.webglAddon) this.loadWebGL();
      if (typeof (this.terminal as any).clearTextureAtlas === "function") {
        (this.terminal as any).clearTextureAtlas();
      }

      // Redraw all visible rows from buffer
      this.terminal.refresh(0, this.terminal.rows - 1);

      // Focus unless a modal/overlay currently has focus
      const active = document.activeElement;
      const overlayFocused = active?.closest(
        "[role='dialog'], .command-palette-backdrop, .settings-backdrop, .profiles-backdrop",
      );
      if (!overlayFocused) {
        this.terminal.focus();
      }
    });
  }

  dispose() {
    this.disposed = true;
    this.unlisteners.forEach((u) => u());
    this.unlisteners = [];
    this.webglAddon?.dispose();
    this.terminal.dispose();
    this.element.remove();
  }
}
