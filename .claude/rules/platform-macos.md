# 플랫폼 규칙 (macOS)

## MUST

- 빌드 후 Finder/Dock 실행을 위해 ad-hoc 서명을 적용한다(`install.sh`가 수행):
  `codesign --force --deep --sign - release/mac-arm64/sshub.app`.
- 투명도는 BrowserWindow `vibrancy: 'hud'` + 투명 `backgroundColor`로 구현하고,
  알파는 `--background` CSS 변수에 baked-in 한다(레이어 이중 합성 방지).
- 타이틀바 드래그는 CSS `-webkit-app-region: drag`(`.app-drag`)로 처리한다.
- 앱 아이콘은 full-bleed 불투명 이미지를 사용한다(투명 마진은 macOS Tahoe에서 회색
  타일로 보임).
- node-pty는 Electron ABI로 리빌드되어야 한다(electron-builder/`@electron/rebuild`가
  수행 — 네이티브 모듈 로드 에러 시 의심).

## MUST NOT

- ad-hoc 서명에 hardened runtime 플래그를 함께 부여하지 않는다(GUI 실행이 Gatekeeper에
  막힐 수 있음).

## MAY

- 다른 Mac으로 복사 후 quarantine으로 막히면
  `xattr -dr com.apple.quarantine /Applications/sshub.app`를 안내한다.
