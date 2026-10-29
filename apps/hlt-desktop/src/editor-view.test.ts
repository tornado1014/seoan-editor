// @vitest-environment jsdom
import { describe, expect, it } from 'vitest';

import { htmlRuns, outlineTree, renderPage, richParagraph, selectOffsets, selectionOffsets, repeatedSentences, documentStats } from './editor-view';
import type { OutlineNode } from './editor-view';

describe('renderPage', () => {
  it('places glyphs and images at the planned points with styles', () => {
    const page = {
      number: 1,
      lines: [
        {
          top: 115.58,
          paragraph: 'paragraph:4',
          glyphs: [
            { ch: '본', x: 89.76, size: 12, style: 2, drop: 0 },
            { ch: '6', x: 101.76, size: 7.44, style: 32, drop: 3.91 },
          ],
        },
      ],
      images: [{ object_code: 7, image: 'image:0', x: 89.76, top: 182.04, width: 399.24, height: 475.32 }],
    };
    const host = document.createElement('div');
    host.append(renderPage(page, 2, (code) => (code === 7 ? 'blob:x' : null), 'paragraph:4'));
    const glyphs = host.querySelectorAll<HTMLElement>('.nk-glyph');
    expect(glyphs[0].style.left).toBe('179.52px');
    expect(glyphs[0].style.fontWeight).toBe('bold');
    expect(glyphs[0].style.getPropertyValue('font-synthesis'), '바탕체엔 굵은 꼴이 없다 — 합성 굵게').toBe('weight');
    expect(glyphs[0].style.getPropertyValue('-webkit-text-stroke'), '윤곽선 흉내는 번진다').toBe('');
    expect(glyphs[1].style.top).toBe(`${(115.58 + 3.91) * 2}px`);
    expect(host.querySelector('.nk-line')?.classList.contains('selected')).toBe(true);
    expect(glyphs[0].style.textDecoration, '밑줄은 글자마다 그리지 않는다').toBe('');
    const image = host.querySelector<HTMLImageElement>('img')!;
    expect(image.src).toBe('blob:x');
    expect(image.style.width).toBe('798.48px');
    expect(image.dataset.image, '크기 바꾸기가 가리킬 그림 id').toBe('image:0');
  });
});

describe('underline', () => {
  it('draws one bar per planned run, spanning the spaces between underlined words', () => {
    const page = {
      number: 1,
      images: [],
      lines: [{ top: 100, paragraph: 'p', glyphs: [{ ch: '가', x: 50, size: 12, style: 1, drop: 0 }, { ch: '나', x: 80, size: 12, style: 1, drop: 0 }], underlines: [[50, 92] as [number, number]] }],
    };
    const host = document.createElement('div');
    host.append(renderPage(page, 2, () => null, null));
    const bars = host.querySelectorAll<HTMLElement>('.nk-underline');
    expect(bars).toHaveLength(1);
    expect(bars[0].style.left).toBe('100px');
    expect(bars[0].style.width, '「가」 앞부터 「나」 끝까지 — 사이 공백 포함').toBe('84px');
  });
});

describe('outline', () => {
  it('nests headings like the NKEditor tree: sections, subsections, identification items', () => {
    const heading = (id: string, label: string) => ({ id, text: `【${label}】` });
    const tree = outlineTree('PCT특허', [
      heading('a', '발명의 설명'),
      heading('b', '발명의 명칭'),
      { id: 'x', text: '본 발명은' },
      heading('c', '발명의 내용'),
      heading('d', '해결하려는 과제'),
      heading('e', '발명의 효과'),
      heading('f', '발명을 실시하기 위한 구체적인 내용'),
      heading('g', '표 1'),
      heading('h', '청구범위'),
      heading('i', '청구항 1'),
      heading('j', '요약서'),
      heading('k', '요약'),
      heading('l', '도면'),
      heading('m', '도 1'),
    ]);
    const shape = (node: OutlineNode): unknown =>
      node.children.length ? { [node.label]: node.children.map(shape) } : node.label;
    expect(shape(tree)).toEqual({
      PCT특허: [
        {
          '발명의 설명': [
            '발명의 명칭',
            { '발명의 내용': ['해결하려는 과제', '발명의 효과'] },
            { '발명을 실시하기 위한 구체적인 내용': ['표 1'] },
          ],
        },
        { 청구범위: ['청구항 1'] },
        { 요약서: ['요약'] },
        { 도면: ['도 1'] },
      ],
    });
  });
});

describe('rich edit panel', () => {
  it('shows formats and maps selections to character offsets', () => {
    const host = document.createElement('div');
    document.body.append(host);
    host.append(
      richParagraph('R6는 굵게 끝', [
        { start: 1, end: 2, style: 32 },
        { start: 4, end: 6, style: 2 },
      ]),
    );
    expect(host.querySelector('sub')?.textContent).toBe('6');
    expect(host.querySelector('span')?.textContent).toBe('굵게');
    expect(host.textContent).toBe('R6는 굵게 끝');
    selectOffsets(host, 4, 6);
    expect(selectionOffsets(host)).toEqual([4, 6]);
    selectOffsets(host, 0, 8);
    expect(selectionOffsets(host)).toEqual([0, 8]);
    selectOffsets(host, 2, 99);
    expect(selectionOffsets(host)).toEqual([2, 8]);
  });
});

