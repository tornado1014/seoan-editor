// 원문 창: 커서가 있는 역문 문장의 원문 문장을 칠하고(문장 짝 → 문장 · 문단 짝 → 점선 문단 · 짝 없음 → 칠하지 않음),
// 원문 문장을 누르면 역문의 짝 문장으로 가고, ⌥-누르기 · 우클릭으로 손 고정을 건다. 오프셋은 모두 UTF-16 코드 단위(Rust 가 바꿔 줬다).
import type { AlignmentDto, PairDto, PinDto } from './contracts';

type Range = [number, number];

/** `offset` 이 든 문장 번호(문장 끝 바로 뒤 자리는 그 문장 · 다음 문장 시작과 겹치면 다음 문장). 없으면 -1. */
function sentenceAt(ranges: Range[], offset: number): number {
  const inside = ranges.findIndex(([start, end]) => offset >= start && offset < end);
  return inside >= 0 ? inside : ranges.findIndex(([, end]) => offset === end);
}

const rangesOf = (a: AlignmentDto, paragraph: string): Range[] => a.target_sentences.find(([id]) => id === paragraph)?.[1] ?? [];

/** 커서 → 짝. 문장 짝이 우선, 없으면 그 문단의 문단 짝, 그것도 없으면 `null`(앞 문단 것을 빌리지 않는다). */
export function pairAtTarget(a: AlignmentDto, paragraph: string, offset: number): PairDto | null {
  const at = sentenceAt(rangesOf(a, paragraph), offset);
  const own = a.pairs.filter((pair) => pair.target_id === paragraph);
  return (
    (at >= 0 ? own.find((pair) => pair.grade === 'sentence' && pair.target_sentence === at) : undefined) ??
    own.find((pair) => pair.grade === 'paragraph') ??
    null
  );
}

/** 원문 문장(문단 번호 · 그 문단 안 오프셋) → 짝. 규칙은 `pairAtTarget` 과 같다. */
export function pairAtSource(a: AlignmentDto, sourceParagraph: number, offset: number): PairDto | null {
  const at = sentenceAt(a.source[sourceParagraph]?.sentences ?? [], offset);
  const own = a.pairs.filter((pair) => pair.source_paragraph === sourceParagraph);
  return (
    (at >= 0 ? own.find((pair) => pair.grade === 'sentence' && pair.source_sentence === at) : undefined) ??
    own.find((pair) => pair.grade === 'paragraph') ??
    null
  );
}

/** 짝의 역문 쪽 범위. 문단 짝은 문단 처음부터 끝까지(`end` = 무한대 — 쪽 줄 길이로 잘린다). */
export function targetRange(a: AlignmentDto, pair: PairDto): { paragraph: string; start: number; end: number } | null {
  if (pair.grade === 'paragraph' || pair.target_sentence === null) {
    return { paragraph: pair.target_id, start: 0, end: Number.POSITIVE_INFINITY };
  }
  const range = rangesOf(a, pair.target_id)[pair.target_sentence];
  return range ? { paragraph: pair.target_id, start: range[0], end: range[1] } : null;
}

const same = (a: PinDto, b: PinDto) =>
  a.target_id === b.target_id &&
  a.target_sentence === b.target_sentence &&
  a.source_paragraph === b.source_paragraph &&
  a.source_sentence === b.source_sentence;

