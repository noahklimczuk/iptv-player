#!/usr/bin/env bash
# Build Aurora's shared Rust core for Apple devices: AuroraCore.xcframework plus the
# Swift bindings that call it (docs/APPLE_PORT_AND_SYNC.md §2.5).
#
#   apple/scripts/build-core.sh                  release build, every slice
#   apple/scripts/build-core.sh --debug          faster, unoptimised
#   apple/scripts/build-core.sh --ios            iOS slices only (skip tvOS)
#   apple/scripts/build-core.sh --debug --sim    day-to-day: only this Mac's simulators
#
# --sim builds one slice per platform instead of three (no device builds, and only the
# simulator architecture this Mac runs), so it is the fast loop for working in the
# simulator. A framework built with --sim cannot run on a real iPhone or Apple TV:
# build without it for that.
#
# Output, both git-ignored and rebuilt from source:
#   apple/Frameworks/AuroraCore.xcframework
#   apple/Shared/Core/Generated/aurora_ffi.swift
#
# Only touches apple/. The Windows workspace in src-native/ is read (the shared crates
# are path dependencies) and never written: this workspace has its own target dir.
set -euo pipefail

APPLE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CORE_DIR="$APPLE_DIR/core"
OUT_FRAMEWORK="$APPLE_DIR/Frameworks/AuroraCore.xcframework"
OUT_SWIFT="$APPLE_DIR/Shared/Core/Generated"
TARGET_DIR="$CORE_DIR/target"
LIB=libaurora_ffi.a

PROFILE=release
CARGO_PROFILE_FLAG=(--release)
WANT_TVOS=1
SIM_ONLY=0
for arg in "$@"; do
  case "$arg" in
    --debug) PROFILE=debug; CARGO_PROFILE_FLAG=() ;;
    --ios) WANT_TVOS=0 ;;
    --sim) SIM_ONLY=1 ;;
    -h|--help) sed -n '2,20p' "$0"; exit 0 ;;
    *) echo "unknown option: $arg" >&2; exit 2 ;;
  esac
done

export PATH="$HOME/.cargo/bin:$PATH"
# The app's minimum OS (project.yml). rustc and the C code in dependencies (ring,
# SQLite) read these, so the core doesn't silently require a newer OS than the app.
export IPHONEOS_DEPLOYMENT_TARGET=26.0
export TVOS_DEPLOYMENT_TARGET=26.0
command -v cargo >/dev/null || { echo "cargo not found: install Rust with rustup" >&2; exit 1; }
command -v xcodebuild >/dev/null || { echo "xcodebuild not found: install Xcode" >&2; exit 1; }

HOST_ARCH="$(uname -m)"   # arm64 or x86_64

# ── Which Rust targets make up each slice ─────────────────────────────────────
#
# A simulator slice must match the Mac it runs on. Apple-silicon Macs run arm64
# simulators; Intel Macs run x86_64 ones. Both are built where the toolchain allows,
# so the framework works on either kind of Mac.
#
# tvOS: `aarch64-apple-tvos` and `aarch64-apple-tvos-sim` ship with stable Rust.
# `x86_64-apple-tvos` (the tvOS simulator on an Intel Mac) is still Tier 3, so it is
# built with nightly and -Zbuild-std, and only when this Mac needs it.
IOS_DEVICE=(aarch64-apple-ios)
IOS_SIM=(aarch64-apple-ios-sim x86_64-apple-ios)
TVOS_DEVICE=(aarch64-apple-tvos)
TVOS_SIM=(aarch64-apple-tvos-sim)
TVOS_SIM_TIER3=()
if [[ "$HOST_ARCH" == "x86_64" ]]; then
  TVOS_SIM_TIER3=(x86_64-apple-tvos)
fi

if (( SIM_ONLY )); then
  IOS_DEVICE=()
  TVOS_DEVICE=()
  if [[ "$HOST_ARCH" == "x86_64" ]]; then
    IOS_SIM=(x86_64-apple-ios)
    TVOS_SIM=()                      # this Mac's tvOS simulator is the Tier 3 one
  else
    IOS_SIM=(aarch64-apple-ios-sim)
    TVOS_SIM=(aarch64-apple-tvos-sim)
    TVOS_SIM_TIER3=()
  fi
fi

