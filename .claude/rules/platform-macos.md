# 플랫폼 규칙 (macOS)

## MUST

- 빌드 후 Finder/Dock 실행을 위해 ad-hoc 서명을 적용한다(`install.sh`가 수행):
  `codesign --force --deep --sign - target/release/sshub.app`.
- 투명도는 `WindowBackgroundAppearance::Blurred` + 알파를 **픽셀당 한 겹만**
  칠해서 구현한다 (겹쳐 칠하면 0.6³≈0.94로 불투명해진다).
- 타이틀바 드래그 스트립은 **레이아웃 흐름 안**에 둔다. 겹쳐 그리면 그 아래
  UI의 클릭을 통째로 삼킨다.
- 앱 아이콘은 full-bleed 불투명 이미지를 사용한다(투명 마진은 macOS Tahoe에서 회색
  타일로 보임).
- gpui 빌드는 Metal 셰이더 컴파일에 full Xcode가 필요하다
  (`.cargo/config.toml`의 `DEVELOPER_DIR` + `runtime_shaders` feature).

## MUST NOT

- ad-hoc 서명에 hardened runtime 플래그를 함께 부여하지 않는다(GUI 실행이 Gatekeeper에
  막힐 수 있음).

## MAY

- 다른 Mac으로 복사 후 quarantine으로 막히면
  `xattr -dr com.apple.quarantine /Applications/sshub.app`를 안내한다.
