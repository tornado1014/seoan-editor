// 편집 화면 쪽 그리기(P5). 쪽 계획(`page_plan`)의 글자·그림 자리를 그대로 놓는다 — 인쇄와 같은 배치.
import type { CharacterFormatDto, PlannedPageDto } from './contracts';

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
    // 그린 크기(pt) — 「그림 크기…」가 mm 로 바꿔 보여 준다.
    element.dataset.width = String(image.width);
    element.dataset.height = String(image.height);
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
  /** 이름 뒤 작은 글(청구항 나무: 다중 인용 · 종속항 수). */
  note?: string;
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

/** 문단마다 큰 절: 【발명의 설명】 → 상세설명 · 【청구범위】(【특허청구범위】) → 청구항 · 【요약서】 · 【도면】 · 첫 큰 절 앞 → 그 밖. */
export function sectionOf(paragraphs: { id: string; text: string }[]): Map<string, 'description' | 'claims' | 'other'> {
  const out = new Map<string, 'description' | 'claims' | 'other'>();
  let section: 'description' | 'claims' | 'other' = 'other';
  for (const paragraph of paragraphs) {
    const heading = /^\s*【([^】]+)】\s*$/.exec(paragraph.text)?.[1];
    if (heading && TOP.has(heading)) {
      section = heading === '발명의 설명' ? 'description' : heading === '청구범위' || heading === '특허청구범위' ? 'claims' : 'other';
    }
    out.set(paragraph.id, section);
  }
  return out;
}

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

const CLAIM = /^청구항 (\d+)$/;

/** 청구항 나무: 형제로 늘어선 「청구항 N」을 첫 인용 항 아래로 옮긴다(`parents` = 항 → 인용하는 앞 항 · 빈 = 독립항).
 *  다중 인용은 「제1·3항」, 아래 종속항이 있으면 「종속 N」을 이름 뒤에 단다. 고친 나무를 돌려준다(제자리). */
export function nestClaims(node: OutlineNode, parents: Record<string, number[]>): OutlineNode {
  const claims = new Map<number, OutlineNode>();
  for (const child of node.children) {
    const number = CLAIM.exec(child.label)?.[1];
    if (number) claims.set(Number(number), child);
  }
  if (claims.size) {
    node.children = node.children.filter((child) => {
      const number = Number(CLAIM.exec(child.label)?.[1]);
      const cited = parents[number] ?? [];
      if (cited.length > 1) child.note = `제${cited.join('·')}항`;
      const parent = claims.get(cited[0]);
      if (!parent) return true;
      parent.children.push(child);
      return false;
    });
    const count = (claim: OutlineNode): number =>
      claim.children.reduce((sum, child) => sum + (CLAIM.test(child.label) ? 1 + count(child) : 0), 0);
    for (const claim of claims.values()) {
      const below = count(claim);
      if (below) claim.note = [claim.note, `종속 ${below}`].filter(Boolean).join(' · ');
    }
  }
  for (const child of node.children) if (!CLAIM.test(child.label)) nestClaims(child, parents);
  return node;
}

/** 되풀이 문장 하나: 글 · 나온 문단들(차례대로) · 청구항 본문 통째인가. */
export interface Repeated {
  text: string;
  at: string[];
  claim: boolean;
}

/** 되풀이 문장 찾기: 상세설명 문장(「.」 뒤가 끊기는 곳에서 나눔 · 공백 정리 · 20자 넘는 것)이 두 번 넘게 나온 것,
 *  그리고 본문이 통째로 같은 청구항. 많이 나온 것부터. */
