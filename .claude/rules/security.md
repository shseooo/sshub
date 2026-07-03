# 보안 규칙 (Security) — 예외 없는 절대 규칙

## MUST

- 비밀(개인 키)은 `ssh_keys/` 아래 0600 파일로만 존재시키고, JSON 저장소에는
  키 평문을 넣지 않는다.
- 사용자 입력으로 파일명을 만들 때 새니타이즈한다(`keyFileName` —
  electron/lib/keyFiles.ts, 영숫자·`-`·`_`만 허용, 나머지 `_`). 경로
  traversal(`../`) 차단을 테스트로 고정한다.
- 내보내기(`export_data`)는 기본적으로 비밀을 제거한다. 개인 키를 포함해
  내보낼 때는 AES-256-GCM(scrypt) passphrase 암호화를 적용한다
  (`electron/lib/crypto.ts`).
- 비밀번호는 연결 시 터미널에서 직접 입력하게 한다.
- BrowserWindow는 `contextIsolation: true`·`nodeIntegration: false`를 유지하고,
  preload는 `invoke`/`on`만 노출한다.
- 개인 키 파일 권한을 0600으로 설정하고, 키 rename 시 개인 키 파일도 함께
  이동해 접속 경로 정합성을 유지한다.

## MUST NOT

- 개인 키 평문을 JSON 저장소·로그·내보내기 파일에 기록하지 않는다.
- 비밀번호를 저장하지 않는다.
- renderer 입력 경로를 검증 없이 fs API에 넘기지 않는다.
