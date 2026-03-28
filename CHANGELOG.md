# Changelog

## [1.4.0] - 2026-03-28

### Added
- **Telegram bot remote launcher**: Spawn Claude Tabs sessions from your phone via a per-machine Telegram bot. Pair your device once, then tap a profile to launch a session and get a remote control link back in Telegram.
- **Settings UI for Telegram**: Bot token configuration with BotFather link, device pairing with 6-char verification code, connect/disconnect status.
- **`open_url` Tauri command**: Generic command for opening external URLs from the webview.

## [1.3.1] - 2026-03-25

### Fixed
- **Settings not persisting across restarts**: Config was stored in memory only (`ConfigLayer::Runtime`). Now persists to `~/.claude-tabs/config.toml` via `ConfigLayer::User` and loads on startup.
- **No save feedback in settings UI**: Added Save button to settings panel that writes config to disk with visual confirmation.

## [1.3.0] - 2026-03-25

### Fixed
- **Settings not persisting**: Auto-switch settings now take effect immediately — SessionStateManager reads config from storage on init and listens for changes.
- **Auto-switch popup ignoring disabled setting**: Disabling "Auto-switch to Your Turn tabs" now actually stops the countdown toast from appearing.
- **Profile editor closing on outside click**: Clicking outside the editor (e.g. during window resize) no longer dismisses the profile/pack editor panel.
- **Listener leak in `useSession`**: Tauri event listener now properly cleaned up if component unmounts before async `listen()` resolves.
- **Event listener leak in window-focus extension**: `config-changed` handler is now removed on deactivate instead of accumulating.
- **Listener leak in PolicyBadge**: Tauri listener cleaned up on early unmount; async `invoke` calls guarded against dead state updates.
- **Uncancellable timeout in HistorySection**: Delayed session refresh timeout now cleared on unmount.

## [1.2.0] - 2026-03-25

### Added
- **Off / Policy / Allow All toggle**: Three-state mode selector in the policy badge and context menu popover. Off = normal dialogs, Policy = LLM judge, Allow All = auto-accept everything.
- **Auto-accept policy in profiles**: Profiles can now have a default auto-accept policy that gets applied when launching a session.
- **Right-click "Set Policy"**: Context menu entry on sessions to quickly set or change the auto-accept policy inline.
- **Empty policy file on session create**: When auto-accept is enabled, an empty policy file is always created for new sessions so the hook has a file to read.

### Changed
- Plugin behavior is now allow/ask only — the plugin never denies on its own, it either auto-allows or falls through to the normal permission dialog.

## [1.1.0] - 2026-03-25

### Added
- **Per-session auto-accept policy**: Policy badge in the tab bar lets you set, edit, and clear auto-accept policies per session. Changes take effect mid-session via file-based policy (`~/.claude/auto-accept-policies/{session_id}`).
- **Auto-accept settings**: Enable/disable auto-accept, set default policy, choose judge model, and select mode (permission only vs all tool calls) in Settings.
- **Auto-update support**: Tauri updater plugin with signed releases and `latest.json` for OTA updates.

### Fixed
- **Double session on profile launch**: Rapid Enter key or double-click could spawn two sessions from the same profile.

## [1.0.0] - 2026-03-24

### Added
- Initial release with multi-session tab management, profiles, auto-switch, and Claude Code integration.