export function repeatedSentences(paragraphs: { id: string; text: string }[]): Repeated[] {
  const sections = sectionOf(paragraphs);
  const heading = (text: string) => /^\s*【[^】]+】\s*$/.test(text);
  const tidy = (text: string) => text.replace(/\s+/g, ' ').trim();
  const sentences = new Map<string, string[]>();
  const claims = new Map<string, string[]>();
  let claim: { id: string; body: string[] } | null = null;
  const closeClaim = () => {
    if (!claim) return;
    const body = tidy(claim.body.join(' '));
    if (body && body !== '삭제') claims.set(body, [...(claims.get(body) ?? []), claim.id]);
    claim = null;
  };
  for (const paragraph of paragraphs) {
    const section = sections.get(paragraph.id);
    if (heading(paragraph.text)) {
      closeClaim();
      if (/^\s*【청구항 [^】]+】\s*$/.test(paragraph.text)) claim = { id: paragraph.id, body: [] };
      continue;
    }
    if (section === 'claims') {
      claim?.body.push(paragraph.text);
      continue;
    }
    if (section !== 'description') continue;
    for (const piece of paragraph.text.split(/(?<=\.)(?=\s|$)/)) {
      const sentence = tidy(piece);
      if (Array.from(sentence).length <= 20) continue;
      sentences.set(sentence, [...(sentences.get(sentence) ?? []), paragraph.id]);
    }
  }
  closeClaim();
  const pick = (map: Map<string, string[]>, claim: boolean) =>
    [...map].filter(([, at]) => at.length > 1).map(([text, at]) => ({ text, at, claim }));
  return [...pick(sentences, false), ...pick(claims, true)].sort((a, b) => b.at.length - a.at.length);
}

/** 문서 통계(이름 · 값): 쪽 · 청구항(독립 · 종속 · 삭제) · 상세설명 문단 · 글자 · 도면 · 요약 글자, 그리고 요금이 붙는 기준.
 *  `parents` = 청구항 → 인용하는 앞 항(빈 = 독립항). 금액은 쓰지 않는다(요금표가 바뀐다). */
