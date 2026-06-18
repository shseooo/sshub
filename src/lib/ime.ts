// IME(한글/CJK 조합) 보호용 순수 판정.
//
// xterm 은 숨겨진 textarea 를 누적시켜 두고 조합 위치를 selectionStart/End 오프셋으로
// 추적한다(compositionend 후에도 textarea 를 비우지 않음 — xtermjs/xterm.js#6012).
// 일반 방향키는 xterm 이 escape 시퀀스로 처리하며 preventDefault 하므로 캐럿이 안
// 움직이지만, xterm 이 소비하지 않는 "모디파이어+방향키"(예: ⌘←)는 브라우저 기본
// 동작이 textarea 캐럿을 이전 조합 텍스트 중간으로 옮긴다. 그러면 다음 조합이 중간에
// 삽입되어 오프셋이 어긋나고 마지막 글자가 무한 중복된다.
//
// VS Code 도 같은 xterm.js 를 쓰며, attachCustomKeyEventHandler 에서 이런 키를
// preventDefault + return false 로 가로채 캐럿이 애초에 움직이지 않게 한다. 조합은
// 평범한 글자 키로만 일어나므로(여기서 절대 매칭되지 않음) 한글 입력은 영향 없다.

export interface KeyLike {
  type?: string
  key: string
  metaKey?: boolean
  ctrlKey?: boolean
  altKey?: boolean
}

const NAV = new Set(['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End'])

/**
 * 캐럿을 이동시켜 조합 오프셋을 깨뜨릴 수 있어 xterm 처리 전에 소비해야 하는
 * 키인지 판정한다 — 모디파이어(⌘/Ctrl/Alt)와 함께 눌린 방향키/Home/End 만.
 */
export function shouldConsumeNavKey(e: KeyLike): boolean {
  if (e.type && e.type !== 'keydown') return false
  if (!NAV.has(e.key)) return false
  return Boolean(e.metaKey || e.ctrlKey || e.altKey)
}
