# 테스트 규칙 (Testing)

## MUST

- 다음은 반드시 테스트한다: ssh config 문서 모델(라운드트립 바이트 동일·최소
  diff), 트리/상태 변환(split/merge/insert/remove), 암호화 라운드트립,
  보안 경계(경로 traversal·비밀 미저장), 저장 포맷 호환.
- 회귀 버그는 재현 테스트를 **먼저** 쓴다. 그리고 **수정을 되돌려 실제로
  실패하는지 확인한다** — 통과하는 테스트가 아무것도 검증하지 않는 경우가 있다
  (포커스 회귀 테스트가 실제로 그랬다).
- 테스트는 `tempfile` + `AppPaths::in_dir`만 쓴다. 실제 사용자 파일 금지.
- 테스트는 대상 파일 옆 `#[cfg(test)] mod tests` 또는 `crates/*/tests/`에 둔다.

## SHOULD

- 작은 순수 함수는 TDD로 쓴다.
- GUI는 자동 검증이 어렵다 — 순수 로직을 뽑아 테스트하고, 화면은 사용자 확인에 맡긴다.
- `#[gpui::test]` + `VisualTestContext`로 액션·포커스 경로는 검증할 수 있다.

## SHOULD NOT

- 렌더링 결과를 단언하려 하지 않는다.

> 검증: `cargo test --workspace`