export function documentStats(
  paragraphs: { id: string; text: string }[],
  pageCount: number,
  parents: Record<string, number[]>,
): [string, string][] {
  const sections = sectionOf(paragraphs);
  const heading = (text: string) => /^\s*【([^】]+)】\s*$/.exec(text)?.[1];
  const visible = (text: string) => Array.from(text.replace(/\s/g, '')).length;
  let claims = 0;
  let deleted = 0;
  let figures = 0;
  let body = 0;
  let characters = 0;
  let spaced = 0;
  let abstract = 0;
  let inAbstract = false;
  paragraphs.forEach((paragraph, at) => {
    const name = heading(paragraph.text);
    if (name) {
      inAbstract = name === '요약';
      if (/^청구항 \d+$/.test(name)) {
        claims += 1;
        if (paragraphs[at + 1]?.text.trim() === '삭제') deleted += 1;
      }
      if (/^도 \S+$/.test(name)) figures += 1;
      return;
    }
    const section = sections.get(paragraph.id);
    if (inAbstract) abstract += visible(paragraph.text);
    // 그림 자리만 있는 문단(「_」)은 글자가 아니다.
    if ((section === 'description' || section === 'claims') && paragraph.text.trim() !== '_') {
      characters += visible(paragraph.text);
      spaced += Array.from(paragraph.text).length;
    }
    if (section === 'description' && paragraph.text.trim() && paragraph.text.trim() !== '_') body += 1;
  });
  const live = claims - deleted;
  // 삭제한 항은 인용이 없어 독립항으로 잡힌다 — 빼고 센다. 나무를 아직 못 받았으면 수만.
  const independent = Object.values(parents).filter((cited) => cited.length === 0).length - deleted;
  const kinds = Object.keys(parents).length ? ` · 독립 ${independent} · 종속 ${live - independent}` : '';
  const over = Math.max(0, pageCount - 20);
  return [
    ['쪽 수', `${pageCount}쪽`],
    ['청구항', `${claims}개${deleted ? `(삭제 ${deleted})` : ''}${kinds}`],
    ['상세설명 문단', `${body}개`],
    ['글자 수(상세설명 · 청구항)', `${characters.toLocaleString()}자(공백 빼고) · ${spaced.toLocaleString()}자(공백 포함)`],
    ['도면', `${figures}개`],
    ['요약', `${abstract}자(공백 빼고 · 권장 400자)`],
    ['요금이 붙는 기준', `20쪽 넘는 쪽 ${over}쪽 · 청구항 ${live}개 — 금액은 특허로 요금표로 확인`],
  ];
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

const BLOCK = new Set(['P', 'DIV', 'LI', 'H1', 'H2', 'H3', 'H4', 'H5', 'H6', 'TR', 'TABLE', 'UL', 'OL', 'BLOCKQUOTE']);
const SKIP = new Set(['STYLE', 'SCRIPT', 'HEAD', 'TITLE', 'META', 'XML']);

/** 클립보드 `text/html`(워드 · 서안 복사) → 서식 런. `\n` = 문단 나누기. 글자 서식이 하나도 없으면 null(그냥 글 붙여넣기). */
export function htmlRuns(html: string): [string, CharacterFormatDto[]][] | null {
  const body = new DOMParser().parseFromString(html, 'text/html').body;
  const runs: [string, CharacterFormatDto[]][] = [];
  let formatted = false;
  const push = (text: string, formats: CharacterFormatDto[]) => {
    const last = runs.at(-1);
    if (last && last[1].join() === formats.join()) last[0] += text;
    else runs.push([text, formats]);
  };
  // 글자 수(줄바꿈 빼고) — 빈 문단(워드 `<p><o:p>&nbsp;</o:p></p>` · 서안 `<p></p>`)을 알아보는 데 쓴다.
  let written = 0;
  const lineOpen = () => {
    const last = runs.at(-1);
    return !!last && !last[0].endsWith('\n');
  };
  const walk = (node: Node, inherited: CharacterFormatDto[]) => {
    if (node.nodeType === Node.TEXT_NODE) {
      const text = (node.textContent ?? '').replace(/[\s\u00a0]+/g, ' ');
      if (!text.trim() && !(runs.at(-1)?.[0].match(/\S$/))) return;
      if (inherited.length > 0 && text.trim()) formatted = true;
      written += text.length;
      return push(text, inherited);
    }
    if (!(node instanceof Element) || SKIP.has(node.tagName) || node.tagName.includes(':')) return;
    if (node.tagName === 'BR') return push('\n', []);
    const formats = new Set(inherited);
    const tag = node.tagName;
    const style = (node.getAttribute('style') ?? '').toLowerCase();
    if (tag === 'B' || tag === 'STRONG' || /font-weight:\s*(bold|[6-9]00)/.test(style)) formats.add('bold');
    if (tag === 'I' || tag === 'EM' || /font-style:\s*italic/.test(style)) formats.add('italic');
    if (tag === 'U' || /text-decoration[^;]*underline/.test(style)) formats.add('underline');
    const sup = tag === 'SUP' || /vertical-align:\s*super/.test(style);
    const sub = tag === 'SUB' || /vertical-align:\s*sub/.test(style);
    if (sup) formats.delete('subscript'), formats.add('superscript');
    if (sub) formats.delete('superscript'), formats.add('subscript');
    // 굵게 끄기(워드 `font-weight:normal`)
    if (/font-weight:\s*(normal|[1-4]00)/.test(style)) formats.delete('bold');
    if (/font-style:\s*normal/.test(style)) formats.delete('italic');
    const block = BLOCK.has(tag);
    if (block && lineOpen()) push('\n', []);
    const before = written;
    const list = [...formats].sort() as CharacterFormatDto[];
    for (const child of Array.from(node.childNodes)) walk(child, list);
    // 문단 끝: 글이 있었으면 줄을 닫고, 글 없는 문단은 빈 줄 하나(겹친 div > p 는 안쪽 p 가 이미 닫았다).
    if (block && (lineOpen() || written === before)) push('\n', []);
  };
  walk(body, []);
  if (!formatted) return null;
  // 줄 앞뒤 빈칸(HTML 들여쓰기 공백)을 걷고, 마지막 문단을 닫은 줄바꿈 하나만 뗀다 — 빈 줄은 남긴다.
  for (const [index, run] of runs.entries()) {
    run[0] = run[0].replace(/ +\n/g, '\n').replace(/\n +/g, '\n');
    const next = runs[index + 1];
    if (next?.[0].startsWith('\n')) run[0] = run[0].replace(/ +$/, '');
    if (run[0].endsWith('\n') && next) next[0] = next[0].replace(/^ +/, '');
  }
  runs[0][0] = runs[0][0].replace(/^ +/, '');
  const last = runs.at(-1)!;
  last[0] = last[0].replace(/ +$/, '').replace(/\n$/, '');
  return runs.filter(([value]) => value !== '');
}
