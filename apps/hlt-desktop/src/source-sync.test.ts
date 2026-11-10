// @vitest-environment jsdom
import { afterEach, describe, expect, test, vi } from 'vitest';
import type { AlignmentDto, PinDto } from './contracts';
import { createSourceSync, pairAtSource, pairAtTarget, targetRange } from './source-sync';

const pair = (target_id: string, target_sentence: number | null, source_paragraph: number, source_sentence: number | null) => ({
  target_id,
  target_sentence,
  source_paragraph,
  source_sentence,
  grade: (target_sentence === null ? 'paragraph' : 'sentence') as 'sentence' | 'paragraph',
});

const alignment = (extra: Partial<AlignmentDto> = {}): AlignmentDto => ({
  revision: 1,
  source: [
    { key: null, text: 'One. Two.', sentences: [[0, 4], [5, 9]] },
    { key: null, text: 'Three.', sentences: [[0, 6]] },
    { key: null, text: 'Four.', sentences: [[0, 5]] },
  ],
  target_sentences: [
    ['p:0', [[0, 3], [4, 7]]],
    ['p:1', [[0, 3]]],
    ['p:2', [[0, 3]]],
  ],
  pairs: [pair('p:0', 0, 0, 0), pair('p:0', 1, 0, 1), pair('p:1', null, 1, null)],
  dropped_pins: [],
  pins: 0,
  ...extra,
});

describe('짝 찾기', () => {
  test('pairAtTarget: 문장 짝 우선 · 문단 짝 · 짝 없는 문단은 null(앞 문단 것을 빌리지 않는다)', () => {
    const a = alignment();
    expect(pairAtTarget(a, 'p:0', 1)).toEqual(pair('p:0', 0, 0, 0));
    expect(pairAtTarget(a, 'p:0', 5)).toEqual(pair('p:0', 1, 0, 1));
    expect(pairAtTarget(a, 'p:1', 2)).toEqual(pair('p:1', null, 1, null));
    expect(pairAtTarget(a, 'p:2', 1)).toBeNull();
  });

  test('pairAtSource · targetRange', () => {
    const a = alignment();
    expect(pairAtSource(a, 0, 6)).toEqual(pair('p:0', 1, 0, 1));
    expect(pairAtSource(a, 1, 0)).toEqual(pair('p:1', null, 1, null));
    expect(pairAtSource(a, 2, 0)).toBeNull();
    expect(targetRange(a, pair('p:0', 1, 0, 1))).toEqual({ paragraph: 'p:0', start: 4, end: 7 });
    expect(targetRange(a, pair('p:1', null, 1, null))).toEqual({ paragraph: 'p:1', start: 0, end: Number.POSITIVE_INFINITY });
  });
});

