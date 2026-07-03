# 테스트 규칙 (Testing · TDD)

## MUST

- 다음 범주는 테스트를 작성한다: 트리/상태 변환(split/merge/insert/remove),
  파서(`electron/lib/sshConfig.ts`), 직렬화·정규화(`normalizeCombo`·키 타입
  판별·파일명 새니타이즈), 보안 경계(경로 traversal 차단·비밀 미저장·암호화
  라운드트립).
- 회귀 버그를 고칠 때는 그 버그를 재현하는 테스트를 먼저 추가한다.
- 테스트는 대상 파일 옆의 `*.test.ts(x)`에 둔다(Vitest — `src/**`와
  `electron/**` 모두).

## SHOULD

- 작은 순수 함수는 TDD로 작성한다(실패 → 최소 구현 → 리팩터).
- 로직을 함수로 추출해 단위 테스트한다.

## SHOULD NOT

- UI 컴포넌트 전체를 E2E로 검증하려 하지 않는다.

## MAY

- 테스트만을 위해 순수 함수를 `export`로 노출한다.

> 검증: `bun run test` / `bun run test:watch`
