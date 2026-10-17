// @vitest-environment jsdom
import { describe, expect, it } from 'vitest';

import { blackAndWhite, latexFigure } from './figure-editor';

/** MathJax 대역: ex 단위 SVG 를 돌려준다(실제 MathJax 는 브라우저에서). */
const engine = {
  tex2svg: (latex: string) => {
    const host = document.createElement('mjx-container');
    host.innerHTML = latex.includes('frac')
      ? '<svg width="6.2ex" height="5.1ex"></svg>'
      : '<svg width="8.5ex" height="1.9ex"></svg>';
    return host;
  },
};

describe('latexFigure', () => {
  it('sizes the SVG in px and pt for 12pt body text (1ex = 8px = 6pt)', () => {
    const figure = latexFigure(engine, 'E = mc^2');
    expect(figure.svg).toContain('width="68"');
    expect(figure.widthPt).toBeCloseTo(51);
    expect(figure.heightPt).toBeCloseTo(11.4);
    expect(latexFigure(engine, '\\frac{a}{b}').heightPt).toBeGreaterThan(figure.heightPt);
  });
  it('refuses MathJax error output', () => {
    const broken = { tex2svg: () => { const h = document.createElement('div'); h.innerHTML = '<svg width="1ex" height="1ex"><g data-mjx-error="x"></g></svg>'; return h; } };
    expect(() => latexFigure(broken, '\\frac{')).toThrow();
  });
});

describe('blackAndWhite', () => {
  it('turns atom colours black and keeps the white background', () => {
    expect(blackAndWhite('<rect style="fill:#FFFFFF"/><path style="stroke:#FF0000"/><text fill="#3333ff">N</text>')).toBe(
      '<rect style="fill:#FFFFFF"/><path style="stroke:#000000"/><text fill="#000000">N</text>',
    );
  });
});
