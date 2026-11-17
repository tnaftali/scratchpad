#!/bin/sh
# Build target/release/Scratchpad.app, so the Dock shows the app name and icon. --install copies it to /Applications.
set -eu
cd "$(dirname "$0")/.."
cargo build --release
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
app=target/release/Scratchpad.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/scratchpad "$app/Contents/MacOS/"

iconset="$(mktemp -d)/AppIcon.iconset"
mkdir -p "$iconset"
for s in 16 32 128 256; do
  sips -z $s $s docs/icon-512.png --out "$iconset/icon_${s}x${s}.png" >/dev/null
  sips -z $((s * 2)) $((s * 2)) docs/icon-512.png --out "$iconset/icon_${s}x${s}@2x.png" >/dev/null
done
cp docs/icon-512.png "$iconset/icon_512x512.png"
iconutil -c icns "$iconset" -o "$app/Contents/Resources/AppIcon.icns"

cat >"$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Scratchpad</string>
  <key>CFBundleDisplayName</key><string>Scratchpad</string>
  <key>CFBundleExecutable</key><string>scratchpad</string>
  <key>CFBundleIdentifier</key><string>com.github.tnaftali.scratchpad</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
EOF
# A stable identity keeps macOS privacy grants (Documents access) across rebuilds;
# an ad-hoc signature changes every build, so macOS asks again each time.
identity=$(security find-identity -v -p codesigning | sed -n 's/.*"\(Apple Development: .*\)"/\1/p' | head -1)
codesign --force --sign "${identity:--}" "$app" || {
  # The keychain asks for approval, which works only from a GUI terminal.
  echo "warning: signing with '$identity' failed, using ad-hoc; macOS will ask for Documents access again" >&2
  codesign --force --sign - "$app"
}

if [ "${1:-}" = "--install" ]; then
  osascript -e 'quit app "Scratchpad"' 2>/dev/null || true
  rm -rf /Applications/Scratchpad.app
  ditto "$app" /Applications/Scratchpad.app
  app=/Applications/Scratchpad.app
fi
echo "$app"