it('repeatedSentences: 상세설명 20자 넘는 문장이 두 번 넘게 · 본문이 같은 청구항 · 짧은 문장 · 다른 절은 빼고 · 많은 것부터', () => {
  const long = '상기 기판은 열처리 공정을 거쳐 단단하게 굳어진 상태가 된다.';
  const decimal = '상기 온도는 약 1.5 배로 높아져서 반응 속도가 빨라지게 된다.';
  const paragraphs = [
    '【발명의 설명】',
    `${long} 짧은 문장이다.`,
    `앞 문장. ${long}`,
    `${long} 소수 1.5 를 쓴 문장은 끝까지 하나의 문장으로 본다.`,
    '짧은 문장이다.',
    decimal,
    `${decimal} 끝.`,
    '【청구범위】',
    '【청구항 1】',
    '기판을 포함하는 장치.',
    '【청구항 2】',
    '기판을 포함하는',
    ' 장치.',
    '【청구항 3】',
    '삭제',
    '【청구항 4】',
    '삭제',
    '【요약서】',
    `${long}`,
  ].map((text, i) => ({ id: `p${i}`, text }));
  expect(repeatedSentences(paragraphs)).toEqual([
    { text: long, at: ['p1', 'p2', 'p3'], claim: false },
    { text: decimal, at: ['p5', 'p6'], claim: false },
    { text: '기판을 포함하는 장치.', at: ['p8', 'p10'], claim: true },
  ]);
});

it('documentStats: 쪽 · 청구항(삭제 · 독립 · 종속) · 문단 · 글자 · 도면 · 요약 · 요금 기준(금액 없음)', () => {
  const paragraphs = [
    '【발명의 설명】',
    '【기술분야】',
    '가나 다',
    '_',
    '【청구범위】',
    '【청구항 1】',
    '장치.',
    '【청구항 2】',
    '제1항에 있어서.',
    '【청구항 3】',
    '삭제',
    '【요약서】',
    '【요약】',
    '요약 글',
    '【도면】',
    '【도 1】',
    '_',
    '【도 2a】',
    '_',
  ].map((text, i) => ({ id: `p${i}`, text }));
  const rows = Object.fromEntries(documentStats(paragraphs, 23, { '1': [], '2': [1], '3': [] }));
  expect(rows['쪽 수']).toBe('23쪽');
  expect(rows['청구항']).toBe('3개(삭제 1) · 독립 1 · 종속 1');
  expect(rows['상세설명 문단']).toBe('1개');
  expect(rows['글자 수(상세설명 · 청구항)']).toBe('16자(공백 빼고) · 18자(공백 포함)');
  expect(rows['도면']).toBe('2개');
  expect(rows['요약']).toBe('3자(공백 빼고 · 권장 400자)');
  expect(rows['요금이 붙는 기준']).toBe('20쪽 넘는 쪽 3쪽 · 청구항 2개 — 금액은 특허로 요금표로 확인');
  expect(Object.fromEntries(documentStats(paragraphs, 5, {}))['청구항']).toBe('3개(삭제 1)');
});

describe('htmlRuns', () => {
  it('reads Word clipboard HTML into formatted runs and paragraph breaks', () => {
    const html = `<html><head><style>p.MsoNormal{margin:0}</style></head><body>
<p class=MsoNormal><b>굵은</b> 글 H<sub>2</sub>O<o:p></o:p></p>
<p class=MsoNormal><span style='font-style:italic'>기울임</span> x<span style='vertical-align:super'>2</span><o:p>&nbsp;</o:p></p>
</body></html>`;
    expect(htmlRuns(html)).toEqual([
      ['굵은', ['bold']],
      [' 글 H', []],
      ['2', ['subscript']],
      ['O\n', []],
      ['기울임', ['italic']],
      [' x', []],
      ['2', ['superscript']],
    ]);
  });
  it('reads what Seoan itself copies', () => {
    expect(htmlRuns('<p><b>앞</b>H<sub>2</sub>O</p><p>x<sup>2</sup>뒤</p>')).toEqual([
      ['앞', ['bold']],
      ['H', []],
      ['2', ['subscript']],
      ['O\nx', []],
      ['2', ['superscript']],
      ['뒤', []],
    ]);
  });
  it('returns null when nothing is formatted (plain paste path)', () => {
    expect(htmlRuns('<p>그냥 글</p><p>둘째 줄</p>')).toBeNull();
    expect(htmlRuns('<p><b> </b></p>')).toBeNull();
  });
});
