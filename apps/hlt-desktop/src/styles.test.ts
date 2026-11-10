import { expect, test } from 'vitest';
import { readFileSync } from 'node:fs';

// vitest 는 `?raw` CSS 를 비워 돌려준다 — 파일을 직접 읽는다.
const css = readFileSync('src/styles.css', 'utf8');

// vitest 는 CSS 를 적용하지 않는다 — 이 글 검사는 선언이 지워지는 것만 막는다(실제 효과는 실제 앱에서 잰다).
const rule = (selector: string) => {
  const found = css.split('\n').find((line: string) => line.startsWith(`${selector} {`));
  expect(found, selector).toBeDefined();
  return found!;
};

test('R-1: 원문 창이 문서 높이로 grid 행을 밀지 못한다', () => {
  expect(rule('.editor-body')).toContain('grid-template-rows: minmax(0, 1fr)');
  expect(rule('.editor-source')).toContain('min-height: 0');
  expect(rule('.source-body')).toContain('min-height: 0');
});

test('R-2: 쪽 칸은 왼쪽 정렬 + 쪽 auto 여백(좁으면 왼쪽도 스크롤로 닿는다)', () => {
  expect(rule('.editor-pages')).toContain('align-items: flex-start');
  expect(rule('.nk-page')).toContain('margin-inline: auto');
});

test('R-3: 배율 상자는 가운데 칸 오른쪽 아래', () => {
  const zoom = rule('.zoom-box');
  expect(zoom).toContain('right: 1.3rem');
  expect(zoom).toContain('bottom: .9rem');
  expect(zoom).not.toContain('grid-column');
});
