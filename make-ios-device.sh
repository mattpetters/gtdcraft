#!/bin/sh
# Builds GTDCraft for a real iPhone/iPad, installs it and launches it.
#
#   ./make-ios-device.sh                    # the first paired iPhone/iPad found
#   ./make-ios-device.sh "My iPhone"        # by name
#   ./make-ios-device.sh 00008130-...       # by UDID
#
# Needs, once: Xcode › Settings › Accounts with your Apple ID (a free one is enough), ./ios/setup.sh (your team and
# bundle ID, in the git-ignored ios/Config/Local.xcconfig), the iPhone unlocked (and
# trusted by this Mac), and Settings › Privacy & Security › Developer Mode on. A free account's build stops working
# after 7 days; run this again to reinstall it.
set -e
cd "$(dirname "$0")"
WANT="${1:-}"

# your own team and identifiers (./ios/setup.sh writes them once)
if [ ! -f ios/Config/Local.xcconfig ]; then
  echo "Run ./ios/setup.sh first: it writes your Apple team and bundle ID to ios/Config/Local.xcconfig."
  exit 1
fi
BUNDLE=$(sed -n 's/^GTD_BUNDLE_ID *= *//p' ios/Config/Local.xcconfig | tail -1)
[ -n "$BUNDLE" ] || { echo "ios/Config/Local.xcconfig has no GTD_BUNDLE_ID; run ./ios/setup.sh --force."; exit 1; }

# Find the device: devicectl's identifier is for installing, hardwareProperties.udid is what xcodebuild wants.
LIST=$(mktemp)
xcrun devicectl list devices --json-output "$LIST" >/dev/null 2>&1
IDS=$({ [ -n "$WANT" ] && printf '%s' "$WANT"; } | python3 -c '
import json,sys
want = sys.argv[1].strip() if len(sys.argv) > 1 else ""
picks = []
for d in json.load(open(sys.argv[2]))["result"].get("devices", []):
    hw = d.get("hardwareProperties", {})
    if hw.get("reality") != "physical" or not hw.get("udid"):
        continue
    name = d.get("deviceProperties", {}).get("name", "")
    keys = (d.get("identifier", ""), hw["udid"], name)
    if want and not any(want.lower() in k.lower() for k in keys):
        continue
    picks.append(d.get("identifier", "") + " " + hw["udid"])
print(picks[0] if picks else "")
' "$WANT" "$LIST")
[ -n "$IDS" ] || { echo "No iPhone found. Plug it in (or put it on the same Wi-Fi) and trust this Mac."; exit 1; }
DEV="${IDS% *}"
UDID="${IDS#* }"
echo "Device: $UDID ($DEV)"

# A locked phone can't be built for or installed to; say so now instead of after a minute.
if xcrun devicectl device info details --device "$DEV" 2>&1 | grep -q "disk image could not be mounted"; then
  echo "Your iPhone is locked. Unlock it, leave it on the home screen, and run this again."
  exit 1
fi

# Build: this is what creates or renews the provisioning profile.
cd ios
command -v xcodegen >/dev/null 2>&1 && xcodegen generate >/dev/null
set +e
# version from Cargo.toml and build number = commit count, like the Mac app's (CFBundleVersion)
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' ../Cargo.toml | head -1)
BUILD=$(git rev-list --count HEAD 2>/dev/null || echo 1)
xcodebuild -project GTDCraft.xcodeproj -scheme GTDCraft -configuration Release \
  -destination "platform=iOS,id=$UDID" -allowProvisioningUpdates -derivedDataPath build build \
  MARKETING_VERSION="$VERSION" CURRENT_PROJECT_VERSION="$BUILD" 2>&1 | tail -25
set -e
APP=build/Build/Products/Release-iphoneos/GTDCraft.app
if [ ! -d "$APP" ]; then
  echo
  echo "The build didn't produce $APP."
  echo "Most likely: no Apple ID in Xcode yet — Xcode › Settings › Accounts › + › Apple ID (a free one is fine),"
  echo "or the iPhone is locked — unlock it and run this again."
  exit 1
fi

# Install and launch.
xcrun devicectl device install app --device "$DEV" "$APP"
xcrun devicectl device process launch --device "$DEV" "$BUNDLE"
echo "GTDCraft is on your iPhone."
