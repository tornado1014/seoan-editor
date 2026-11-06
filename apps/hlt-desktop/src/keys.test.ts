import { describe, expect, test } from 'vitest';
import { osKeys } from './keys';

describe('osKeys', () => {
  test('맥은 그대로 · 윈도는 Ctrl · Alt · Shift 차례로', () => {
    expect(osKeys('저장 (⇧⌘S)', false)).toBe('저장 (⇧⌘S)');
    const cases: [string, string][] = [
      ['⇧⌘Z', 'Ctrl+Shift+Z'],
      ['⌘K', 'Ctrl+K'],
      ['⇧Tab', 'Shift+Tab'],
      ['⌘Enter', 'Ctrl+Enter'],
      ['⌘+ · ⌘− · ⌘0', 'Ctrl++ · Ctrl+− · Ctrl+0'],
      ['설정 (⌘,)', '설정 (Ctrl+,)'],
      ['⌥⇧⌘X', 'Ctrl+Alt+Shift+X'],
    ];
    for (const [mac, windows] of cases) expect(osKeys(mac, true)).toBe(windows);
  });
});
