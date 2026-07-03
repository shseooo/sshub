# 일반 규칙 (General)

## MUST

- 패키지 매니저는 bun 을 사용한다.
- 안정성·속도·보안을 최우선으로 둔다.
- 변경을 커밋/완료로 보고하기 전에 `bun run build`, `bun run typecheck:electron`,
  `bun run test`를 모두 통과시킨다.
- 사용자에게 노출되는 문자열은 i18n 3종(ko/en/ja)에 동시에 추가한다.

## MUST NOT

- `npm`/`yarn`을 사용하거나 `package-lock.json`을 생성하지 않는다.
- 시각적 화려함을 안정성·속도·보안보다 앞세우지 않는다.
- 사용자 문자열을 한 언어에만 추가하지 않는다.
- `~/.ssh/config`의 사용자/린터 지정 값을 임의로 되돌리지 않는다.
- 대화형 `rm`이나 `git -i` 계열을 사용하지 않는다.

## SHOULD

- 커밋 메시지는 한국어 요약을 사용한다.
- 파일 삭제는 `git rm` 또는 `find -delete`를 사용한다.

## MAY

- 결정 배경이 필요하면 `docs/DEVELOPMENT.md`를 참조한다.
