#!/bin/sh
# Builds GTDCraft.app (release) with the elephant icon into ./dist. Run from app/.
set -e
cargo build --release
APP=dist/GTDCraft.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/gtdcraft "$APP/Contents/MacOS/gtdcraft"
cp target/release/gtdcraft-calendar "$APP/Contents/MacOS/gtdcraft-calendar"  # Calendar helper (installed on first use)
cp assets/GTDCraft.icns "$APP/Contents/Resources/GTDCraft.icns"
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
# the version lives in Cargo.toml; Info.plist (also embedded in the binary by build.rs) gets it stamped in
BUILD=$(git rev-list --count HEAD 2>/dev/null || echo 0)
sed -e "s#<key>CFBundleShortVersionString</key><string>[^<]*#<key>CFBundleShortVersionString</key><string>$VERSION#" \
    -e "s#<key>CFBundleVersion</key><string>[^<]*#<key>CFBundleVersion</key><string>$BUILD#" Info.plist > "$APP/Contents/Info.plist"
# sign so macOS ties privacy permissions (Calendar) to GTDCraft.app: this Mac's developer identity when there is
# one (the grant then survives a rebuild), ad-hoc otherwise
IDENTITY=$(security find-identity -v -p codesigning 2>/dev/null | sed -n 's/.*"\(Apple Development:.*\)"/\1/p' | head -1)
if [ -n "$IDENTITY" ]; then
  codesign --force --sign "$IDENTITY" --identifier local.gtdcraft "$APP" >/dev/null 2>&1 || echo "codesign failed (permissions may be asked per launch)"
else
  codesign --force --sign - --identifier local.gtdcraft "$APP" >/dev/null 2>&1 || echo "codesign failed (permissions may be asked per launch)"
fi
echo "Built $APP"
