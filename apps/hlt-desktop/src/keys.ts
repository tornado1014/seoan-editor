/** 단축키 표기: 글은 맥 표기(⌘ ⇧ ⌥ ⌃)로 쓰고, 윈도에서는 보이기 직전에 Ctrl · Alt · Shift 로 바꾼다.
 *  키 처리는 이미 ⌘ 와 Ctrl 을 같이 받는다(`metaKey || ctrlKey`). 서안은 맥 · 윈도만 낸다 — 윈도가 아니면 맥 표기. */
export const isWindows = (): boolean => typeof navigator !== 'undefined' && /Windows/i.test(navigator.userAgent);

const NAMES: [string, string][] = [
  ['⌘⌃', 'Ctrl'],
  ['⌥', 'Alt'],
  ['⇧', 'Shift'],
];

export function osKeys(text: string, windows = isWindows()): string {
  if (!windows) return text;
  return text.replace(/[⌘⌃⌥⇧]+/g, (group) =>
    NAMES.filter(([marks]) => [...marks].some((mark) => group.includes(mark)))
      .map(([, name]) => `${name}+`)
      .join(''),
  );
}
