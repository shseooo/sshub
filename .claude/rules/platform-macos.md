# 플랫폼 규칙 (macOS)

## MUST

- 웹뷰 내부 HTML5 드래그앤드롭을 쓰려면 윈도우 설정에 `dragDropEnabled: false`를
  지정한다(기본값 true면 OS 파일드롭 핸들러가 DnD 이벤트를 가로챈다).
- 빌드 후 Finder/Dock 실행을 위해 ad-hoc 서명을 적용한다:
  `codesign --force --deep --sign - …/sshub.app`.
- 투명도(vibrancy)는 `macos-private-api` + window-vibrancy로 구현하고, 알파는
  `--background` CSS 변수에 baked-in 한다(레이어 이중 합성 방지).
- 앱 아이콘은 full-bleed 불투명 이미지를 사용한다(투명 마진은 macOS Tahoe에서 회색
  타일로 보임).

## MUST NOT

- ad-hoc 서명에 hardened runtime 플래그를 함께 부여하지 않는다(GUI 실행이 Gatekeeper에
  막힐 수 있음).

## MAY

- 다른 Mac으로 복사 후 quarantine으로 막히면
  `xattr -dr com.apple.quarantine /Applications/sshub.app`를 안내한다.
