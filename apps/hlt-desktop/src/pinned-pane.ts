// 고정 창: 쪽 위에 얹는 읽기 전용 두 번째 보기. 아래에서 고쳐도 판이 바뀔 때마다 다시 그리되, 맨 위 문단을 붙잡아 스크롤이 튀지 않게 한다.
// 쪽 모양은 재현하지 않는다 — 문단마다 `<p data-id>` 한 줄(글 + 서식 구간).
import type { EditorParagraphDto } from './contracts';
import { richParagraph } from './editor-view';

export type PinnedParagraph = Pick<EditorParagraphDto, 'id' | 'text' | 'spans' | 'has_objects'>;

export type PinnedPane = {
  /** 다시 그리되 맨 위 문단을 붙잡아 스크롤 유지. */
  render(paragraphs: PinnedParagraph[]): void;
  /** `startId` 가 null 이면 문서 처음. */
  show(startId: string | null): void;
  /** 숨기고 비운다. */
  hide(): void;
  readonly shown: boolean;
  topParagraph(): string | null;
};

const HEADING = /^\s*【[^】]+】\s*$/;

/** 【청구범위】 문단 — 없으면 첫 【청구항 N】 문단 · 둘 다 없으면 `null`. */
export function claimsStart(paragraphs: { id: string; text: string }[]): string | null {
  const find = (pattern: RegExp) => paragraphs.find((paragraph) => pattern.test(paragraph.text))?.id ?? null;
  return find(/^\s*【(특허)?청구범위】\s*$/) ?? find(/^\s*【청구항\s*\d+】/);
}

export function createPinnedPane(host: HTMLElement, on: { go(id: string, caret: boolean): void }): PinnedPane {
  host.replaceChildren();
  const head = document.createElement('div');
  head.className = 'pinned-head';
  const title = document.createElement('span');
  title.textContent = '고정 창 — 읽기 전용';
  const claims = document.createElement('button');
  claims.type = 'button';
  claims.textContent = '청구범위';
  claims.title = '고정 창을 청구범위 처음으로 옮깁니다';
  head.append(title, claims);
  const body = document.createElement('div');
  body.className = 'pinned-body';
  host.append(head, body);

  let paragraphs: PinnedParagraph[] = [];
  const elementOf = (id: string) => [...body.querySelectorAll<HTMLElement>('p[data-id]')].find((each) => each.dataset.id === id);

  const pane: PinnedPane = {
    get shown() {
      return !host.hidden;
    },
    topParagraph() {
      let top: string | null = null;
      for (const each of body.querySelectorAll<HTMLElement>('p[data-id]')) {
        if (top !== null && each.offsetTop > body.scrollTop) break;
        top = each.dataset.id ?? null;
      }
      return top;
    },
    render(next) {
      const before = paragraphs;
      const held = pane.shown ? pane.topParagraph() : null;
      const delta = held === null ? 0 : body.scrollTop - (elementOf(held)?.offsetTop ?? 0);
      paragraphs = next;
      if (!pane.shown) return;
      body.replaceChildren(
        ...next.map((paragraph) => {
          const element = document.createElement('p');
          element.dataset.id = paragraph.id;
          if (HEADING.test(paragraph.text)) element.className = 'pinned-heading';
          element.append(richParagraph(paragraph.text, paragraph.spans));
          if (paragraph.has_objects) {
            const note = document.createElement('span');
            note.className = 'pinned-object';
            note.textContent = ' [표 · 그림]';
            element.append(note);
          }
          return element;
        }),
      );
      if (held === null) return;
      // 붙잡은 문단이 지워졌으면 그 앞 문단 중 남은 것.
      const keep = new Set(next.map((paragraph) => paragraph.id));
      let at = before.findIndex((paragraph) => paragraph.id === held);
      while (at > 0 && !keep.has(before[at].id)) at -= 1;
      const anchor = at >= 0 && keep.has(before[at].id) ? elementOf(before[at].id) : undefined;
      body.scrollTop = anchor ? anchor.offsetTop + (before[at].id === held ? delta : 0) : 0;
    },
    show(startId) {
      host.hidden = false;
      const list = paragraphs;
      paragraphs = [];
      pane.render(list);
      body.scrollTop = (startId !== null ? elementOf(startId)?.offsetTop : 0) ?? 0;
    },
    hide() {
      host.hidden = true;
      paragraphs = [];
      body.replaceChildren();
    },
  };

  body.addEventListener('click', (event) => {
    const element = (event.target as HTMLElement).closest<HTMLElement>('p[data-id]');
    const paragraph = paragraphs.find((each) => each.id === element?.dataset.id);
    if (paragraph) on.go(paragraph.id, !paragraph.has_objects);
  });
  claims.addEventListener('click', () => pane.show(claimsStart(paragraphs)));
  return pane;
}
