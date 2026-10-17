// 쪽 위 편집(WYSIWYG)의 커서 셈. 쪽 계획 줄마다 `start`(문단 글 안 자리)와 `stops`(글자 경계 x pt)가 실린다 —
// 클릭 → 자리, 자리 → 그릴 곳, 위아래 이동, 그리고 다시 굽기 전에 보이는 임시 고침(글자를 끼우고 뒤 글자를 민다).
import type { PlannedLineDto, PlannedPageDto } from './contracts';

export interface Caret {
  paragraph: string;
  offset: number;
}

export interface CaretLine {
  page: number;
  line: PlannedLineDto;
}

/** 커서가 설 수 있는 줄, 쪽 · 위 차례. */
export function caretLines(pages: Map<number, PlannedPageDto>): CaretLine[] {
  const out: CaretLine[] = [];
  for (const index of [...pages.keys()].sort((a, b) => a - b)) {
    for (const line of pages.get(index)!.lines) {
      if (line.paragraph && line.start !== undefined && line.stops?.length) out.push({ page: index, line });
    }
  }
  return out;
}

const size = (line: PlannedLineDto) => line.stops!.length - 1;

/** 자리가 든 줄. 줄 끝 = 다음 줄 처음이면 다음 줄. */
export function lineAt(lines: CaretLine[], caret: Caret): CaretLine | null {
  let fallback: CaretLine | null = null;
  for (const item of lines) {
    if (item.line.paragraph !== caret.paragraph) continue;
    const start = item.line.start!;
    if (caret.offset >= start && caret.offset < start + size(item.line)) return item;
    if (caret.offset === start + size(item.line)) fallback = item;
  }
  return fallback;
}

/** 커서를 그릴 곳(쪽 · x · 줄 윗변 pt). */
export function caretPoint(lines: CaretLine[], caret: Caret): { page: number; x: number; top: number } | null {
  const item = lineAt(lines, caret);
  if (!item) return null;
  return { page: item.page, x: item.line.stops![caret.offset - item.line.start!], top: item.line.top };
}

function nearestIndex(line: PlannedLineDto, x: number): number {
  const stops = line.stops!;
  for (let i = 0; i < stops.length - 1; i += 1) {
    if (x < (stops[i] + stops[i + 1]) / 2) return i;
  }
  return stops.length - 1;
}

/** 쪽 위 점(pt) → 자리. 그 쪽의 커서 줄 가운데 세로로 가장 가까운 줄. */
export function hit(lines: CaretLine[], page: number, x: number, y: number): Caret | null {
  let best: CaretLine | null = null;
  let distance = Infinity;
  for (const item of lines) {
    if (item.page !== page) continue;
    const { top } = item.line;
    const d = y >= top - 3 && y <= top + 15 ? 0 : Math.abs(y - (top + 6));
    if (d < distance) {
      distance = d;
      best = item;
    }
  }
  if (!best) return null;
  return { paragraph: best.line.paragraph!, offset: best.line.start! + nearestIndex(best.line, x) };
}

/** 위(-1) · 아래(+1) 줄로. `goalX` = 처음 움직이기 시작한 x(짧은 줄을 지나도 열이 유지된다). */
export function vertical(lines: CaretLine[], caret: Caret, direction: -1 | 1, goalX: number): Caret {
  const item = lineAt(lines, caret);
  if (!item) return caret;
  const target = lines[lines.indexOf(item) + direction];
  if (!target) return caret;
  return { paragraph: target.line.paragraph!, offset: target.line.start! + nearestIndex(target.line, goalX) };
}

/** 줄 처음 · 끝 자리. 끝은 같은 문단 다음 줄이 있으면 그 줄 처음 앞 글자(줄 끝 공백 앞)다. */
export function lineEdge(lines: CaretLine[], caret: Caret, end: boolean): Caret {
  const item = lineAt(lines, caret);
  if (!item) return caret;
  const start = item.line.start!;
  if (!end) return { ...caret, offset: start };
  const next = lines[lines.indexOf(item) + 1];
  const continues = next?.line.paragraph === caret.paragraph && size(item.line) > 0;
  return { ...caret, offset: start + size(item.line) - (continues ? 1 : 0) };
}

