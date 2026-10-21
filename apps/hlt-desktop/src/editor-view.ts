// 편집 화면 쪽 그리기(P5). 쪽 계획(`page_plan`)의 글자·그림 자리를 그대로 놓는다 — 인쇄와 같은 배치.
import type { PlannedPageDto } from './contracts';

export const BOLD = 2;
export const ITALIC = 4;
export const UNDERLINE = 1;

/** 쪽 하나를 그린다. `imageUrl` = 그림 객체 코드 → Blob URL(없으면 빈 상자). */
export function renderPage(
  page: PlannedPageDto,
  scale: number,
  imageUrl: (objectCode: number) => string | null,
  selected: string | null,
): DocumentFragment {
  const fragment = document.createDocumentFragment();
  // 표 칸 테두리 — 글자보다 먼저(아래) 깐다. 빈 칸은 상자만 있으니 상자도 누를 수 있게 칸 id 를 단다.
  for (const box of page.cells ?? []) {
    const element = document.createElement('div');
    element.className = 'nk-cell';
    element.dataset.cell = box.cell;
    place(element, box.x, box.top, scale);
    element.style.width = `${box.width * scale}px`;
    element.style.height = `${box.height * scale}px`;
    fragment.append(element);
  }
  for (const image of page.images) {
    const element = document.createElement('img');
    element.className = 'nk-image';
    element.dataset.image = image.image;
    const url = imageUrl(image.object_code);
    if (url) element.src = url;
    place(element, image.x, image.top, scale);
    element.style.width = `${image.width * scale}px`;
    element.style.height = `${image.height * scale}px`;
    fragment.append(element);
  }
  for (const line of page.lines) {
    const row = document.createElement('div');
    row.className = 'nk-line';
    if (line.paragraph) row.dataset.paragraph = line.paragraph;
    if (line.cell) row.dataset.cell = line.cell;
    if (line.paragraph && line.paragraph === selected) row.classList.add('selected');
    for (const glyph of line.glyphs) {
      const span = document.createElement('span');
      span.className = 'nk-glyph';
      span.textContent = glyph.ch;
      place(span, glyph.x, line.top + glyph.drop, scale);
      span.style.fontSize = `${glyph.size * scale}px`;
      applyStyle(span, glyph.style);
      row.append(span);
    }
    // 밑줄은 줄 단위 구간으로 — 글자마다 그리면 공백(글리프가 없다)에서 끊긴다. 위치 = 기준선(0.859em) + 0.12em · 두께 0.05em(PDF 와 같다).
    for (const [from, to] of line.underlines ?? []) {
      const bar = document.createElement('div');
      bar.className = 'nk-underline';
      place(bar, from, line.top + 12 * (0.859 + 0.12), scale);
      bar.style.width = `${(to - from) * scale}px`;
      bar.style.height = `${Math.max(1, 12 * 0.05 * scale)}px`;
      row.append(bar);
    }
    fragment.append(row);
  }
  return fragment;
}

/** 바탕체엔 굵은·기울인 글꼴이 없다 — 굵게는 운영체제가 합성한 굵은 꼴(통특기도 같다) · 기울임은 0.34 사선으로 직접 그린다.
 * ⛔ 윤곽선(`-webkit-text-stroke`)으로 굵게를 흉내 내면 획 안팎으로 번져 지저분하다(2026-10-16 사용자 화면 두 번 —
 * 0.04em 은 이웃을 덮었고 0.03em 도 뭉갰다). 전역 `font-synthesis: none` 을 굵은 글자에서만 푼다(자리는 고정이라 폭은 무관). */
export function applyStyle(element: HTMLElement, style: number) {
  if (style & BOLD) {
    element.style.fontWeight = 'bold';
    element.style.setProperty('font-synthesis', 'weight');
  }
  if (style & ITALIC) {
    element.style.display = 'inline-block';
    element.style.transform = 'skewX(-18.8deg)';
    element.style.transformOrigin = '0 85%';
  }
}

function place(element: HTMLElement, x: number, top: number, scale: number) {
  element.style.left = `${x * scale}px`;
  element.style.top = `${top * scale}px`;
}

/** 구조 창 마디: 절 표제(【…】) · 맨 위는 서식 이름(통특기 트리 뿌리 「특허 명세서등(국어)」 · 「PCT특허」). */
export interface OutlineNode {
  id: string | null;
  label: string;
  children: OutlineNode[];
}

/** 큰 절(뿌리 바로 아래). */
const TOP = new Set(['발명의 설명', '청구범위', '특허청구범위', '요약서', '도면']);
/** 둘째 절 아래 셋째 절: 【발명의 내용】 · 【선행기술문헌】 의 아래 칸. */
const THIRD: Record<string, Set<string>> = {
  '발명의 내용': new Set(['해결하고자 하는 과제', '해결하려는 과제', '과제의 해결 수단', '발명의 효과']),
  선행기술문헌: new Set(['특허문헌', '비특허문헌']),
};
/** 식별항목(통특기 우클릭 「식별항목」 7종 + 번호) — 그때 열린 가장 깊은 절 아래. */
const ITEM = /^(도|표|청구항|수학식|화학식|반응식|참고도) [A-Za-z0-9]+$/;

