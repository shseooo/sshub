#!/usr/bin/env bash
#
# install.sh — build sshub (Rust/GPUI) and install it into /Applications (macOS).
#
# Electron 판 ../install.sh 의 Rust 대응. 릴리스 바이너리를 빌드해 .app 번들을
# 손으로 조립하고(gpui는 번들러가 없다), ad-hoc 서명 후 /Applications 에 설치한다.
# 재실행해도 안전하다.
#
set -euo pipefail

APP_NAME="sshub"
BUNDLE_ID="com.massivelinks.sshub"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

bold() { printf '\033[1m%s\033[0m\n' "$*"; }
info() { printf '\033[36m▸ %s\033[0m\n' "$*"; }
ok()   { printf '\033[32m✓ %s\033[0m\n' "$*"; }
warn() { printf '\033[33m! %s\033[0m\n' "$*"; }
die()  { printf '\033[31m✗ %s\033[0m\n' "$*" >&2; exit 1; }

[ "$(uname -s)" = "Darwin" ] || die "이 스크립트는 macOS 전용입니다. (현재: $(uname -s))"

bold "sshub (Rust/GPUI) 빌드 & 설치"
echo

# ---------------------------------------------------------------------------
# 1) Rust 툴체인
# ---------------------------------------------------------------------------
[ -d "$HOME/.cargo/bin" ] && export PATH="$HOME/.cargo/bin:$PATH"
if command -v cargo >/dev/null 2>&1; then
  ok "cargo ($(cargo --version | cut -d' ' -f2))"
else
  info "rustup 설치 중…"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  export PATH="$HOME/.cargo/bin:$PATH"
  command -v cargo >/dev/null 2>&1 || die "rustup 설치 실패. 터미널을 새로 열고 다시 시도하세요."
  ok "cargo 설치 완료"
fi

# ---------------------------------------------------------------------------
# 2) Xcode + Metal 툴체인 — gpui의 build.rs가 Metal 셰이더를 컴파일한다.
#    CommandLineTools만으로는 `metal` 이 없어 빌드가 실패한다.
#    (.cargo/config.toml 이 DEVELOPER_DIR 을 Xcode 로 지정해 두었다.)
# ---------------------------------------------------------------------------
XCODE_DIR="/Applications/Xcode.app/Contents/Developer"
if [ -d "$XCODE_DIR" ]; then
  ok "Xcode"
  if DEVELOPER_DIR="$XCODE_DIR" xcrun --find metal >/dev/null 2>&1; then
    ok "Metal 툴체인"
  else
    warn "Metal 툴체인 미설치 → 내려받습니다 (약 700MB, 몇 분 소요)."
    DEVELOPER_DIR="$XCODE_DIR" xcodebuild -downloadComponent MetalToolchain \
      || die "Metal 툴체인 설치 실패. Xcode를 한 번 실행해 초기 설정을 마친 뒤 다시 시도하세요."
    ok "Metal 툴체인 설치 완료"
  fi
else
  warn "Xcode 미설치 — gpui 셰이더 빌드는 runtime_shaders 폴백으로 진행합니다."
  warn "빌드가 실패하면 App Store에서 Xcode를 설치하세요."
fi

echo
# ---------------------------------------------------------------------------
# 3) 빌드 & 테스트
# ---------------------------------------------------------------------------
info "테스트 (cargo test --workspace)…"
cargo test --workspace --quiet

info "릴리스 빌드 (cargo build --release) — 처음이면 몇 분 걸립니다…"
cargo build --release --bin "$APP_NAME"

BIN="target/release/${APP_NAME}"
[ -x "$BIN" ] || die "빌드 산출물을 찾지 못했습니다: $BIN"
ok "빌드 완료: $BIN"

echo
# ---------------------------------------------------------------------------
# 4) .app 번들 조립 (gpui는 자체 번들러가 없다)
# ---------------------------------------------------------------------------
APP_SRC="target/release/${APP_NAME}.app"
info ".app 번들 생성…"
rm -rf "$APP_SRC"
mkdir -p "$APP_SRC/Contents/MacOS" "$APP_SRC/Contents/Resources"
cp "$BIN" "$APP_SRC/Contents/MacOS/${APP_NAME}"

VERSION="$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)".*/\1/')"
cat > "$APP_SRC/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>${APP_NAME}</string>
  <key>CFBundleDisplayName</key><string>${APP_NAME}</string>
  <key>CFBundleIdentifier</key><string>${BUNDLE_ID}</string>
  <key>CFBundleVersion</key><string>${VERSION}</string>
  <key>CFBundleShortVersionString</key><string>${VERSION}</string>
  <key>CFBundleExecutable</key><string>${APP_NAME}</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <!-- 메뉴바·창을 가진 일반 GUI 앱 (Dock에 표시) -->
  <key>LSUIElement</key><false/>
  <key>NSHighResolutionCapable</key><true/>
  <!-- 다크 UI 고정: 앱 테마가 다크 전용이라 시스템 라이트에서 크롬만 밝아지는 것을 막는다 -->
  <key>NSRequiresAquaSystemAppearance</key><false/>
</dict>
</plist>
PLIST

if [ -f "assets/icon.icns" ]; then
  cp assets/icon.icns "$APP_SRC/Contents/Resources/icon.icns"
  /usr/libexec/PlistBuddy -c "Add :CFBundleIconFile string icon" \
    "$APP_SRC/Contents/Info.plist" >/dev/null 2>&1 || true
else
  warn "assets/icon.icns 없음 — 기본 아이콘으로 설치합니다."
fi
ok "번들 생성: $APP_SRC"

echo
# ---------------------------------------------------------------------------
# 5) ad-hoc 서명 (Finder/Dock 실행 허용; hardened runtime 미부여) + 설치
# ---------------------------------------------------------------------------
info "ad-hoc 서명…"
codesign --force --deep --sign - "$APP_SRC"

DEST="/Applications/${APP_NAME}.app"
if pgrep -x "$APP_NAME" >/dev/null 2>&1; then
  warn "${APP_NAME}이(가) 실행 중입니다 → 종료합니다."
  osascript -e "quit app \"${APP_NAME}\"" 2>/dev/null || pkill -x "$APP_NAME" || true
  sleep 1
fi

info "/Applications 로 복사…"
rm -rf "$DEST"
cp -R "$APP_SRC" "$DEST"
xattr -dr com.apple.quarantine "$DEST" 2>/dev/null || true

echo
ok "설치 완료 → $DEST"

info "${APP_NAME} 실행…"
if open "$DEST"; then
  ok "${APP_NAME} 실행됨."
else
  warn "자동 실행 실패. Launchpad 또는 'open -a ${APP_NAME}' 로 실행하세요."
  echo "  (경고가 뜨면: 우클릭 → 열기)"
fi
