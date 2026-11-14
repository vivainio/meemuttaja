#!/bin/sh
set -eu
binary=${1:-target/release/meemuttaja}
output=${2:-dist}
app="$output/Meemuttaja.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary" "$app/Contents/MacOS/meemuttaja"
cp assets/icon.icns "$app/Contents/Resources/icon.icns"
cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleExecutable</key><string>meemuttaja</string>
  <key>CFBundleIdentifier</key><string>com.github.vivainio.meemuttaja</string>
  <key>CFBundleName</key><string>Meemuttaja</string>
  <key>CFBundleDisplayName</key><string>Meemuttaja</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleVersion</key><string>0.2.3</string>
  <key>CFBundleShortVersionString</key><string>0.2.3</string>
  <key>CFBundleIconFile</key><string>icon</string>
  <key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
# Ad-hoc sign so the bundle has a valid signature (avoids "damaged" errors on Apple Silicon)
codesign --force --deep --sign - "$app"
codesign --verify --deep --strict "$app"
