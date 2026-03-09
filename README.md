# Claude Tabs

A tab-based terminal manager for running multiple [Claude Code](https://docs.anthropic.com/en/docs/claude-code) sessions in parallel. Built with Tauri 2, React, and Rust.

## Demo

[Watch the demo](https://github.com/MjMoshiri/Claude-Tab/raw/main/demo.mov)

## Features

- **Parallel Sessions** — Run multiple Claude Code instances side by side (`Cmd+T` for new session)
- **Auto-Focus** — Automatically switches to the session that needs your attention
- **Smart Notifications** — Native macOS notifications when Claude needs your input or permission
- **Session Archive** — Every session is saved to SQLite; search, resume, or fork past conversations
- **Profiles** — Reusable session templates with custom prompts, models, and tools
- **Keyboard-First** — Navigate and manage sessions without the mouse (`Cmd+1-9` for quick switch)
- **Extension System** — Plugin architecture on both frontend (React) and backend (Rust)

## Architecture

```
Frontend (React/TypeScript)          Backend (Rust/Tokio)
┌─────────────────────────┐         ┌─────────────────────────┐
│  Kernel                 │         │  core                   │
│  ├─ ExtensionHost       │         │  ├─ EventBus            │
│  ├─ ComponentRegistry   │   IPC   │  ├─ SessionStore        │
│  ├─ EventBus            │◄───────►│  ├─ StateMachine        │
│  └─ KeybindingManager   │         │  └─ Config              │
├─────────────────────────┤         ├─────────────────────────┤
│  Extensions             │         │  pty (portable-pty)     │
│  ├─ terminal-panel      │         │  storage (SQLite)       │
│  ├─ tab-bar             │         │  tauri-bridge           │
│  ├─ command-palette     │         │  extensions/            │
│  ├─ profiles            │         │  ├─ claude-hooks        │
│  ├─ settings            │         │  ├─ auto-switch         │
│  └─ status-bar          │         │  ├─ output-parser       │
│                         │         │  └─ system-notify       │
└─────────────────────────┘         └─────────────────────────┘
```

## Getting Started

### Prerequisites

- [Node.js](https://nodejs.org/) (v18+)
- [Rust](https://rustup.rs/) (stable)
- [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/)
- [Claude Code](https://docs.anthropic.com/en/docs/claude-code) installed

### Development

```bash
npm install
npm run dev
```

### Build

```bash
npm run build
```

## License

MIT
