# Changelog

## [Unreleased]

### Added
- **Per-session auto-accept policy**: Tiny "Policy" badge in the tab bar (top-left) lets you set, edit, and clear auto-accept policies per session. Changes take effect mid-session via file-based policy (`~/.claude/auto-accept-policies/{session_id}`), no restart needed. Requires the [claude-auto-accept](https://github.com/MjMoshiri/claude-auto-accept) plugin.
- **Auto-update support**: Added Tauri updater plugin. Check for updates in Settings > Updates. Pulls from GitHub Releases (requires release infrastructure setup: signing key + CI publishing `latest.json`).

### Fixed
- **Double session on profile launch**: Rapid Enter key or double-click could spawn two sessions from the same profile. Added ref-based guard to prevent concurrent launches in both the Quick Launcher and Profiles panel.