/** 고른 구간 [a, b)의 칠할 상자들(쪽 · x · 윗변 · 폭 pt). */
export function selectionBoxes(
  lines: CaretLine[],
  paragraph: string,
  a: number,
  b: number,
): { page: number; x: number; top: number; width: number }[] {
  const out = [];
  for (const item of lines) {
    if (item.line.paragraph !== paragraph) continue;
    const start = item.line.start!;
    const from = Math.max(a, start);
    const to = Math.min(b, start + size(item.line));
    if (from >= to) continue;
    const x = item.line.stops![from - start];
    out.push({ page: item.page, x, top: item.line.top, width: item.line.stops![to - start] - x });
  }
  return out;
}

const width = (ch: string, fontSize: number) => (ch.charCodeAt(0) < 0x80 ? fontSize / 2 : fontSize);
// 탭은 본문 왼쪽 끝(49.80pt)부터 36pt(300 px600) 격자의 다음 칸까지 — 코어 `reflow::next_tab` 과 같다.
const LEFT = 49.8;
const TAB = 36;
const advance = (ch: string, x: number, fontSize: number) =>
  ch === '\t' ? LEFT + (Math.floor((x - LEFT) / TAB + 1e-9) + 1) * TAB - x : width(ch, fontSize);

/**
 * 임시 고침: 자리에 글을 끼운다(줄은 다시 흘리지 않고 뒤 글자를 민다 — 다시 구우면 통특기 줄나눔으로 바로잡힌다).
 * 바뀐 쪽 차례를 돌려준다(없으면 null — 자리를 못 찾았다).
 */
export function previewInsert(pages: Map<number, PlannedPageDto>, caret: Caret, text: string): number | null {
  const lines = caretLines(pages);
  const item = lineAt(lines, caret);
  if (!item) return null;
  const { line } = item;
  const i = caret.offset - line.start!;
  const x0 = line.stops![i];
  const before = line.glyphs.filter((glyph) => glyph.x < x0 - 1e-6).at(-1);
  const fontSize = before?.size ?? line.glyphs[0]?.size ?? 12;
  const chars = Array.from(text);
  const starts: number[] = [];
  let total = 0;
  for (const ch of chars) {
    starts.push(x0 + total);
    total += advance(ch, x0 + total, fontSize);
  }
  for (const glyph of line.glyphs) if (glyph.x >= x0 - 1e-6) glyph.x += total;
  const added = chars
    .map((ch, k) => ({ ch, x: starts[k], size: fontSize, style: before?.style ?? 0, drop: before?.drop ?? 0 }))
    .filter((glyph) => glyph.ch !== ' ' && glyph.ch !== '\t');
  line.glyphs.push(...added);
  line.glyphs.sort((a, b) => a.x - b.x);
  line.stops = [...line.stops!.slice(0, i), ...starts, ...line.stops!.slice(i).map((x) => x + total)];
  for (const other of lines) {
    if (other.line !== line && other.line.paragraph === caret.paragraph && other.line.start! > line.start!) {
      other.line.start! += chars.length;
    }
  }
  return item.page;
}

/** 임시 고침: 한 줄 안의 [a, b)를 지운다. 줄을 넘는 구간이면 null(다시 굽기를 기다린다). */
export function previewDelete(
  pages: Map<number, PlannedPageDto>,
  paragraph: string,
  a: number,
  b: number,
): number | null {
  const lines = caretLines(pages);
  const item = lineAt(lines, { paragraph, offset: a });
  if (!item) return null;
  const { line } = item;
  const start = line.start!;
  if (a < start || b > start + size(line)) return null;
  const x0 = line.stops![a - start];
  const x1 = line.stops![b - start];
  const shift = x1 - x0;
  line.glyphs = line.glyphs.filter((glyph) => glyph.x < x0 - 1e-6 || glyph.x >= x1 - 1e-6);
  for (const glyph of line.glyphs) if (glyph.x >= x1 - 1e-6) glyph.x -= shift;
  line.stops = [...line.stops!.slice(0, a - start), ...line.stops!.slice(b - start).map((x) => x - shift)];
  for (const other of lines) {
    if (other.line !== line && other.line.paragraph === paragraph && other.line.start! > start) {
      other.line.start! -= b - a;
    }
  }
  return item.page;
}
