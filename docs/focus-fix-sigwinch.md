# Focus fix: cursor mispositioned on tab switch

## Root cause

Two problems combined:

1. **`display: none` zeroes terminal dimensions** — When a tab is inactive, its container has 0x0 dimensions. FitAddon can't compute cell sizes. When the tab becomes active, FitAddon recalculates and changes the terminal size, triggering SIGWINCH. ink (Claude Code's TUI) receives SIGWINCH and re-renders, but its `previousLineCount` is based on the OLD dimensions. The `eraseLines()` call uses stale data, so the cursor ends up at the wrong position.

2. **Focus before fit** — We called `terminal.focus()` (which shows the cursor) BEFORE `fitAddon.fit()` (which corrects dimensions). The cursor rendered at stale/wrong coordinates.

## Research findings

- ink hides the terminal cursor via DECTCEM (`\x1b[?25l`) and renders on the **main buffer** (no alternate screen)
- ink uses relative cursor movements (`cursorUp` / `cursorTo`) after each render frame to position the cursor at the input field via `useCursor`
- ink tracks `previousLineCount` between frames and uses it to erase/rewrite — if terminal dimensions change unexpectedly, this count is wrong
- xterm.js fully supports DECTCEM but `display: none` causes: zero dimensions, browser blur, FitAddon breakage, WebGL context loss
- VS Code avoids `display: none` entirely — uses CSS class toggle that preserves dimensions

## Solution (v2)

### 1. `visibility: hidden` instead of `display: none` (terminal-panel/index.tsx)

Inactive terminals use `visibility: hidden` + `pointer-events: none` instead of `display: none`. This keeps the container in the DOM with real dimensions so FitAddon always works and ink never gets confused by dimension changes on tab switch.

### 2. Correct activation sequence (UnifiedTerminal.tsx)

Follow VS Code's pattern — fit and refresh BEFORE focus:

1. `requestAnimationFrame` — wait for browser to compute layout
2. `fitAddon.fit()` — recalculate terminal dimensions
3. `terminal.refresh(0, rows - 1)` — force full redraw of all visible rows
4. `terminal.clearTextureAtlas()` — fix WebGL glyph corruption
5. `terminal.focus()` — show cursor at the now-correct buffer position

## If it still doesn't work

The issue may be in ink's cursor management itself (not in claude-tabs). Next steps:

1. Add debug logging for cursor state on tab switch:
   - `terminal.buffer.active.cursorX` / `cursorY`
   - `(terminal as any)._core._inputHandler._coreService.isCursorHidden`
2. If cursor IS hidden by DECTCEM but still visible in xterm.js, it's an xterm.js bug with cursor visibility on focus
3. If cursor position in buffer is wrong, the issue is ink's relative cursor math — consider sending `Ctrl+L` (`\x0c`) to the PTY to trigger a full TUI redraw
4. Nuclear option: intercept cursor position from buffer, detect the input prompt pattern, and write cursor-positioning escape codes directly
