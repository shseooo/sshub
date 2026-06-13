# 보안 규칙 (Security) — 예외 없는 절대 규칙

## MUST

- 비밀(개인 키)은 `ssh_keys/` 아래 0600 파일로만 존재시키고, `insert_ssh_key`는
  `pem_data: None`을 유지한다.
- 사용자 입력으로 파일명을 만들 때 새니타이즈한다(`key_file_name` — 영숫자·`-`·`_`만
  허용, 나머지 `_`). 경로 traversal(`../`) 차단을 테스트로 고정한다.
- 내보내기(`export_bundle`)는 비밀을 제거한다(`pem_data=None`). 개인 키를 포함해
  내보낼 때는 cocoon passphrase 암호화를 적용한다.
- 비밀번호는 연결 시 터미널에서 직접 입력하게 한다.
- CSP는 `tauri.conf.json`에 유지한다.
- 개인 키 파일 권한을 0600으로 설정하고(`secure_private_file`), 키 rename 시 개인 키
  파일도 함께 이동해 접속 경로 정합성을 유지한다.

## MUST NOT

- 개인 키 평문을 JSON 저장소·로그·내보내기 파일에 기록하지 않는다.
- 비밀번호를 저장하지 않는다.

## SHOULD

- 외부 origin을 CSP에 추가할 때는 필요한 최소 범위로만 한다.
