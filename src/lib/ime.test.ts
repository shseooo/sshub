import { describe, it, expect } from 'vitest'
import { shouldConsumeNavKey } from './ime'

describe('shouldConsumeNavKey', () => {
  // 재현 버그: 한글 조합 중 ⌘← 로 줄 맨앞으로 이동하면 이후 입력이 중복된다.
  it('⌘← (줄 맨앞 이동)을 소비 대상으로 판정한다', () => {
    expect(shouldConsumeNavKey({ key: 'ArrowLeft', metaKey: true })).toBe(true)
  })

  it('모디파이어+방향키/Home/End 를 소비한다', () => {
    expect(shouldConsumeNavKey({ key: 'ArrowRight', metaKey: true })).toBe(true)
    expect(shouldConsumeNavKey({ key: 'ArrowLeft', altKey: true })).toBe(true)
    expect(shouldConsumeNavKey({ key: 'ArrowUp', ctrlKey: true })).toBe(true)
    expect(shouldConsumeNavKey({ key: 'Home', metaKey: true })).toBe(true)
    expect(shouldConsumeNavKey({ key: 'End', ctrlKey: true })).toBe(true)
  })

  // 모디파이어 없는 방향키/Home/End 는 xterm 이 이미 preventDefault 하므로 소비 불필요.
  it('모디파이어 없는 방향키·Home/End 는 소비하지 않는다', () => {
    expect(shouldConsumeNavKey({ key: 'ArrowLeft' })).toBe(false)
    expect(shouldConsumeNavKey({ key: 'Home' })).toBe(false)
    expect(shouldConsumeNavKey({ key: 'End' })).toBe(false)
  })

  // Shift+방향키(선택)는 조합과 무관하고 xterm 이 처리 — 소비하지 않는다.
  it('Shift 만 눌린 방향키는 소비하지 않는다', () => {
    expect(shouldConsumeNavKey({ key: 'ArrowLeft', metaKey: false, ctrlKey: false, altKey: false })).toBe(false)
  })

  // 한글 자모 등 일반 글자 키는 절대 소비하지 않는다(조합 보존의 핵심).
  it('일반 글자/조합 키는 소비하지 않는다', () => {
    expect(shouldConsumeNavKey({ key: 't', metaKey: true })).toBe(false)
    expect(shouldConsumeNavKey({ key: 'ㅅ' })).toBe(false)
    expect(shouldConsumeNavKey({ key: 'Process' })).toBe(false)
    expect(shouldConsumeNavKey({ key: 'Enter', metaKey: true })).toBe(false)
  })

  // keydown 외 이벤트(keyup 등)는 소비하지 않는다.
  it('keydown 이 아니면 소비하지 않는다', () => {
    expect(shouldConsumeNavKey({ type: 'keyup', key: 'ArrowLeft', metaKey: true })).toBe(false)
  })
})