export function createSourceSync(o: {
  pane: HTMLElement;
  caret(): { paragraph: string; offset: number } | null;
  go(paragraph: string, offset: number): void;
  markTarget(range: { paragraph: string; start: number; end: number } | null, grade: 'sentence' | 'paragraph'): void;
  pin(pin: PinDto): Promise<AlignmentDto>;
  say(text: string): void;
  /** 열 때 · 다시 정렬했을 때 · 손 고정 뒤(머리 글의 수를 다시 그리라고). */
  changed?(a: AlignmentDto): void;
  /** 원문 문장 우클릭 메뉴(없으면 우클릭은 그냥 둔다). */
  menu?(items: { text: string; action(): void }[]): void;
}) {
  let alignment: AlignmentDto | null = null;
  let shown = false;
  let lastKey = '';

  function render(a: AlignmentDto) {
    o.pane.replaceChildren(
      ...a.source.map((paragraph, index) => {
        const element = document.createElement('p');
        element.dataset.p = String(index);
        let at = 0;
        paragraph.sentences.forEach(([start, end], sentence) => {
          if (start > at) element.append(paragraph.text.slice(at, start));
          const span = document.createElement('span');
          span.dataset.s = String(sentence);
          span.textContent = paragraph.text.slice(start, end);
          element.append(span);
          at = end;
        });
        if (at < paragraph.text.length) element.append(paragraph.text.slice(at));
        return element;
      }),
    );
  }

  /** 보이는 자리 밖이면 가운데쯤으로(보이면 그대로 — 원문을 누른 사람의 눈을 흔들지 않는다). */
  function reveal(element: HTMLElement) {
    const top = element.offsetTop;
    const view = o.pane.clientHeight;
    if (top < o.pane.scrollTop || top + element.offsetHeight > o.pane.scrollTop + view) {
      o.pane.scrollTop = Math.max(0, top - view / 3);
    }
  }

  function clear() {
    o.pane.querySelector('.current')?.classList.remove('current');
    o.pane.querySelector('.current-para')?.classList.remove('current-para');
  }

  function follow() {
    if (!shown || !alignment) return;
    const caret = o.caret();
    const pair = caret ? pairAtTarget(alignment, caret.paragraph, caret.offset) : null;
    const key = pair ? `${pair.grade}:${pair.source_paragraph}:${pair.source_sentence}` : '';
    const range = pair ? targetRange(alignment, pair) : null;
    o.markTarget(range, pair?.grade ?? 'sentence');
    if (!pair) {
      clear();
      lastKey = '';
      return;
    }
    const paragraph = o.pane.querySelector<HTMLElement>(`p[data-p="${pair.source_paragraph}"]`);
    const sentence =
      pair.grade === 'sentence' ? paragraph?.querySelector<HTMLElement>(`[data-s="${pair.source_sentence}"]`) : null;
    const element = sentence ?? paragraph;
    if (!element) return;
    if (!element.classList.contains('current') && !element.classList.contains('current-para')) {
      clear();
      element.classList.add(sentence ? 'current' : 'current-para');
    }
    if (key !== lastKey) reveal(element);
    lastKey = key;
  }

  async function pinHere(sentence: HTMLElement | null, paragraphIndex: number) {
    const caret = o.caret();
    const at = caret && alignment ? sentenceAt(rangesOf(alignment, caret.paragraph), caret.offset) : -1;
    if (!caret || at < 0) return o.say('먼저 역문 문장에 커서를 두세요.');
    if (!sentence) return o.say('고정할 원문 문장을 눌러 주세요.');
    const pin = { target_id: caret.paragraph, target_sentence: at, source_paragraph: paragraphIndex, source_sentence: Number(sentence.dataset.s) };
    try {
      const next = await o.pin(pin);
      update(next);
      o.say(next.dropped_pins.some((dropped) => same(dropped, pin)) ? '이 고정은 적용되지 않았습니다(다른 청구항이거나 앞뒤 고정과 차례가 어긋납니다).' : '');
    } catch (error) {
      o.say(`손 고정에 실패했습니다 — ${error instanceof Error ? error.message : String(error)}`);
    }
  }

  const spot = (event: Event) => {
    const target = event.target as HTMLElement;
    const paragraph = target.closest<HTMLElement>('p[data-p]');
    return { paragraph, sentence: target.closest<HTMLElement>('[data-s]'), index: Number(paragraph?.dataset.p) };
  };
  o.pane.addEventListener('click', (event) => {
    if (!alignment) return;
    const { paragraph, sentence, index } = spot(event);
    if (!paragraph) return;
    if ((event as MouseEvent).altKey) return void pinHere(sentence, index);
    const start = sentence ? alignment.source[index].sentences[Number(sentence.dataset.s)][0] : 0;
    const pair = pairAtSource(alignment, index, start);
    const range = pair ? targetRange(alignment, pair) : null;
    if (range) o.go(range.paragraph, range.start);
  });
  o.pane.addEventListener('contextmenu', (event) => {
    const { paragraph, sentence, index } = spot(event);
    if (!o.menu || !paragraph || !sentence) return;
    event.preventDefault();
    o.menu([{ text: '이 문장에 맞추기', action: () => void pinHere(sentence, index) }]);
  });

  function update(a: AlignmentDto) {
    const scroll = o.pane.scrollTop;
    alignment = a;
    render(a);
    o.pane.scrollTop = scroll;
    o.changed?.(a);
    follow();
  }

  return {
    open(a: AlignmentDto) {
      shown = true;
      lastKey = '';
      alignment = a;
      render(a);
      o.pane.scrollTop = 0;
      o.changed?.(a);
      follow();
    },
    update,
    follow,
    close() {
      shown = false;
      alignment = null;
      lastKey = '';
      o.pane.replaceChildren();
      o.markTarget(null, 'sentence');
    },
    get shown() {
      return shown;
    },
  };
}