stable_targets=(${IOS_DEVICE[@]+"${IOS_DEVICE[@]}"} "${IOS_SIM[@]}")
if (( WANT_TVOS )); then
  stable_targets+=(${TVOS_DEVICE[@]+"${TVOS_DEVICE[@]}"} ${TVOS_SIM[@]+"${TVOS_SIM[@]}"})
fi

echo "==> Rust targets"
rustup target add "${stable_targets[@]}" >/dev/null

# Only the static library, per Apple target. The crate's manifest declares no
# staticlib or cdylib, so `cargo test` and the Mac build link neither; each is asked
# for here, where it is needed.
build() {   # build <target>
  local target="$1"
  echo "==> aurora-ffi for $target ($PROFILE)"
  ( cd "$CORE_DIR" && cargo rustc -p aurora-ffi --lib --crate-type staticlib \
      ${CARGO_PROFILE_FLAG[@]+"${CARGO_PROFILE_FLAG[@]}"} --target "$target" )
}

for t in "${stable_targets[@]}"; do build "$t"; done

if (( WANT_TVOS )) && (( ${#TVOS_SIM_TIER3[@]} )); then
  rustup toolchain install nightly --profile minimal --component rust-src >/dev/null
  for t in "${TVOS_SIM_TIER3[@]}"; do
    echo "==> aurora-ffi for $t ($PROFILE, nightly -Zbuild-std: Tier 3)"
    ( cd "$CORE_DIR" && cargo +nightly rustc -Zbuild-std -p aurora-ffi --lib --crate-type staticlib \
        ${CARGO_PROFILE_FLAG[@]+"${CARGO_PROFILE_FLAG[@]}"} --target "$t" )
  done
fi

lib_for() { echo "$TARGET_DIR/$1/$PROFILE/$LIB"; }

# One fat library per simulator platform: lipo the archs together.
fat() {   # fat <name> <target>...
  local name="$1"; shift
  local out="$TARGET_DIR/universal/$PROFILE/$name/$LIB"
  mkdir -p "$(dirname "$out")"
  local inputs=()
  for t in "$@"; do inputs+=("$(lib_for "$t")"); done
  lipo -create "${inputs[@]}" -output "$out"
  echo "$out"
}

# ── Swift bindings ────────────────────────────────────────────────────────────
#
# Read out of a Mac build of the same crate, so the interface is exactly the one the
# static libraries export.
echo "==> Swift bindings"
( cd "$CORE_DIR" && cargo rustc -p aurora-ffi --lib --crate-type cdylib \
    ${CARGO_PROFILE_FLAG[@]+"${CARGO_PROFILE_FLAG[@]}"} )
HEADERS="$TARGET_DIR/bindings/$PROFILE"
rm -rf "$HEADERS" && mkdir -p "$HEADERS"
( cd "$CORE_DIR" && cargo run -q -p uniffi-bindgen -- generate \
    --library "$TARGET_DIR/$PROFILE/libaurora_ffi.dylib" \
    --language swift --out-dir "$HEADERS" )

mkdir -p "$OUT_SWIFT"
mv "$HEADERS"/*.swift "$OUT_SWIFT/"
# An XCFramework's headers directory wants the module map under this exact name.
mv "$HEADERS"/*.modulemap "$HEADERS/module.modulemap"

# ── XCFramework ───────────────────────────────────────────────────────────────
echo "==> AuroraCore.xcframework"
args=()
if (( ${#IOS_DEVICE[@]} )); then
  args+=(-library "$(lib_for "${IOS_DEVICE[0]}")" -headers "$HEADERS")
fi
args+=(-library "$(fat ios-simulator "${IOS_SIM[@]}")" -headers "$HEADERS")
if (( WANT_TVOS )); then
  if (( ${#TVOS_DEVICE[@]} )); then
    args+=(-library "$(lib_for "${TVOS_DEVICE[0]}")" -headers "$HEADERS")
  fi
  args+=(-library "$(fat tvos-simulator ${TVOS_SIM[@]+"${TVOS_SIM[@]}"} ${TVOS_SIM_TIER3[@]+"${TVOS_SIM_TIER3[@]}"})" -headers "$HEADERS")
fi
rm -rf "$OUT_FRAMEWORK"
mkdir -p "$(dirname "$OUT_FRAMEWORK")"
xcodebuild -create-xcframework "${args[@]}" -output "$OUT_FRAMEWORK" >/dev/null

echo "==> Done"
echo "    $OUT_FRAMEWORK"
echo "    $OUT_SWIFT/aurora_ffi.swift"
