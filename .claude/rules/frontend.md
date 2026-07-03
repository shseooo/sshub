# 프론트엔드 규칙 (React 19 · TS · Tailwind v4)

## MUST

- 컴포넌트는 함수형 + 훅으로 작성한다.
- 상태를 다음과 같이 배치한다: 서버 상태 → TanStack Query 훅(`useServers`,
  `useKeys`), 전역 UI 상태 → Context(Terminal/Language/Shortcuts/Theme),
  화면 로컬 상태 → `useState`.
- IPC는 `src/lib/commands.ts` 래퍼를 통해 호출한다. 새 커맨드를 추가하면
  래퍼 함수와 `electron/main.ts`의 `invoke` switch 등록을 함께 추가한다.
- 드롭다운은 `@/components/Select`를 사용한다.
- 공유 타입은 `src/types/`에 두고, DTO는 메인 프로세스와 camelCase로 1:1로
  맞춘다.

## MUST NOT

- 클래스 컴포넌트를 사용하지 않는다.
- 컴포넌트/훅에서 `window.electronAPI`/`invoke`를 직접 호출하지 않는다.
- 네이티브 `<select>`를 새로 추가하지 않는다.

## SHOULD

- 복잡한 순수 로직은 `src/lib/`로 분리해 테스트 가능하게 만든다.
- CRT/phosphor 테마 토큰(`bg-background`, `text-phosphor`, `border-border`,
  `crt-in` 등)을 재사용하고, 스타일 관습(2칸 들여쓰기·무세미콜론·작은따옴표)을 따른다.
- 주석은 "왜"를 설명한다.

## SHOULD NOT

- 테스트 불가능한 형태로 순수 로직을 컴포넌트에 인라인하지 않는다.
- 자명한 "무엇"을 반복하는 주석을 달지 않는다.

## MAY

- 단위 테스트를 위해 순수 함수를 `export`로 노출한다.
