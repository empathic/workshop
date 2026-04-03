#!/usr/bin/env bash
#
# Tag a commit as the next release version.
#
# Reads the version from Cargo.toml, confirms the tag name and commit,
# then creates an annotated git tag. Defaults to HEAD of main.
#
# Usage:
#   scripts/tag-release.sh [--alpha N | --suffix SUFFIX] [COMMIT]
#
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CARGO_TOML="$REPO_ROOT/packages/workshop/Cargo.toml"

SUFFIX=""
COMMIT_ARG=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --alpha) SUFFIX="-alpha.${2:?--alpha requires a number}"; shift 2 ;;
    --beta)  SUFFIX="-beta.${2:?--beta requires a number}"; shift 2 ;;
    --rc)    SUFFIX="-rc.${2:?--rc requires a number}"; shift 2 ;;
    --suffix) SUFFIX="-${2:?--suffix requires a value}"; shift 2 ;;
    -h|--help)
      echo "Usage: tag-release.sh [--alpha N | --beta N | --rc N | --suffix SUFFIX] [COMMIT]"
      exit 0
      ;;
    *) COMMIT_ARG="$1"; shift ;;
  esac
done

# Parse version from Cargo.toml
VERSION=$(grep '^version = ' "$CARGO_TOML" | head -1 | sed 's/version = "\(.*\)"/\1/')
if [[ -z "$VERSION" ]]; then
  echo "Error: could not read version from $CARGO_TOML"
  exit 1
fi

# If no suffix was provided via flags, ask interactively
if [[ -z "$SUFFIX" ]]; then
  echo "Version from Cargo.toml: $VERSION"
  echo ""
  echo "  1) release   → v${VERSION}"
  echo "  2) alpha"
  echo "  3) beta"
  echo "  4) rc"
  echo "  5) custom suffix"
  echo ""
  read -rp "Tag type [1-5]: " TAG_CHOICE
  case "$TAG_CHOICE" in
    1|release) ;;
    2|alpha) read -rp "Alpha number: " N; SUFFIX="-alpha.${N}" ;;
    3|beta)  read -rp "Beta number: " N;  SUFFIX="-beta.${N}" ;;
    4|rc)    read -rp "RC number: " N;    SUFFIX="-rc.${N}" ;;
    5|custom) read -rp "Suffix (without leading -): " S; SUFFIX="-${S}" ;;
    *) echo "Error: invalid choice"; exit 1 ;;
  esac
fi

TAG="v${VERSION}${SUFFIX}"

# Default to HEAD of main
COMMIT="${COMMIT_ARG:-}"
if [[ -z "$COMMIT" ]]; then
  MAIN_HEAD=$(git -C "$REPO_ROOT" rev-parse main 2>/dev/null) || {
    echo "Error: could not resolve 'main' branch"
    exit 1
  }
  COMMIT="$MAIN_HEAD"
  COMMIT_LABEL="HEAD of main ($(git -C "$REPO_ROOT" rev-parse --short "$COMMIT"))"
else
  # Resolve to full SHA
  COMMIT=$(git -C "$REPO_ROOT" rev-parse "$COMMIT" 2>/dev/null) || {
    echo "Error: could not resolve commit '$1'"
    exit 1
  }
  COMMIT_LABEL="$(git -C "$REPO_ROOT" rev-parse --short "$COMMIT")"
fi

# Check tag doesn't already exist
if git -C "$REPO_ROOT" rev-parse "$TAG" &>/dev/null; then
  echo "Error: tag $TAG already exists"
  exit 1
fi

# Show what we're about to do
echo "Tag:    $TAG"
echo "Commit: $COMMIT_LABEL"
echo ""
git -C "$REPO_ROOT" log --oneline -1 "$COMMIT"
echo ""
read -rp "Create tag? [y/N] " CONFIRM

case "$CONFIRM" in
  y|Y|yes|YES)
    git -C "$REPO_ROOT" tag -a "$TAG" "$COMMIT" -m "Release $TAG"
    echo ""
    echo "Created tag $TAG"
    echo "Push with: git push origin $TAG"
    ;;
  *)
    echo "Aborted."
    exit 1
    ;;
esac
