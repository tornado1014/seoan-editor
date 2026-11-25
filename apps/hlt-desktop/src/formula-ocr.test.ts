import { describe, expect, it } from 'vitest';

import { decodeTokens, greedy, resizeBicubic, tidyLatex } from './formula-ocr';

/** 합성 그림: 화소 (x, y) 의 채널 k = (37x + 91y + 53k) mod 256. */
function picture(w: number, h: number): Uint8Array {
  const rgb = new Uint8Array(w * h * 3);
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) for (let k = 0; k < 3; k++) rgb[(y * w + x) * 3 + k] = (x * 37 + y * 91 + k * 53) % 256;
  return rgb;
}

describe('resizeBicubic', () => {
  // 기준값 = Pillow `Image.resize((4, 4), Image.BICUBIC)` 가 같은 그림에 낸 값.
  it('shrinks like Pillow BICUBIC (9×6 → 4×4)', () => {
    expect([...resizeBicubic(picture(9, 6), 9, 6, 4, 4)]).toEqual([
      55, 116, 154, 146, 180, 155, 148, 124, 107, 65, 93, 153, 144, 129, 104, 98, 123, 109, 112, 144, 178, 158, 147, 130,
      81, 130, 166, 127, 145, 135, 141, 146, 96, 95, 111, 168, 174, 154, 120, 91, 90, 126, 93, 154, 155, 184, 172, 118,
    ]);
  });
  it('enlarges like Pillow BICUBIC (2×3 → 4×4)', () => {
    expect([...resizeBicubic(picture(2, 3), 2, 3, 4, 4)]).toEqual([
      0, 45, 98, 3, 56, 109, 24, 77, 130, 35, 88, 141, 49, 99, 172, 59, 115, 183, 80, 147, 204, 91, 163, 215, 128, 190,
      139, 139, 173, 150, 160, 138, 171, 171, 122, 182, 184, 255, 20, 195, 192, 31, 216, 55, 52, 227, 0, 63,
    ]);
  });
});

describe('decodeTokens', () => {
  it('joins byte-level pieces and drops special tokens', () => {
    const words = ['<pad>', '<s>', '</s>', '\\frac', 'Ġ{', 'Ġa', 'Ġ}'];
    expect(decodeTokens([1, 3, 4, 5, 6, 2, 99], words)).toBe('\\frac { a }');
  });
});

describe('tidyLatex', () => {
  it('removes the spaces between tokens', () => {
    expect(tidyLatex('3 . 1 0 \\times 1 0 ^ { 7 }')).toBe('3.10\\times10^{7}');
    expect(tidyLatex('\\mathrm { s u b j e c t ~ t o } \\qquad \\| x \\|')).toBe('\\mathrm{subject~to}\\qquad\\|x\\|');
  });
  it('keeps the space that ends a control word before a letter', () => {
    expect(tidyLatex('\\alpha x + \\forall k , \\Delta \\mathfrak { t }')).toBe('\\alpha x+\\forall k,\\Delta\\mathfrak{t}');
  });
});

describe('greedy', () => {
  /** 대본대로 다음 토큰을 내는 디코더(낱말 5개). */
  const scripted = (script: number[]) => async (ids: number[]) => {
    const scores = [0, 0, 0, 0, 0];
    scores[script[ids.length - 1] ?? 2] = 1;
    return scores;
  };
  it('stops at the end token and leaves the start token out', async () => {
    expect(await greedy(scripted([3, 4, 3, 2, 4]))).toEqual([3, 4, 3]);
  });
  it('stops at the limit when the end token never comes', async () => {
    expect(await greedy(async () => [0, 0, 0, 1, 0], 6)).toEqual([3, 3, 3, 3, 3, 3]);
  });
});
