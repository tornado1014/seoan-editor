// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';
import { claimsStart, createPinnedPane, type PinnedParagraph } from './pinned-pane';

const para = (id: string, text: string, has_objects = false): PinnedParagraph => ({ id, text, spans: [], has_objects });
const rows = (...texts: string[]) => texts.map((text, i) => para(`p${i}`, text));

// jsdom 은 배치를 모른다 — 문단 높이를 20px 로 흉내 낸다.
const original = Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'offsetTop');
beforeEach(() => {
  Object.defineProperty(HTMLElement.prototype, 'offsetTop', {
    configurable: true,
    get(this: HTMLElement) {
      return this.parentElement ? [...this.parentElement.children].indexOf(this) * 20 : 0;
    },
  });
});
afterEach(() => {
  if (original) Object.defineProperty(HTMLElement.prototype, 'offsetTop', original);
});

function make() {
  const host = document.createElement('section');
  host.hidden = true;
  const go = vi.fn();
  const pane = createPinnedPane(host, { go });
  return { host, go, pane, body: host.querySelector<HTMLElement>('.pinned-body')! };
}

describe('claimsStart', () => {
  test('【청구범위】 가 있으면 그 문단', () => {
    expect(claimsStart([...rows('가', '【발명의 설명】'), para('c', '【청구범위】'), para('d', '【청구항 1】')])).toBe('c');
  });
  test('【청구범위】 가 없으면 첫 【청구항 N】', () => {
    expect(claimsStart([para('a', '본문'), para('b', '【청구항 1】'), para('c', '【청구항 2】')])).toBe('b');
  });
  test('둘 다 없으면 null', () => {
    expect(claimsStart(rows('가', '나'))).toBeNull();
  });
});

describe('createPinnedPane', () => {
  test('show(null) 은 문서 처음에서 시작한다', () => {
    const { pane } = make();
    pane.render(rows('가', '나', '다'));
    pane.show(null);
    expect(pane.shown).toBe(true);
    expect(pane.topParagraph()).toBe('p0');
  });

  test('show(id) 는 그 문단이 맨 위가 된다', () => {
    const { pane } = make();
    pane.render(rows('가', '나', '다'));
    pane.show('p2');
    expect(pane.topParagraph()).toBe('p2');
  });

  test('문단 하나를 고쳐 다시 그려도 맨 위 문단이 같다', () => {
    const { pane } = make();
    pane.render(rows('가', '나', '다', '라'));
    pane.show('p2');
    pane.render(rows('가', '나', '다 고침', '라'));
    expect(pane.topParagraph()).toBe('p2');
  });

  test('앞에 문단이 늘어도 붙잡은 문단이 맨 위다', () => {
    const { pane } = make();
    pane.render(rows('가', '나', '다', '라'));
    pane.show('p2');
    pane.render([para('x', '새'), ...rows('가', '나', '다', '라')]);
    expect(pane.topParagraph()).toBe('p2');
  });

  test('붙잡은 문단이 지워지면 그 앞 문단', () => {
    const { pane } = make();
    pane.render(rows('가', '나', '다', '라'));
    pane.show('p2');
    pane.render(rows('가', '나', '라').map((each, i) => ({ ...each, id: ['p0', 'p1', 'p3'][i] })));
    expect(pane.topParagraph()).toBe('p1');
  });

  test('문단을 누르면 가기 · 표와 그림 문단은 커서 없이', () => {
    const { pane, body, go } = make();
    pane.render([para('a', '글'), para('b', '표', true)]);
    pane.show(null);
    body.querySelector<HTMLElement>('p[data-id="a"]')!.click();
    body.querySelector<HTMLElement>('p[data-id="b"]')!.click();
    expect(go.mock.calls).toEqual([['a', true], ['b', false]]);
    expect(body.querySelector('p[data-id="b"]')!.textContent).toContain('[표 · 그림]');
  });

  test('표제 문단은 굵게 · 읽기 전용(입력 칸 없음) · 닫으면 비운다', () => {
    const { pane, host, body } = make();
    pane.render(rows('【청구범위】', '본문'));
    pane.show(null);
    expect(body.querySelector('p[data-id="p0"]')!.classList.contains('pinned-heading')).toBe(true);
    expect(host.querySelector('[contenteditable], input, textarea')).toBeNull();
    pane.hide();
    expect(pane.shown).toBe(false);
    expect(body.children.length).toBe(0);
  });

  test('머리 단추 「청구범위」 는 청구범위 처음으로', () => {
    const { pane, host } = make();
    pane.render([...rows('가', '나'), para('c', '【청구범위】'), para('d', '청구항')]);
    pane.show(null);
    host.querySelector<HTMLButtonElement>('.pinned-head button')!.click();
    expect(pane.topParagraph()).toBe('c');
  });
});
