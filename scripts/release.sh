#!/usr/bin/env bash
set -euo pipefail

# Usage: ./scripts/release.sh [patch|minor|major]
# Builds locally, then tags + publishes to GitHub only on success.

BUMP="${1:-patch}"
TARGET="aarch64-apple-darwin"
REPO="MjMoshiri/Claude-Tab"

# ── Validate ──────────────────────────────────────────────────────────
case "$BUMP" in
  patch|minor|major) ;;
  *) echo "Usage: $0 [patch|minor|major]"; exit 1 ;;
esac

command -v gh >/dev/null || { echo "gh CLI required"; exit 1; }
command -v jq >/dev/null || { echo "jq required"; exit 1; }

# Ensure clean working tree
if ! git diff --quiet || ! git diff --cached --quiet; then
  echo "Error: working tree is dirty. Commit or stash changes first."
  exit 1
fi

# Ensure on main
BRANCH=$(git branch --show-current)
if [ "$BRANCH" != "main" ]; then
  echo "Error: must be on main branch (currently on $BRANCH)"
  exit 1
fi

# ── Compute version ──────────────────────────────────────────────────
CURRENT=$(jq -r .version src-tauri/tauri.conf.json)
IFS='.' read -r major minor patch <<< "$CURRENT"
case "$BUMP" in
  major) major=$((major + 1)); minor=0; patch=0 ;;
  minor) minor=$((minor + 1)); patch=0 ;;
  patch) patch=$((patch + 1)) ;;
esac
NEW="${major}.${minor}.${patch}"
TAG="v$NEW"

echo "==> Releasing $CURRENT -> $NEW ($TAG)"
echo ""

# ── Update version in source files ───────────────────────────────────
jq --arg v "$NEW" '.version = $v' src-tauri/tauri.conf.json > tmp.json && mv tmp.json src-tauri/tauri.conf.json
sed -i '' "s/^version = \"$CURRENT\"/version = \"$NEW\"/" src-tauri/Cargo.toml
jq --arg v "$NEW" '.version = $v' package.json > tmp.json && mv tmp.json package.json

echo "==> Version files updated"

# ── Build ─────────────────────────────────────────────────────────────
echo "==> Installing dependencies..."
npm install

echo "==> Building frontend..."
npm run build

echo "==> Building Tauri app for $TARGET..."
npx tauri build --target "$TARGET"

echo "==> Build succeeded!"

# ── Locate artifacts ──────────────────────────────────────────────────
BUNDLE_DIR="src-tauri/target/$TARGET/release/bundle"
DMG=$(find "$BUNDLE_DIR/dmg" -name '*.dmg' -type f 2>/dev/null | head -1)
TAR=$(find "$BUNDLE_DIR/macos" -name '*.app.tar.gz' ! -name '*.sig' -type f 2>/dev/null | head -1)
SIG=$(find "$BUNDLE_DIR/macos" -name '*.app.tar.gz.sig' -type f 2>/dev/null | head -1)

if [ -z "$TAR" ]; then
  echo "Error: could not find .app.tar.gz artifact"
  git checkout -- src-tauri/tauri.conf.json src-tauri/Cargo.toml package.json
  exit 1
fi

echo "==> Artifacts:"
[ -n "$DMG" ] && echo "    DMG: $DMG"
echo "    TAR: $TAR"
[ -n "$SIG" ] && echo "    SIG: $SIG"

# ── Commit, tag, push ────────────────────────────────────────────────
echo "==> Committing version bump..."
git add src-tauri/tauri.conf.json src-tauri/Cargo.toml package.json
git commit -m "chore: bump version to $NEW"
git tag "$TAG"
git push origin main --tags

echo "==> Tag $TAG pushed"

# ── Create GitHub Release ─────────────────────────────────────────────
UPLOAD_FILES=""
[ -n "$DMG" ] && UPLOAD_FILES="$DMG"
[ -n "$TAR" ] && UPLOAD_FILES="$UPLOAD_FILES $TAR"
[ -n "$SIG" ] && UPLOAD_FILES="$UPLOAD_FILES $SIG"

gh release create "$TAG" \
  --repo "$REPO" \
  --title "Claude Tab $TAG" \
  --notes "See [CHANGELOG.md](https://github.com/$REPO/blob/main/CHANGELOG.md) for details." \
  $UPLOAD_FILES

echo "==> Release $TAG created"

# ── Generate and upload latest.json for auto-updater ──────────────────
if [ -n "$SIG" ] && [ -n "$TAR" ]; then
  SIG_CONTENT=$(cat "$SIG")
  TAR_NAME=$(basename "$TAR")
  TAR_URL="https://github.com/$REPO/releases/download/$TAG/$TAR_NAME"

  cat > /tmp/latest.json <<ENDJSON
{
  "version": "$NEW",
  "notes": "See CHANGELOG.md for details.",
  "pub_date": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "platforms": {
    "darwin-aarch64": {
      "signature": "$SIG_CONTENT",
      "url": "$TAR_URL"
    }
  }
}
ENDJSON

  gh release upload "$TAG" /tmp/latest.json --repo "$REPO" --clobber
  rm -f /tmp/latest.json
  echo "==> latest.json uploaded"
fi

echo ""
echo "Done! https://github.com/$REPO/releases/tag/$TAG"
