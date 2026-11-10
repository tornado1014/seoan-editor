import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';
import type { LiveIssueDto } from './contracts';
import { createLiveChecks, summarize } from './live-checks';

const issue = (severity: 'error' | 'warning', paragraph_id: string | null, detail = 'd'): LiveIssueDto => ({
  severity,
  code: null,
  paragraph_id,
  detail,
});

describe('summarize', () => {
  test('문단 없는 항목은 건수에만 · 같은 문단에 오류와 경고가 있으면 그 문단 표시는 오류', () => {
    const sum = summarize([issue('warning', 'p1'), issue('error', 'p1'), issue('warning', null), issue('warning', 'p2')]);
    expect([sum.errors, sum.warnings]).toEqual([1, 3]);
    expect([...sum.byParagraph.keys()]).toEqual(['p1', 'p2']);
    expect(sum.byParagraph.get('p1')!.length).toBe(2);
    expect(sum.byParagraph.get('p1')![0].severity, '오류가 앞').toBe('error');
  });
});

describe('createLiveChecks', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());
  const make = (answer: () => { revision: number; issues: LiveIssueDto[] }, revision = () => 3) => {
    const fetch = vi.fn(async () => answer());
    const draw = vi.fn();
    const strip = vi.fn();
    return { fetch, draw, strip, live: createLiveChecks({ fetch, revision, draw, strip }) };
  };

  test('300ms 간격으로 5번 touch 해도 마지막 뒤 1000ms 에 한 번만 가져온다', async () => {
    const { fetch, strip, live } = make(() => ({ revision: 3, issues: [] }));
    for (let i = 0; i < 5; i += 1) {
      live.touch();
      await vi.advanceTimersByTimeAsync(300);
    }
    expect(fetch).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(700);
    expect(fetch).toHaveBeenCalledTimes(1);
    expect(strip).toHaveBeenCalledWith(0, 0);
  });

  test('낡은 판의 결과는 그리지 않고 다시 예약한다', async () => {
    let screen = 4;
    const { fetch, draw, strip, live } = make(() => ({ revision: 3, issues: [issue('error', 'p1')] }), () => screen);
    live.touch();
    await vi.advanceTimersByTimeAsync(1000);
    expect(fetch).toHaveBeenCalledTimes(1);
    expect(draw).not.toHaveBeenCalled();
    expect(strip).not.toHaveBeenCalled();
    screen = 3; // 화면이 따라잡았다 — 다시 예약한 가져오기가 받아들여진다.
    await vi.advanceTimersByTimeAsync(1000);
    expect(fetch).toHaveBeenCalledTimes(2);
    expect(strip).toHaveBeenCalledWith(1, 0);
    expect(draw).toHaveBeenCalledTimes(1);
  });

  test('stop 뒤엔 예약된 것도 · 돌아오던 결과도 쓰지 않는다', async () => {
    const { fetch, draw, strip, live } = make(() => ({ revision: 3, issues: [] }));
    live.touch();
    live.stop();
    await vi.advanceTimersByTimeAsync(2000);
    expect(fetch).not.toHaveBeenCalled();
    const slow = make(() => ({ revision: 3, issues: [] }));
    const pending = slow.live.now();
    slow.live.stop();
    await pending;
    expect(slow.draw).not.toHaveBeenCalled();
    expect(slow.strip).not.toHaveBeenCalled();
    expect(draw).not.toHaveBeenCalled();
    expect(strip).not.toHaveBeenCalled();
  });

  test('now 는 기다리지 않고 바로 가져온다', async () => {
    const { fetch, strip, live } = make(() => ({ revision: 3, issues: [issue('warning', null)] }));
    await live.now();
    expect(fetch).toHaveBeenCalledTimes(1);
    expect(strip).toHaveBeenCalledWith(0, 1);
  });
});
