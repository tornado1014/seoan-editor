import { describe, expect, it } from 'vitest';
import type { PlannedPageDto } from './contracts';
import {
  caretLines,
  caretPoint,
  hit,
  lineEdge,
  previewDelete,
  previewInsert,
  selectionBoxes,
  vertical,
} from './page-caret';

// 문단 p 두 줄: 「가나 」(공백이 줄 끝에 매달림) · 「다A」. 글 = 「가나 다A」.
function pages(): Map<number, PlannedPageDto> {
  return new Map([
    [
      0,
      {
        number: 1,
        images: [],
        lines: [
          {
            top: 100,
            paragraph: 'p',
            start: 0,
            stops: [50, 62, 74, 80],
            glyphs: [
              { ch: '가', x: 50, size: 12, style: 0, drop: 0 },
              { ch: '나', x: 62, size: 12, style: 2, drop: 0 },
            ],
          },
          {
            top: 116,
            paragraph: 'p',
            start: 3,
            stops: [50, 62, 68],
            glyphs: [
              { ch: '다', x: 50, size: 12, style: 0, drop: 0 },
              { ch: 'A', x: 62, size: 12, style: 0, drop: 0 },
            ],
          },
          // 표 칸 줄 · 쪽번호는 커서 줄이 아니다.
          { top: 130, paragraph: 'q', cell: 'cell:1', glyphs: [] },
          { top: 800, paragraph: null, glyphs: [{ ch: '1', x: 280, size: 10, style: 0, drop: 0 }] },
        ],
      },
    ],
  ]);
}

describe('page caret', () => {
  it('maps clicks to offsets and offsets back to points', () => {
    const lines = caretLines(pages());
    expect(lines).toHaveLength(2);
    // 「나」 오른쪽 절반 → 나 뒤(2) · 둘째 줄 「A」 앞 → 4.
    expect(hit(lines, 0, 70, 104)).toEqual({ paragraph: 'p', offset: 2 });
    expect(hit(lines, 0, 61, 118)).toEqual({ paragraph: 'p', offset: 4 });
    // 줄 끝(3) = 다음 줄 처음으로 그린다.
    expect(caretPoint(lines, { paragraph: 'p', offset: 3 })).toEqual({ page: 0, x: 50, top: 116 });
    expect(caretPoint(lines, { paragraph: 'p', offset: 5 })).toEqual({ page: 0, x: 68, top: 116 });
  });

  it('moves by lines and to line edges', () => {
    const lines = caretLines(pages());
    expect(vertical(lines, { paragraph: 'p', offset: 1 }, 1, 62)).toEqual({ paragraph: 'p', offset: 4 });
    expect(vertical(lines, { paragraph: 'p', offset: 4 }, -1, 62)).toEqual({ paragraph: 'p', offset: 1 });
    // 첫 줄 끝 = 매달린 공백 앞(2) · 마지막 줄 끝 = 글 끝(5).
    expect(lineEdge(lines, { paragraph: 'p', offset: 0 }, true).offset).toBe(2);
    expect(lineEdge(lines, { paragraph: 'p', offset: 4 }, true).offset).toBe(5);
    expect(lineEdge(lines, { paragraph: 'p', offset: 4 }, false).offset).toBe(3);
  });

  it('boxes a selection across lines', () => {
    expect(selectionBoxes(caretLines(pages()), 'p', 1, 4)).toEqual([
      { page: 0, x: 62, top: 100, width: 18 },
      { page: 0, x: 50, top: 116, width: 12 },
    ]);
  });

  it('previews an insertion by pushing later glyphs and later lines', () => {
    const map = pages();
    expect(previewInsert(map, { paragraph: 'p', offset: 1 }, '라b')).toBe(0);
    const [first, second] = map.get(0)!.lines;
    expect(first.glyphs.map((glyph) => [glyph.ch, glyph.x, glyph.style])).toEqual([
      ['가', 50, 0],
      ['라', 62, 0],
      ['b', 74, 0],
      ['나', 80, 2],
    ]);
    expect(first.stops).toEqual([50, 62, 74, 80, 92, 98]);
    expect(second.start).toBe(5);
  });

  it('previews a deletion inside one line and refuses one across lines', () => {
    const map = pages();
    expect(previewDelete(map, 'p', 0, 1)).toBe(0);
    const [first, second] = map.get(0)!.lines;
    expect(first.glyphs.map((glyph) => [glyph.ch, glyph.x])).toEqual([['나', 50]]);
    expect(first.stops).toEqual([50, 62, 68]);
    expect(second.start).toBe(2);
    expect(previewDelete(map, 'p', 1, 3)).toBeNull();
  });
});

// 표 칸 줄: 칸 줄의 `paragraph` 는 표 문단(p)과 같다 — 커서 열쇠는 칸 id 여야 본문 커서와 섞이지 않는다.
function tablePages(): Map<number, PlannedPageDto> {
  return new Map([
    [
      0,
      {
        number: 1,
        images: [],
        lines: [
          { top: 100, paragraph: 'p', start: 0, stops: [50, 62], glyphs: [{ ch: '가', x: 50, size: 12, style: 0, drop: 0 }] },
          {
            top: 120,
            paragraph: 'p',
            cell: 'cell:0:0:0',
            start: 0,
            stops: [52, 61, 70],
            glyphs: [
              { ch: '표', x: 52, size: 9, style: 0, drop: 0 },
              { ch: '칸', x: 61, size: 9, style: 0, drop: 0 },
            ],
          },
          { top: 120, paragraph: 'p', cell: 'cell:0:0:1', start: 0, stops: [150], glyphs: [] },
        ],
      },
    ],
  ]);
}

describe('표 칸 커서', () => {
  it('칸 줄을 누르면 칸 커서(빈 칸 포함)', () => {
    const lines = caretLines(tablePages());
    expect(hit(lines, 0, 62, 122)).toEqual({ paragraph: 'cell:0:0:0', offset: 1 });
    expect(hit(lines, 0, 151, 122)).toEqual({ paragraph: 'cell:0:0:1', offset: 0 });
  });
  it('본문 커서는 같은 문단 id 의 칸 줄에 붙지 않는다', () => {
    const lines = caretLines(tablePages());
    expect(caretPoint(lines, { paragraph: 'p', offset: 1 })).toEqual({ page: 0, x: 62, top: 100 });
    expect(caretPoint(lines, { paragraph: 'cell:0:0:0', offset: 2 })).toEqual({ page: 0, x: 70, top: 120 });
  });
  it('칸에 친 글은 그 칸 줄에만 끼운다', () => {
    const map = tablePages();
    previewInsert(map, { paragraph: 'cell:0:0:1', offset: 0 }, '새');
    const page = map.get(0)!;
    expect(page.lines[2].glyphs.map((glyph) => glyph.ch)).toEqual(['새']);
    expect(page.lines[0].glyphs.map((glyph) => glyph.ch)).toEqual(['가']);
  });
});
