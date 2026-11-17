#!/usr/bin/env bash
# Generate apple/Aurora.xcodeproj from apple/project.yml with XcodeGen.
set -euo pipefail
APPLE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
XCODEGEN="$(command -v xcodegen || true)"
[[ -z "$XCODEGEN" && -x "$HOME/.local/xcodegen/bin/xcodegen" ]] && XCODEGEN="$HOME/.local/xcodegen/bin/xcodegen"
if [[ -z "$XCODEGEN" ]]; then
  echo "XcodeGen not found. Install it with 'brew install xcodegen'." >&2
  exit 1
fi
# XcodeGen lists source files when it runs, so the generated bindings must exist first
# or the project silently leaves them out (and every core type is "not in scope").
if [[ ! -f "$APPLE_DIR/Shared/Core/Generated/aurora_ffi.swift" ]]; then
  echo "Build the core first: apple/scripts/build-core.sh (it writes the Swift bindings)." >&2
  exit 1
fi
"$XCODEGEN" generate --spec "$APPLE_DIR/project.yml" --project "$APPLE_DIR"
