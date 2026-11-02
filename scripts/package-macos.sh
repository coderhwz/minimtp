#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
cargo build --release --locked
app="dist/MiniMTP.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/minimtp "$app/Contents/MacOS/minimtp"
cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>minimtp</string>
<key>CFBundleIdentifier</key><string>dev.hwz.minimtp</string>
<key>CFBundleName</key><string>MiniMTP</string>
<key>CFBundleDisplayName</key><string>MiniMTP</string>
<key>CFBundleVersion</key><string>1</string>
<key>CFBundleShortVersionString</key><string>0.1.0</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleIconFile</key><string>AppIcon</string>
<key>LSMinimumSystemVersion</key><string>11.0</string>
<key>NSHighResolutionCapable</key><true/>
<key>NSSupportsAutomaticGraphicsSwitching</key><true/>
</dict></plist>
PLIST
swift scripts/icon.swift
iconutil -c icns target/AppIcon.iconset -o "$app/Contents/Resources/AppIcon.icns"
codesign --force --sign - "$app"
ditto -c -k --sequesterRsrc --keepParent "$app" dist/MiniMTP-macos.zip
printf 'Built %s\n' "$app"
