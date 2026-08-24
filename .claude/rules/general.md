# 일반 규칙 (General)

## MUST

- 안정성·속도·보안을 최우선으로 둔다.
- 완료 보고 전에 `cargo test --workspace`와 `cargo build --release --bin sshub`를
  통과시킨다. 자기 코드에서 나온 경고는 남기지 않는다.
- 사용자에게 보이는 문자열은 `crates/sshub/src/i18n/strings.json`에 ko/en/ja를
  함께 넣고 `bun scripts/gen_i18n.mjs`로 재생성한다.
- gpui/alacritty API는 로컬 레지스트리 소스를 grep해 확인한다 (docs.rs 추측 금지).

## MUST NOT

- `generated.rs` 같은 생성물을 직접 고치지 않는다.
- 시각적 화려함을 안정성·속도·보안보다 앞세우지 않는다.
- 대화형 `rm`이나 `git -i` 계열을 쓰지 않는다.

## SHOULD

- 커밋 메시지는 한국어 요약을 쓰고, 무엇을 고쳤는지보다 **왜**를 남긴다.
- 파일 삭제는 `git rm` 또는 `find -delete`.

## MAY

- 결정 배경은 `docs/DESIGN-*.md`를 참조한다.
