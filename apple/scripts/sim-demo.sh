#!/usr/bin/env bash
# Build the app for the simulator and launch it against the test Xtream panel.
#
#   apple/scripts/sim-demo.sh            Apple TV simulator
#   apple/scripts/sim-demo.sh ios        iPhone simulator
#   apple/scripts/sim-demo.sh tv --play  ...and play the first channel
#   apple/scripts/sim-demo.sh tv --fresh ...from an empty library (reinstalls the app)
#   apple/scripts/sim-demo.sh tv --vod   ...importing films and series too (added to a
#                                         library that has only live TV)
#   apple/scripts/sim-demo.sh tv --tab guide  ...opening on a tab (live, guide, films,
#                                             series, settings, search)
#
# The panel's address and login come from ~/.config/aurora-tv/test-provider.env (or
# $AURORA_TEST_PROVIDER_FILE), which is never committed: AURORA_TEST_XTREAM_URL, _USER
# and _PASS. They reach the app as launch arguments, which only debug builds read.
# Needs a core built for the simulator first: apple/scripts/build-core.sh --debug --sim
set -euo pipefail

APPLE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ENV_FILE="${AURORA_TEST_PROVIDER_FILE:-$HOME/.config/aurora-tv/test-provider.env}"
[[ -r "$ENV_FILE" ]] || { echo "No test panel: create $ENV_FILE (see this script's header)." >&2; exit 1; }
# shellcheck disable=SC1090
source "$ENV_FILE"

PLATFORM=tv; PLAY=NO; FRESH=0; TAB=live; CONTENT=live
while (( $# )); do
  case "$1" in
    tv|ios) PLATFORM=$1 ;;
    --play) PLAY=YES ;;
    --fresh) FRESH=1 ;;
    --vod) CONTENT=all ;;
    --tab) TAB=${2:?--tab needs a name}; shift ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
  shift
done

if [[ $PLATFORM == tv ]]; then
  SCHEME=AuroraTV; BUNDLE=io.github.noahklimczuk.aurora; SDK=appletvsimulator
  DEST='generic/platform=tvOS Simulator'; RUNTIME='tvOS 26'; KIND='Apple TV'
else
  SCHEME=AuroraMobile; BUNDLE=io.github.noahklimczuk.aurora; SDK=iphonesimulator
  DEST='generic/platform=iOS Simulator'; RUNTIME='iOS 26'; KIND='iPhone'
fi

DERIVED="$APPLE_DIR/.build/DerivedData"
"$APPLE_DIR/scripts/generate-project.sh" >/dev/null
echo "==> Building $SCHEME"
xcodebuild -project "$APPLE_DIR/Aurora.xcodeproj" -scheme "$SCHEME" -destination "$DEST" \
  ARCHS="$(uname -m)" -derivedDataPath "$DERIVED" \
  CODE_SIGN_STYLE=Manual CODE_SIGN_IDENTITY=- DEVELOPMENT_TEAM= build -quiet
# Signed "to run locally" (ad hoc), not unsigned: an unsigned app has no
# application-identifier entitlement, and the simulator's Keychain then refuses to store
# the provider login (errSecMissingEntitlement), so an Xtream import fails at once.

# One simulator at a time: two at once can swamp a 16 GB Mac.
DEVICE=$(xcrun simctl list devices "$RUNTIME" available | grep -m1 "$KIND" | grep -oE '[0-9A-F-]{36}')
[[ -n "$DEVICE" ]] || { echo "No $KIND simulator for $RUNTIME: add one in Xcode." >&2; exit 1; }
echo "==> $KIND simulator $DEVICE"
xcrun simctl boot "$DEVICE" 2>/dev/null || true
xcrun simctl bootstatus "$DEVICE" -b >/dev/null
open -a Simulator
(( FRESH )) && xcrun simctl uninstall "$DEVICE" "$BUNDLE" 2>/dev/null || true
xcrun simctl install "$DEVICE" "$DERIVED/Build/Products/Debug-$SDK/$SCHEME.app"
xcrun simctl terminate "$DEVICE" "$BUNDLE" 2>/dev/null || true
xcrun simctl launch "$DEVICE" "$BUNDLE" \
  -AuroraDemoXtreamURL "$AURORA_TEST_XTREAM_URL" \
  -AuroraDemoXtreamUser "$AURORA_TEST_XTREAM_USER" \
  -AuroraDemoXtreamPass "$AURORA_TEST_XTREAM_PASS" \
  -AuroraAutoplay "$PLAY" -AuroraTab "$TAB" -AuroraDemoContent "$CONTENT" >/dev/null
echo "==> Launched. A first run imports the panel's live TV (a few minutes for a big one)."
