#!/bin/sh
# Builds GTDCraft for the iOS Simulator, installs and launches it on an iPhone simulator, and saves a screenshot.
# Usage (from app/): ./make-ios.sh [device name, default "iPhone 17 Pro"] [screenshot path]
set -e
DEVICE="${1:-iPhone 17 Pro}"
SHOT="${2:-dist/ios-screenshot.png}"
TARGET=aarch64-apple-ios-sim
cargo build --release --target $TARGET --bin gtdcraft
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
BUILD=$(git rev-list --count HEAD 2>/dev/null || echo 0)
APP=dist/ios/GTDCraft.app
rm -rf "$APP"
mkdir -p "$APP"
cp target/$TARGET/release/gtdcraft "$APP/gtdcraft"
cp ios/Assets.xcassets/AppIcon.appiconset/AppIcon-1024.png "$APP/AppIcon.png"  # full-bleed; iOS rounds it
cat > "$APP/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>GTDCraft</string>
  <key>CFBundleDisplayName</key><string>GTDCraft</string>
  <key>CFBundleIdentifier</key><string>local.gtdcraft</string>
  <key>CFBundleExecutable</key><string>gtdcraft</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$BUILD</string>
  <key>MinimumOSVersion</key><string>17.0</string>
  <key>UIDeviceFamily</key><array><integer>1</integer><integer>2</integer></array>
  <key>UILaunchScreen</key><dict/>
  <key>UIRequiresFullScreen</key><false/>
  <key>UISupportedInterfaceOrientations</key><array><string>UIInterfaceOrientationPortrait</string><string>UIInterfaceOrientationLandscapeLeft</string><string>UIInterfaceOrientationLandscapeRight</string></array>
  <key>CFBundleIcons</key><dict><key>CFBundlePrimaryIcon</key><dict><key>CFBundleIconFiles</key><array><string>AppIcon</string></array></dict></dict>
  <key>NSCalendarsFullAccessUsageDescription</key><string>GTDCraft shows your Calendar events in Forecast.</string>
  <key>NSLocationWhenInUseUsageDescription</key><string>GTDCraft uses your location to show nearby places and send location reminders.</string>
  <key>NSSupportsLiveActivities</key><true/>
  <key>NSLocalNetworkUsageDescription</key><string>GTDCraft syncs with the sync server on your Mac over your network.</string>
  <key>CFBundleURLTypes</key><array><dict><key>CFBundleURLName</key><string>local.gtdcraft.pair</string><key>CFBundleURLSchemes</key><array><string>gtdcraft</string></array></dict></array>
</dict></plist>
PLIST
codesign --force --sign - "$APP" >/dev/null 2>&1 || true
UDID=$(xcrun simctl list devices available | grep -F "$DEVICE (" | head -1 | sed -E 's/.*\(([0-9A-F-]{36})\).*/\1/')
if [ -z "$UDID" ]; then echo "No simulator named $DEVICE"; xcrun simctl list devices available | grep -E "iPhone|iPad" | head; exit 1; fi
xcrun simctl boot "$UDID" 2>/dev/null || true
xcrun simctl bootstatus "$UDID" -b >/dev/null
xcrun simctl install "$UDID" "$APP"
xcrun simctl terminate "$UDID" local.gtdcraft >/dev/null 2>&1 || true
SIMCTL_CHILD_GTDCRAFT_NO_NOTIFY=1 xcrun simctl launch "$UDID" local.gtdcraft
sleep 6
mkdir -p "$(dirname "$SHOT")"
xcrun simctl io "$UDID" screenshot "$SHOT" >/dev/null
echo "Installed on $DEVICE ($UDID); screenshot: $SHOT"