describe('원문 창', () => {
  afterEach(() => vi.restoreAllMocks());

  function make(extra: { caret?: { paragraph: string; offset: number } | null; pin?: (pin: PinDto) => Promise<AlignmentDto>; visible?: boolean } = {}) {
    const pane = document.createElement('div');
    document.body.append(pane);
    const state = { caret: extra.caret === undefined ? { paragraph: 'p:0', offset: 1 } : extra.caret };
    const log = { go: [] as unknown[], mark: [] as [unknown, unknown][], say: [] as string[], pin: [] as PinDto[] };
    const sync = createSourceSync({
      pane,
      caret: () => state.caret,
      go: (paragraph, offset) => log.go.push([paragraph, offset]),
      markTarget: (range, grade) => log.mark.push([range, grade]),
      pin: extra.pin ?? (async (pin) => (log.pin.push(pin), alignment())),
      say: (text) => log.say.push(text),
      paragraphs: () => ['p:0', 'p:1', 'p:2', 'p:3'],
      caretVisible: () => extra.visible ?? false,
    });
    return { pane, state, log, sync };
  }
  const span = (pane: HTMLElement, p: number, s: number) => pane.querySelector<HTMLElement>(`p[data-p="${p}"] [data-s="${s}"]`)!;

  test('문장 짝: 원문 문장 .current · 스크롤 · 역문 문장 칠하기 / 문단 짝: 점선 문단 · 문단 칠하기 / 짝 없음: 칠하지 않는다', () => {
    vi.spyOn(HTMLElement.prototype, 'offsetTop', 'get').mockImplementation(function (this: HTMLElement) {
      return this.dataset.s === '1' ? 500 : 0;
    });
    const { pane, state, sync, log } = make({ caret: { paragraph: 'p:0', offset: 5 } });
    sync.open(alignment());
    expect(span(pane, 0, 1).classList.contains('current')).toBe(true);
    expect(pane.scrollTop).toBe(500);
    expect(log.mark.at(-1)).toEqual([{ paragraph: 'p:0', start: 4, end: 7 }, 'sentence']);

    state.caret = { paragraph: 'p:1', offset: 0 };
    sync.follow();
    expect(pane.querySelectorAll('.current').length).toBe(0);
    expect(pane.querySelector('p[data-p="1"]')!.classList.contains('current-para')).toBe(true);
    expect(log.mark.at(-1)).toEqual([{ paragraph: 'p:1', start: 0, end: Number.POSITIVE_INFINITY }, 'paragraph']);

    state.caret = { paragraph: 'p:2', offset: 0 };
    sync.follow();
    expect(pane.querySelectorAll('.current, .current-para').length).toBe(0);
    expect(log.mark.at(-1)![0]).toBeNull();
    state.caret = null;
    sync.follow();
    expect(log.mark.at(-1)![0]).toBeNull();
  });

  test('원문 문장을 누르면 역문 짝 문장 처음으로 · 짝 없는 문장은 아무 데도 가지 않는다', () => {
    const { pane, sync, log } = make();
    sync.open(alignment());
    span(pane, 0, 1).click();
    expect(log.go).toEqual([['p:0', 4]]);
    span(pane, 2, 0).click();
    expect(log.go).toHaveLength(1);
  });

  test('⌥-누르기: 지금 커서 문장과 그 원문 문장으로 고정 · 돌아온 정렬로 다시 그린다', async () => {
    const { pane, sync, log } = make({ caret: { paragraph: 'p:2', offset: 1 } });
    sync.open(alignment());
    span(pane, 2, 0).dispatchEvent(new MouseEvent('click', { bubbles: true, altKey: true }));
    await Promise.resolve();
    await Promise.resolve();
    expect(log.pin).toEqual([{ target_id: 'p:2', target_sentence: 0, source_paragraph: 2, source_sentence: 0 }]);
    expect(log.go).toEqual([]);
  });

  test('고정이 적용되지 않으면(dropped_pins) 알린다 · 커서가 없으면 먼저 커서를 두라고 한다', async () => {
    const dropped: PinDto = { target_id: 'p:2', target_sentence: 0, source_paragraph: 2, source_sentence: 0 };
    const { pane, sync, log } = make({ caret: { paragraph: 'p:2', offset: 1 }, pin: async () => alignment({ dropped_pins: [dropped] }) });
    sync.open(alignment());
    span(pane, 2, 0).dispatchEvent(new MouseEvent('click', { bubbles: true, altKey: true }));
    await new Promise((done) => setTimeout(done, 0));
    expect(log.say.at(-1)).toBe('이 고정은 적용되지 않았습니다(다른 청구항이거나 앞뒤 고정과 차례가 어긋납니다).');

    const none = make({ caret: null });
    none.sync.open(alignment());
    span(none.pane, 0, 0).dispatchEvent(new MouseEvent('click', { bubbles: true, altKey: true }));
    expect(none.log.say.at(-1)).toBe('먼저 역문 문장에 커서를 두세요.');
    expect(none.log.pin).toEqual([]);
  });

  test('update: 원문 칸 스크롤 자리를 지킨다 · 원문 칸에 편집 가능한 요소가 없다', () => {
    const { pane, sync } = make({ caret: null });
    sync.open(alignment());
    pane.scrollTop = 123;
    sync.update(alignment({ revision: 2 }));
    expect(pane.scrollTop).toBe(123);
    expect(pane.querySelectorAll('input, textarea, [contenteditable]').length).toBe(0);
  });

  test('짝 없는 문단에 커서: 칠하지 않고 앞쪽 가장 가까운 짝 문단 자리로 원문 칸만 스크롤한다', () => {
    vi.spyOn(HTMLElement.prototype, 'offsetTop', 'get').mockImplementation(function (this: HTMLElement) {
      return this.dataset.p === '1' ? 400 : 0;
    });
    const { pane, sync, log } = make({ caret: { paragraph: 'p:3', offset: 0 } }); // p:3 · p:2 모두 짝 없음 → 앞의 p:1 짝
    sync.open(alignment({ target_sentences: [...alignment().target_sentences, ['p:3', [[0, 3]]]] }));
    expect(pane.scrollTop).toBe(400);
    expect(pane.querySelectorAll('.current, .current-para')).toHaveLength(0);
    expect(log.mark.at(-1)![0]).toBeNull();
  });

  test('followTop: 맨 위 문단의 짝 자리로 칠하지 않고 스크롤 · 커서가 보이면 아무것도 하지 않는다', () => {
    vi.spyOn(HTMLElement.prototype, 'offsetTop', 'get').mockImplementation(function (this: HTMLElement) {
      return this.dataset.p === '1' ? 400 : 0;
    });
    const hidden = make({ caret: null });
    hidden.sync.open(alignment());
    hidden.sync.followTop('p:1');
    expect(hidden.pane.scrollTop).toBe(400);
    expect(hidden.pane.querySelectorAll('.current, .current-para')).toHaveLength(0);
    const visible = make({ caret: { paragraph: 'p:0', offset: 0 }, visible: true });
    visible.sync.open(alignment());
    visible.pane.scrollTop = 7;
    visible.sync.followTop('p:1');
    expect(visible.pane.scrollTop).toBe(7);
  });

  test('R-4a: followTop 으로 내려간 뒤 커서가 그대로인 follow() 는 스크롤하지 않고 강조만 유지 · 커서가 바뀌면 그 문장으로 간다', () => {
    vi.spyOn(HTMLElement.prototype, 'offsetTop', 'get').mockImplementation(function (this: HTMLElement) {
      return this.dataset.s === '1' ? 500 : this.dataset.p === '1' ? 900 : 0;
    });
    const { pane, state, sync } = make({ caret: { paragraph: 'p:0', offset: 5 } });
    sync.open(alignment());
    expect(pane.scrollTop).toBe(500);
    sync.followTop('p:1'); // 커서가 안 보임 → 맨 위 문단 쪽으로
    expect(pane.scrollTop).toBe(900);
    sync.follow(); // 쪽을 다시 그려서 불린 follow — 커서는 그대로
    expect(pane.scrollTop).toBe(900);
    expect(span(pane, 0, 1).classList.contains('current')).toBe(true);
    state.caret = { paragraph: 'p:0', offset: 1 };
    sync.follow();
    expect(pane.scrollTop).toBe(0);
  });

  test('짝 없는 원문 문장을 누르면 ⌥클릭 안내', () => {
    const { pane, sync, log } = make();
    sync.open(alignment());
    span(pane, 2, 0).click();
    expect(log.say.at(-1)).toBe('이 문장은 짝이 없습니다 — ⌥클릭으로 맞출 수 있습니다.');
  });

  test('invalidate: 고친 뒤 정렬이 새로 올 때까지 칠한 것을 거두고 따라가지 않는다 · update 가 되살린다', () => {
    const { pane, sync, log, state } = make({ caret: { paragraph: 'p:0', offset: 1 } });
    sync.open(alignment());
    expect(pane.querySelectorAll('.current')).toHaveLength(1);
    sync.invalidate();
    expect(pane.querySelectorAll('.current, .current-para')).toHaveLength(0);
    expect(log.mark.at(-1)![0]).toBeNull();
    state.caret = { paragraph: 'p:0', offset: 5 };
    sync.follow();
    expect(pane.querySelectorAll('.current')).toHaveLength(0);
    sync.update(alignment({ revision: 2 }));
    expect(pane.querySelectorAll('.current')).toHaveLength(1);
  });

  test('close: 칠한 것을 거둔다', () => {
    const { pane, sync, log } = make();
    sync.open(alignment());
    sync.close();
    expect(sync.shown).toBe(false);
    expect(pane.children.length).toBe(0);
    expect(log.mark.at(-1)![0]).toBeNull();
  });
});
