#!/usr/bin/env bash
#
# install.sh — build sshub and install it into /Applications (macOS).
#
# Checks the build prerequisites (Xcode CLT, bun, Rust), installs any that are
# missing, builds the release bundle, ad-hoc signs it, and copies it to
# /Applications. Safe to re-run.
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

bold "sshub 빌드 & 설치"
echo

# ---------------------------------------------------------------------------
# 1) Xcode Command Line Tools (clang, etc. — required to compile the Rust side)
# ---------------------------------------------------------------------------
if xcode-select -p >/dev/null 2>&1; then
  ok "Xcode Command Line Tools"
else
  warn "Xcode Command Line Tools 미설치 → 설치 창을 띄웁니다."
  xcode-select --install || true
  die "설치 창에서 완료 후 이 스크립트를 다시 실행하세요."
fi

# ---------------------------------------------------------------------------
# 2) Rust (cargo) — Tauri 백엔드 컴파일용
# ---------------------------------------------------------------------------
[ -f "$HOME/.cargo/env" ] && source "$HOME/.cargo/env"
if command -v cargo >/dev/null 2>&1; then
  ok "Rust ($(cargo --version))"
else
  info "Rust 설치 중 (rustup)…"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  source "$HOME/.cargo/env"
  command -v cargo >/dev/null 2>&1 || die "Rust 설치 실패. 터미널을 새로 열고 다시 시도하세요."
  ok "Rust 설치 완료 ($(cargo --version))"
fi

# ---------------------------------------------------------------------------
# 3) bun — 패키지 매니저 / 프론트엔드 빌드
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
# 4) 의존성 설치 + 릴리스 빌드
# ---------------------------------------------------------------------------
info "의존성 설치 (bun install)…"
bun install

info "릴리스 빌드 (bun run tauri build) — 몇 분 걸릴 수 있습니다…"
bun run tauri build

APP_SRC="src-tauri/target/release/bundle/macos/${APP_NAME}.app"
[ -d "$APP_SRC" ] || die "빌드 산출물을 찾지 못했습니다: $APP_SRC"
ok "빌드 완료: $APP_SRC"

echo
# ---------------------------------------------------------------------------
# 5) ad-hoc 서명 (Finder/Dock 실행 허용) + /Applications 설치
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
# 인터넷 다운로드가 아니므로 보통 불필요하지만, 혹시 모를 quarantine 제거.
xattr -dr com.apple.quarantine "$DEST" 2>/dev/null || true

echo
ok "설치 완료 → $DEST"
bold "Launchpad 또는 'open -a ${APP_NAME}' 로 실행하세요."
echo "  (첫 실행 시 경고가 뜨면: 우클릭 → 열기)"
