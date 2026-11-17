#!/usr/bin/env bash
# Builds a release .app bundle for GPUI Browser.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

BIN=target/release/gpui-browser
APP="target/package/GPUI Browser.app"
VERSION="${1:-0.1.0}"

"$ROOT/scripts/cargo" build --release -p browser-app --bin gpui-browser

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/gpui-browser"
chmod +x "$APP/Contents/MacOS/gpui-browser"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>GPUI Browser</string>
    <key>CFBundleDisplayName</key><string>GPUI Browser</string>
    <key>CFBundleIdentifier</key><string>com.gpuibrowser.app</string>
    <key>CFBundleExecutable</key><string>gpui-browser</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
    <key>CFBundleShortVersionString</key><string>${VERSION}</string>
    <key>CFBundleVersion</key><string>1</string>
    <key>LSMinimumSystemVersion</key><string>13.0</string>
    <key>LSApplicationCategoryType</key><string>public.app-category.productivity</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSPrincipalClass</key><string>NSApplication</string>
    <key>NSHumanReadableCopyright</key><string>Copyright © 2026 GPUI Browser Contributors</string>
    <key>NSCameraUsageDescription</key><string>Websites may request camera access; GPUI Browser currently denies it by default.</string>
    <key>NSMicrophoneUsageDescription</key><string>Websites may request microphone access; GPUI Browser currently denies it by default.</string>
    <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
</dict>
</plist>
PLIST

plutil -lint "$APP/Contents/Info.plist" >/dev/null

# Generate an icon if the toolchain is available.
if command -v swift >/dev/null 2>&1 && command -v iconutil >/dev/null 2>&1; then
  ICONSET=target/package/AppIcon.iconset
  rm -rf "$ICONSET" target/package/icon-1024.png
  mkdir -p "$ICONSET"
  swift - <<'SWIFT' >/dev/null 2>&1 || true
import AppKit
let size: CGFloat = 1024
let image = NSImage(size: NSSize(width: size, height: size))
image.lockFocus()
let rect = NSRect(x: 40, y: 40, width: size - 80, height: size - 80)
let path = NSBezierPath(roundedRect: rect, xRadius: 200, yRadius: 200)
let gradient = NSGradient(colors: [
    NSColor(calibratedRed: 0.10, green: 0.16, blue: 0.24, alpha: 1),
    NSColor(calibratedRed: 0.20, green: 0.45, blue: 0.85, alpha: 1),
])!
gradient.draw(in: path, angle: -60)
let attrs: [NSAttributedString.Key: Any] = [
    .font: NSFont.systemFont(ofSize: 380, weight: .semibold),
    .foregroundColor: NSColor.white,
]
let text = "GB" as NSString
let bounds = text.size(withAttributes: attrs)
text.draw(at: NSPoint(x: (size - bounds.width) / 2, y: (size - bounds.height) / 2), withAttributes: attrs)
image.unlockFocus()
if let tiff = image.tiffRepresentation,
   let rep = NSBitmapImageRep(data: tiff),
   let png = rep.representation(using: .png, properties: [:]) {
    try? png.write(to: URL(fileURLWithPath: "target/package/icon-1024.png"))
}
SWIFT
  if [[ -f target/package/icon-1024.png ]]; then
    for size in 16 32 64 128 256 512; do
      sips -z $size $size target/package/icon-1024.png --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
      double=$((size * 2))
      sips -z $double $double target/package/icon-1024.png --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
    done
    sips -z 1024 1024 target/package/icon-1024.png --out "$ICONSET/icon_512x512@2x.png" >/dev/null
    iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns" || true
    if [[ -f "$APP/Contents/Resources/AppIcon.icns" ]]; then
      /usr/libexec/PlistBuddy -c "Add :CFBundleIconFile string AppIcon" "$APP/Contents/Info.plist"
    fi
  fi
fi

codesign --force --sign - "$APP" >/dev/null 2>&1 || echo "warning: ad-hoc codesign failed" >&2

SIZE_KB=$(du -sk "$APP" | awk '{print $1}')
echo "Bundle: $APP"
echo "Bundle size: ${SIZE_KB} KB"
echo "Binary size: $(stat -f%z "$APP/Contents/MacOS/gpui-browser") bytes"
