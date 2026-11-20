#!/usr/bin/env bash
# Build Aurora for the App Store and send it to TestFlight: the iPhone/iPad app, the
# Apple TV app (with its Top Shelf), or both (docs/APPLE_PORT_AND_SYNC.md, Phase 5).
#
#   apple/scripts/testflight.sh                 both apps
#   apple/scripts/testflight.sh --tv            Apple TV only
#   apple/scripts/testflight.sh --mobile        iPhone and iPad only
#   apple/scripts/testflight.sh --no-upload     archive and export, keep the .ipa files
#
# Signs with an App Store Connect API key, so it runs the same on a CI runner as on a
# Mac with nobody signed in to Xcode. Set:
#   ASC_KEY_ID          the key's ID
#   ASC_ISSUER_ID       the issuer ID shown above the keys list
#   ASC_KEY_PATH        the .p8 file (CI: write the secret to a file first)
#   DEVELOPMENT_TEAM    the team the app IDs belong to
# and, optionally, BUILD_NUMBER (CI: the run number), which must rise with every upload.
# The keys the core compiles in (AURORA_TMDB_KEY and the rest) are read from the
# environment by build-core.sh, as for any build.
#
# Before the first upload, App Store Connect needs the two apps (tv.aurora.mobile and
# tv.aurora.tv), and the developer account the app group group.tv.aurora.shared; Xcode
# makes the profiles itself (-allowProvisioningUpdates). Read plan §2.6 on licensing
# before inviting anyone outside the household.
#
# Only touches apple/. A CI job runs it on a macOS runner after build-core.sh's
# prerequisites (Rust, XcodeGen) are installed; README "Releasing" says how.
set -euo pipefail

APPLE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$APPLE_DIR/.build/release"
SCHEMES=(AuroraMobile AuroraTV)
UPLOAD=1

for arg in "$@"; do
  case "$arg" in
    --tv) SCHEMES=(AuroraTV) ;;
    --mobile) SCHEMES=(AuroraMobile) ;;
    --no-upload) UPLOAD=0 ;;
    *) echo "Unknown option: $arg" >&2; exit 2 ;;
  esac
done

for var in ASC_KEY_ID ASC_ISSUER_ID ASC_KEY_PATH DEVELOPMENT_TEAM; do
  if [[ -z "${!var:-}" ]]; then
    echo "Set $var (see the top of this script)." >&2
    exit 1
  fi
done
[[ -f "$ASC_KEY_PATH" ]] || { echo "ASC_KEY_PATH does not name a file: $ASC_KEY_PATH" >&2; exit 1; }

BUILD_NUMBER="${BUILD_NUMBER:-$(date +%Y%m%d%H%M)}"
AUTH=(-allowProvisioningUpdates
      -authenticationKeyPath "$ASC_KEY_PATH"
      -authenticationKeyID "$ASC_KEY_ID"
      -authenticationKeyIssuerID "$ASC_ISSUER_ID")

# Every device slice, release: a --sim framework cannot run on hardware.
build_args=()
[[ " ${SCHEMES[*]} " == *" AuroraTV "* ]] || build_args+=(--ios)
# (Spelled so that bash 3.2, macOS's own, accepts an empty list under set -u.)
"$APPLE_DIR/scripts/build-core.sh" ${build_args[@]+"${build_args[@]}"}
"$APPLE_DIR/scripts/generate-project.sh"

rm -rf "$OUT"
mkdir -p "$OUT"
cat > "$OUT/ExportOptions.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>method</key><string>app-store-connect</string>
  <key>destination</key><string>$([[ $UPLOAD == 1 ]] && echo upload || echo export)</string>
  <key>teamID</key><string>$DEVELOPMENT_TEAM</string>
  <key>signingStyle</key><string>automatic</string>
  <key>manageAppVersionAndBuildNumber</key><false/>
</dict>
</plist>
PLIST

for scheme in "${SCHEMES[@]}"; do
  case "$scheme" in
    AuroraTV) destination="generic/platform=tvOS" ;;
    *) destination="generic/platform=iOS" ;;
  esac
  echo "==> $scheme: archive (build $BUILD_NUMBER)"
  xcodebuild -project "$APPLE_DIR/Aurora.xcodeproj" -scheme "$scheme" \
    -configuration Release -destination "$destination" \
    -archivePath "$OUT/$scheme.xcarchive" \
    DEVELOPMENT_TEAM="$DEVELOPMENT_TEAM" CODE_SIGN_STYLE=Automatic \
    CURRENT_PROJECT_VERSION="$BUILD_NUMBER" \
    "${AUTH[@]}" archive -quiet

  echo "==> $scheme: $([[ $UPLOAD == 1 ]] && echo "upload to TestFlight" || echo "export")"
  xcodebuild -exportArchive -archivePath "$OUT/$scheme.xcarchive" \
    -exportOptionsPlist "$OUT/ExportOptions.plist" \
    -exportPath "$OUT/$scheme" \
    "${AUTH[@]}"
done

echo "Done: build $BUILD_NUMBER of ${SCHEMES[*]}."
