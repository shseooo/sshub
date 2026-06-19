#!/usr/bin/env bash
#
# install.sh — build sshub (Electron) and install it into /Applications (macOS).
#
# Checks build prerequisites (Xcode CLT, bun), installs any that are missing,
# builds the release .app (no DMG — faster), ad-hoc signs it, copies it to
# /Applications, and launches it. Safe to re-run.
#
# Note: sshub moved from Tauri (WKWebView) to Electron (Chromium) to fix Korean
# IME input. The old Tauri build script is kept as install-tauri.sh.
#
set -euo pipefail

APP_NAME="sshub"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

bold() { printf '\033[1m%s\033[0m\n' "$*"; }
info() { printf '\033[36m▸ %s\033[0m\n' "$*"; }
ok()   { printf '\033[32m✓ %s\033[0m\n' "$*"; }
warn() { printf '\033[33m! %s\033[0m\n' "$*"; }
die()  { printf '\033[31m✗ %s\033[0m\n' "$*" >&2; exit 1; }

[ "$(uname -s)" = "Darwin" ] || die "이 스크립트는 macOS 전용입니다. (현재: $(uname -s))"

bold "sshub (Electron) 빌드 & 설치"
echo

# ---------------------------------------------------------------------------
# 1) Xcode Command Line Tools — node-pty 네이티브 모듈 컴파일(node-gyp/clang)용
# ---------------------------------------------------------------------------
if xcode-select -p >/dev/null 2>&1; then
  ok "Xcode Command Line Tools"
else
  warn "Xcode Command Line Tools 미설치 → 설치 창을 띄웁니다."
  xcode-select --install || true
  die "설치 창에서 완료 후 이 스크립트를 다시 실행하세요."
fi

# ---------------------------------------------------------------------------
# 2) bun — 패키지 매니저 / 프론트엔드 빌드
# ---------------------------------------------------------------------------
[ -d "$HOME/.bun/bin" ] && export PATH="$HOME/.bun/bin:$PATH"
if command -v bun >/dev/null 2>&1; then
  ok "bun ($(bun --version))"
else
  info "bun 설치 중…"
  curl -fsSL https://bun.sh/install | bash
  export PATH="$HOME/.bun/bin:$PATH"
  command -v bun >/dev/null 2>&1 || die "bun 설치 실패. 터미널을 새로 열고 다시 시도하세요."
  ok "bun 설치 완료 ($(bun --version))"
fi

echo
# ---------------------------------------------------------------------------
# 3) 의존성 설치 (node-pty 는 Electron ABI 로 리빌드됨 — electron-builder 가 처리)
# ---------------------------------------------------------------------------
info "의존성 설치 (bun install)…"
bun install

# ---------------------------------------------------------------------------
# 4) 빌드: 프론트(dist) → main/preload(electron/out) → .app (DMG 생략)
# ---------------------------------------------------------------------------
info "프론트엔드 빌드 (bun run build)…"
bun run build
info "Electron main/preload 번들 (bun run electron:build)…"
bun run electron:build
info "릴리스 .app 패키징 (electron-builder --mac dir) — 몇 분 걸릴 수 있습니다…"
./node_modules/.bin/electron-builder --mac dir

APP_SRC="$(ls -d release/mac*/"${APP_NAME}".app 2>/dev/null | head -1 || true)"
[ -n "$APP_SRC" ] && [ -d "$APP_SRC" ] || die "빌드 산출물을 찾지 못했습니다: release/mac*/${APP_NAME}.app"
ok "빌드 완료: $APP_SRC"

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
