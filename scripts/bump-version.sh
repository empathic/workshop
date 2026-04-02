#!/usr/bin/env bash
#
# Bump the workshop version interactively.
#
# Reads the current version from Cargo.toml, prompts for bump type
# (major/minor/patch) or a custom version, then updates Cargo.toml.
# Bazel reads the version from Cargo.toml at repo-setup time via
# the cargo_version module extension — no manual sync needed.
#
# Usage:
#   scripts/bump-version.sh
#
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CARGO_TOML="$REPO_ROOT/packages/workshop/Cargo.toml"

# Parse current version from Cargo.toml
CURRENT=$(grep '^version = ' "$CARGO_TOML" | head -1 | sed 's/version = "\(.*\)"/\1/')
if [[ -z "$CURRENT" ]]; then
  echo "Error: could not read version from $CARGO_TOML"
  exit 1
fi

IFS='.' read -r MAJOR MINOR PATCH <<< "$CURRENT"

NEXT_MAJOR="$((MAJOR + 1)).0.0"
NEXT_MINOR="${MAJOR}.$((MINOR + 1)).0"
NEXT_PATCH="${MAJOR}.${MINOR}.$((PATCH + 1))"

echo "Current version: $CURRENT"
echo ""
echo "  1) patch  → $NEXT_PATCH"
echo "  2) minor  → $NEXT_MINOR"
echo "  3) major  → $NEXT_MAJOR"
echo "  4) custom"
echo ""
read -rp "Bump type [1-4]: " CHOICE

case "$CHOICE" in
  1|patch) NEW_VERSION="$NEXT_PATCH" ;;
  2|minor) NEW_VERSION="$NEXT_MINOR" ;;
  3|major) NEW_VERSION="$NEXT_MAJOR" ;;
  4|custom)
    read -rp "Enter version: " NEW_VERSION
    if [[ ! "$NEW_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.]+)?$ ]]; then
      echo "Error: invalid semver: $NEW_VERSION"
      exit 1
    fi
    ;;
  *) echo "Error: invalid choice"; exit 1 ;;
esac

echo ""
echo "Bumping $CURRENT → $NEW_VERSION"
echo ""

# Update Cargo.toml (Bazel picks this up automatically)
sed -i.bak "s/^version = \"$CURRENT\"/version = \"$NEW_VERSION\"/" "$CARGO_TOML"
rm -f "$CARGO_TOML.bak"

echo "Updated: $CARGO_TOML"
echo ""
echo "Run 'git diff' to review, then commit when ready."