/** 구조 창 나무(통특기 왼쪽 트리처럼 큰 절 → 절 → 아래 절 · 식별항목). */
export function outlineTree(title: string, paragraphs: { id: string; text: string }[]): OutlineNode {
  const root: OutlineNode = { id: null, label: title, children: [] };
  let top: OutlineNode | null = null;
  let second: OutlineNode | null = null;
  let third: OutlineNode | null = null;
  for (const paragraph of paragraphs) {
    if (!/^\s*【[^】]+】\s*$/.test(paragraph.text)) continue;
    const node: OutlineNode = { id: paragraph.id, label: paragraph.text.trim().slice(1, -1), children: [] };
    if (TOP.has(node.label)) {
      root.children.push(node);
      [top, second, third] = [node, null, null];
    } else if (ITEM.test(node.label)) {
      (third ?? second ?? top ?? root).children.push(node);
    } else if (second && THIRD[second.label]?.has(node.label)) {
      second.children.push(node);
      third = node;
    } else {
      (top ?? root).children.push(node);
      [second, third] = [node, null];
    }
  }
  return root;
}

export const SUPERSCRIPT = 16;
export const SUBSCRIPT = 32;

/** 편집 창: 문단 글을 서식대로 보이는 조각으로(글자 하나하나의 서식을 모아 같은 서식끼리 묶는다). */
export function richParagraph(
  text: string,
  spans: { start: number; end: number; style: number; size?: number }[],
): DocumentFragment {
  const chars = Array.from(text);
  const style = new Array<number>(chars.length).fill(0);
  const size = new Array<number | undefined>(chars.length).fill(undefined);
  for (const span of spans) {
    for (let at = span.start; at < Math.min(span.end, chars.length); at += 1) {
      style[at] |= span.style;
      if (span.size) size[at] = span.size;
    }
  }
  const fragment = document.createDocumentFragment();
  let at = 0;
  while (at < chars.length) {
    let end = at + 1;
    while (end < chars.length && style[end] === style[at] && size[end] === size[at]) end += 1;
    const piece = chars.slice(at, end).join('');
    if (style[at] === 0 && !size[at]) {
      fragment.append(document.createTextNode(piece));
    } else {
      const element = document.createElement(
        style[at] & SUPERSCRIPT ? 'sup' : style[at] & SUBSCRIPT ? 'sub' : 'span',
      );
      element.textContent = piece;
      applyStyle(element, style[at]);
      if (size[at]) element.style.fontSize = `${(size[at]! / 12) * 100}%`;
      fragment.append(element);
    }
    at = end;
  }
  return fragment;
}

/** 편집 창 선택 → 글자 자리 [start, end)(글자 = 유니코드 글자 하나). 창 밖 선택이면 null. */
export function selectionOffsets(root: HTMLElement): [number, number] | null {
  const selection = root.ownerDocument.getSelection();
  if (!selection || selection.rangeCount === 0) return null;
  const range = selection.getRangeAt(0);
  if (!root.contains(range.startContainer) || !root.contains(range.endContainer)) return null;
  const length = (container: Node, offset: number) => {
    const before = root.ownerDocument.createRange();
    before.setStart(root, 0);
    before.setEnd(container, offset);
    return Array.from(before.toString()).length;
  };
  return [length(range.startContainer, range.startOffset), length(range.endContainer, range.endOffset)];
}

/** 편집 창 선택을 글자 자리로 되돌린다(서식을 건 뒤 같은 글자를 다시 고른 상태로). */
export function selectOffsets(root: HTMLElement, start: number, end: number) {
  const walker = root.ownerDocument.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  const range = root.ownerDocument.createRange();
  let seen = 0;
  let node: Node | null;
  let startSet = false;
  let last: Node | null = null;
  while ((node = walker.nextNode())) {
    last = node;
    const chars = Array.from(node.textContent ?? '');
    const units = (count: number) => chars.slice(0, count).join('').length;
    if (!startSet && seen + chars.length >= start) {
      range.setStart(node, units(start - seen));
      startSet = true;
    }
    if (startSet && seen + chars.length >= end) {
      range.setEnd(node, units(end - seen));
      const selection = root.ownerDocument.getSelection();
      selection?.removeAllRanges();
      selection?.addRange(range);
      return;
    }
    seen += chars.length;
  }
  // 끝이 글 밖이면 글 끝까지.
  if (startSet && last) {
    range.setEnd(last, (last.textContent ?? '').length);
    const selection = root.ownerDocument.getSelection();
    selection?.removeAllRanges();
    selection?.addRange(range);
  }
}
