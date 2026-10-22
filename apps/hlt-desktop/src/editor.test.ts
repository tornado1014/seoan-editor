// @vitest-environment jsdom
// 쪽 위 편집(WYSIWYG) 흐름: 쪽을 눌러 커서 → 치면 바로 보이고 0.4초 뒤 문단 고침 한 번 → 엔터 = 문단 나누기 →
// 문단 첫머리 Backspace = 앞 문단에 붙이기. 다시 굽는 동안 친 키는 버리지 않는다. 백엔드는 가짜다.
import { beforeEach, describe, expect, test, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import type { EditorViewDto, PlannedImageDto, PlannedPageDto } from './contracts';
import { mountEditor } from './editor';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('./figure-editor', () => ({}));
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ label: 'main' }) }));
const handlers = new Map<string, (event: { payload: unknown }) => unknown>();
vi.mock('@tauri-apps/api/event', () => ({
  listen: async (name: string, handler: (event: { payload: unknown }) => unknown) => (handlers.set(name, handler), () => {}),
}));
type MenuItem = { text?: string; item?: string; enabled?: boolean; action?: () => void; items?: MenuItem[] };
const menus: MenuItem[][] = [];
vi.mock('@tauri-apps/api/menu', () => ({
  Menu: { new: async ({ items }: { items: MenuItem[] }) => (menus.push(items), { popup: async () => {} }) },
}));

const SCALE = 1.3;

function view(texts: string[]): EditorViewDto {
  return {
    title: 't',
    editable: true,
    page_count: 1,
    page_width: 595,
    page_height: 842,
    preparing: false,
    paragraph_pages: Object.fromEntries(texts.map((_, i) => [`paragraph:${i}`, 0])),
    paragraphs: texts.map((text, i) => ({
      id: `paragraph:${i}`,
      text,
      wipo_id: null,
      number: numbers[i] ?? null,
      has_objects: false,
      spans: [],
    })),
    cells: Object.fromEntries(cellLines.map((line) => [line.cell, line.text])),
  } as unknown as EditorViewDto;
}

/** 문단마다 한 줄 · 한글 12pt · ASCII 6pt. */
function page(texts: string[]): PlannedPageDto {
  return {
    number: 1,
    images: pageImages,
    lines: [...texts, ...cellLines.map((line) => line.text)].map((text, i) => {
      const stops = [50];
      for (const ch of text) stops.push(stops.at(-1)! + (ch.charCodeAt(0) < 0x80 ? 6 : 12));
      return {
        top: 100 + 20 * i,
        ...(i < texts.length ? { paragraph: `paragraph:${i}` } : { paragraph: null, cell: cellLines[i - texts.length].cell }),
        start: 0,
        stops,
        glyphs: Array.from(text)
          .map((ch, k) => ({ ch, x: stops[k], size: 12, style: 0, drop: 0 }))
          .filter((glyph) => glyph.ch !== ' '),
      };
    }),
  };
}

let texts: string[] = [];
let pageImages: PlannedImageDto[] = [];
/** 문단 줄들 아래 표 칸 줄(칸마다 한 줄). */
let cellLines: { cell: string; text: string }[] = [];
let numbers: (string | null)[] = [];
let closeAnswer = true;
const calls: [string, unknown][] = [];

beforeEach(() => {
  texts = ['가나다', '라마'];
  pageImages = [];
  cellLines = [];
  numbers = [];
  closeAnswer = true;
  calls.length = 0;
  // 쪽이 보이면 바로 그린다.
  globalThis.IntersectionObserver = class {
    constructor(private callback: IntersectionObserverCallback) {}
    observe(target: Element) {
      queueMicrotask(() => this.callback([{ isIntersecting: true, target } as IntersectionObserverEntry], this as never));
    }
    disconnect() {}
  } as never;
  Element.prototype.scrollIntoView = () => {};
  vi.mocked(invoke).mockImplementation((async (command: string, args?: Record<string, unknown>) => {
    calls.push([command, args]);
    switch (command) {
      case 'editor_open':
      case 'editor_view':
        return view(texts);
      case 'editor_page':
        return page(texts);
      case 'editor_image':
        return new Uint8Array();
      case 'editor_resize_image':
      case 'editor_insert_picture':
      case 'editor_replace_picture':
        return view(texts);
      case 'editor_read_review':
        return {
          proposals: [
            { label: '보정안 1', tracked: true, claims: [{ number: '1', before: ['가'], after: ['나'] }] },
            { label: '보정안 2', tracked: false, claims: [{ number: '2', before: ['다'], after: [] }] },
          ],
          notes: [
            { paragraphs: ['0001'], wrong: '가나', right: '가다', sentence: '…', positional: false },
            { paragraphs: ['0002'], wrong: '갉', right: null, sentence: '…', positional: false },
            { paragraphs: ['0003'], wrong: '라', right: '마', sentence: '…', positional: false },
            { paragraphs: ['0004'], wrong: '제1', right: '제2', sentence: '…', positional: true },
          ],
          figures: [{ number: '5', sentence: '…' }],
          matches: [3, 1, 2, 2],
        };
      case 'editor_apply_review':
        return {
          view: view(texts),
          outcome: { claims_changed: [], claims_deleted: ['2'], claims_added: [], claims_failed: [], notes_applied: [[0, 3], [1, 1]] },
        };
      case 'editor_cp949_scan':
        return Array.from(String(args!.text))
          .filter((ch) => ch === '\u2212' || ch === '\u2075')
          .map((ch) => ({
            from: ch,
            code: `U+${ch.codePointAt(0)!.toString(16).toUpperCase()}`,
            suggestion: ch === '\u2212' ? '-' : null,
            count: 1,
            context: String(args!.text),
          }));
      case 'editor_replace_text': {
        if (String(args!.text).includes('≈')) {
          throw { code: 'cp949_invalid', phase: 'generate_body', details: { char: 'U+2248' } };
        }
        const index = Number(String(args!.paragraphId).split(':')[1]);
        texts.splice(index, 1, ...String(args!.text).split('\n'));
        return view(texts);
      }
      case 'editor_replace_text_page_break': {
        const index = Number(String(args!.paragraphId).split(':')[1]);
        texts.splice(index, 1, ...String(args!.text).split('\n'));
        return view(texts);
      }
      case 'editor_find': {
        // 가짜 찾기: 글이 있으면 글자 그대로 · 비고 서식만이면 「굵게」 = 첫 문단 [1, 2) 하나.
        const { text, format } = args!.query as { text: string; format: string | null };
        if (!text) return format === 'bold' ? [{ paragraph_id: 'paragraph:0', start: 1, end: 2, context: '나' }] : [];
        const out: { paragraph_id: string; start: number; end: number; context: string }[] = [];
        texts.forEach((t, i) => {
          for (let at = t.indexOf(text); at >= 0; at = t.indexOf(text, at + text.length)) {
            out.push({ paragraph_id: `paragraph:${i}`, start: at, end: at + text.length, context: t });
          }
        });
        return out;
      }
      case 'editor_replace': {
        const matches = args!.matches as { paragraph_id: string; start: number; end: number }[];
        for (const m of [...matches].sort((a, b) => b.start - a.start)) {
          const i = Number(m.paragraph_id.split(':')[1]);
          const keep = args!.replacement === '' && args!.format !== 'keep';
          if (!keep) texts[i] = texts[i].slice(0, m.start) + String(args!.replacement) + texts[i].slice(m.end);
        }
        return view(texts);
      }
      case 'editor_set_alignment':
      case 'editor_toggle_format':
        return view(texts);
      case 'editor_new_opinion':
        texts = ['【의견내용】', ''];
        return view(texts);
      case 'editor_save':
      case 'editor_save_as':
        return '/tmp/x.HLT';
      case 'editor_close_document':
        return closeAnswer;
      case 'editor_new_tab':
        return undefined;
      case 'editor_remove_items': {
        for (const number of args!.numbers as string[]) {
          const index = texts.indexOf(`【${args!.kind} ${number}】`);
          let end = index + 1;
          if (args!.kind === '청구항') while (end < texts.length && !/^【.*】$/.test(texts[end])) end += 1;
          else if (texts[end] === '') end += 1;
          texts.splice(index, end - index);
        }
        return view(texts);
      }
      case 'editor_insert_item': {
        const index = Number(String(args!.paragraphId).split(':')[1]);
        const at = args!.before ? index : index + 1;
        texts.splice(at, 0, `【${args!.kind} ${args!.number}】`, '');
        return { view: view(texts), body: `paragraph:${at + 1}` };
      }
      case 'editor_close':
      case 'editor_open_pending':
        return command === 'editor_close' ? true : null;
      case 'editor_delete_range': {
        const from = Number(String(args!.fromId).split(':')[1]);
        const to = Number(String(args!.toId).split(':')[1]);
        texts.splice(from, to - from + 1, texts[from].slice(0, Number(args!.fromOffset)) + texts[to].slice(Number(args!.toOffset)));
        return view(texts);
      }
      case 'editor_toggle_format_spans':
        return view(texts);
      case 'editor_merge_paragraph': {
        const index = Number(String(args!.paragraphId).split(':')[1]);
        texts.splice(index - 1, 2, texts[index - 1] + texts[index]);
        return view(texts);
      }
    }
    throw new Error(command);
  }) as never);
});

const settle = (ms = 0) => new Promise((resolve) => setTimeout(resolve, ms));

async function open() {
  const root = document.createElement('div');
  document.body.replaceChildren(root);
  mountEditor(root);
  root.querySelector<HTMLButtonElement>('[data-action="open"]')!.click();
  await settle(10);
  return root;
}

function clickAt(root: HTMLElement, x: number, y: number) {
  const host = root.querySelector('.nk-page')!;
  host.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, button: 0, clientX: x * SCALE, clientY: y * SCALE }));
}

function type(root: HTMLElement, text: string) {
  const input = root.querySelector<HTMLTextAreaElement>('.caret-input')!;
  input.value = text;
  input.dispatchEvent(new Event('input'));
}

function press(root: HTMLElement, key: string) {
  root.querySelector('.caret-input')!.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true }));
}

const last = (name: string) => calls.filter(([command]) => command === name).at(-1);
const glyphs = (root: HTMLElement) => Array.from(root.querySelectorAll('.nk-glyph')).map((glyph) => glyph.textContent).join('');

describe('page editing', () => {
  test('typing shows at once and becomes one paragraph edit after a pause', async () => {
    const root = await open();
    clickAt(root, 63, 104); // 「나」 앞 → 자리 1
    expect(root.querySelector('.nk-caret')).not.toBeNull();
    type(root, '김');
    type(root, 'x');
    expect(glyphs(root)).toBe('가김x나다라마');
    expect(calls.filter(([command]) => command === 'editor_replace_text')).toHaveLength(0);
    await settle(500);
    expect(calls.filter(([command]) => command === 'editor_replace_text')).toEqual([
      ['editor_replace_text', { paragraphId: 'paragraph:0', text: '가김x나다' }],
    ]);
  });

  test('enter splits, backspace at the start merges, and keys typed meanwhile are kept', async () => {
    const root = await open();
    clickAt(root, 63, 104);
    press(root, 'Enter');
    type(root, 'Z'); // 나누기가 끝나기 전에 친 글 — 새 문단 처음에 들어가야 한다.
    await settle(600);
    expect(texts).toEqual(['가', 'Z나다', '라마']);
    clickAt(root, 49, 124); // 둘째 문단 처음
    press(root, 'Backspace');
    await settle(50);
    expect(last('editor_merge_paragraph')).toEqual(['editor_merge_paragraph', { paragraphId: 'paragraph:1' }]);
    expect(texts).toEqual(['가Z나다', '라마']);
    type(root, '!');
    await settle(500);
    expect(texts).toEqual(['가!Z나다', '라마']);
  });

  test('cmd/ctrl+enter breaks the page at the caret; tab types a tab instead of leaving the page', async () => {
    const root = await open();
    clickAt(root, 63, 104); // 「나」 앞(1)
    root.querySelector('.caret-input')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', ctrlKey: true, bubbles: true }));
    await settle(50);
    expect(last('editor_replace_text_page_break')).toEqual([
      'editor_replace_text_page_break',
      { paragraphId: 'paragraph:0', text: '가\n나다' },
    ]);
    expect(calls.filter(([command]) => command === 'editor_replace_text')).toHaveLength(0);
    expect(texts).toEqual(['가', '나다', '라마']);
    // 커서는 새 쪽 첫머리(둘째 조각 처음) — 치면 거기 들어간다.
    const tab = new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true });
    root.querySelector('.caret-input')!.dispatchEvent(tab);
    expect(tab.defaultPrevented, '포커스를 옮기지 않는다').toBe(true);
    await settle(500);
    expect(texts).toEqual(['가', '\t나다', '라마']);
  });

  describe('find and replace', () => {
    const bar = (root: HTMLElement) => root.querySelector<HTMLElement>('.find-bar')!;
    const findText = (root: HTMLElement) => root.querySelector<HTMLInputElement>('.find-text')!;
    const enter = (element: HTMLElement) => element.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }));
    const click = (root: HTMLElement, action: string) => root.querySelector<HTMLButtonElement>(`[data-action="${action}"]`)!.click();
    const label = (root: HTMLElement) => root.querySelector('.find-count')!.textContent;
    const count = (name: string) => calls.filter(([command]) => command === name).length;

    test('format-only find selects the match on the page and keeps focus in the find box', async () => {
      const root = await open();
      click(root, 'find');
      root.querySelector<HTMLSelectElement>('.find-format')!.value = 'bold';
      click(root, 'find-next');
      await settle(30);
      expect(last('editor_find')).toEqual(['editor_find', { query: { text: '', format: 'bold' } }]);
      expect(root.querySelectorAll('.nk-sel').length, '쪽 위에 고른 표시').toBeGreaterThan(0);
      expect(bar(root).hidden).toBe(false);
      expect(document.activeElement, '포커스는 찾기 칸에').toBe(findText(root));
    });

    test('no match names what was searched', async () => {
      const root = await open();
      click(root, 'find');
      root.querySelector<HTMLSelectElement>('.find-format')!.value = 'italic';
      click(root, 'find-next');
      await settle(30);
      expect(label(root)).toContain('기울임 글자: 찾는 것이 없습니다');
    });

    test('enter walks the results without editing the paragraph', async () => {
      texts = ['가나가나', '가'];
      const root = await open();
      click(root, 'find');
      findText(root).value = '가';
      findText(root).focus();
      enter(findText(root));
      await settle(30);
      expect(label(root)).toMatch(/^1 \/ 3/);
      // 사람처럼 지금 포커스가 있는 곳에 Enter — 포커스가 찾기 칸을 떠나면 다음 찾기가 안 된다.
      const focused = () => document.activeElement as HTMLElement;
      enter(focused());
      await settle(30);
      expect(label(root)).toMatch(/^2 \/ 3/);
      enter(focused());
      await settle(30);
      enter(focused());
      await settle(30);
      expect(label(root), '끝에서 처음으로').toMatch(/^1 \/ 3/);
      expect(count('editor_replace_text')).toBe(0);
      expect(texts).toEqual(['가나가나', '가']);
    });

    test('replace shows first, then replaces only what is shown, and moves on', async () => {
      texts = ['가나가나', '가'];
      const root = await open();
      click(root, 'find');
      findText(root).value = '가';
      root.querySelector<HTMLInputElement>('.replace-text')!.value = '다';
      click(root, 'replace-one');
      await settle(30);
      expect(count('editor_replace'), '첫 클릭은 보여 주기만').toBe(0);
      click(root, 'replace-one');
      await settle(30);
      expect(last('editor_replace')![1]).toMatchObject({ matches: [{ paragraph_id: 'paragraph:0', start: 0, end: 1 }], replacement: '다' });
      expect(texts).toEqual(['다나가나', '가']);
      expect(label(root), '다음 결과로').toMatch(/^1 \/ 2/);
      click(root, 'replace-one');
      await settle(30);
      expect(texts).toEqual(['다나다나', '가']);
    });

    test('an edit after finding makes replace find again instead of using stale offsets', async () => {
      texts = ['가나가나', '라'];
      const root = await open();
      click(root, 'find');
      findText(root).value = '가';
      click(root, 'find-next');
      await settle(30);
      // 다른 곳을 고친다(다시 굽기 → 새 판).
      clickAt(root, 49, 124);
      type(root, 'Z');
      await settle(500);
      const replaced = count('editor_replace');
      click(root, 'replace-one');
      await settle(30);
      expect(count('editor_replace'), '낡은 결과로 바꾸지 않는다').toBe(replaced);
      expect(count('editor_find')).toBe(2);
    });

    test('replace all with an empty text and a format keeps the text', async () => {
      texts = ['가나가나'];
      const root = await open();
      click(root, 'find');
      findText(root).value = '가';
      root.querySelector<HTMLSelectElement>('.replace-format')!.value = 'bold';
      click(root, 'replace-all');
      await settle(30);
      expect(last('editor_replace')![1]).toMatchObject({ replacement: '', format: 'bold' });
      expect(texts).toEqual(['가나가나']);
      expect(label(root)).toBe('2곳 바꿈');
    });
  });

  test('new opinion opens a blank opinion with its one identification item', async () => {
    const root = await open();
    root.querySelector<HTMLButtonElement>('[data-action="new-opinion"]')!.click();
    await settle(30);
    expect(last('editor_new_opinion')).toEqual(['editor_new_opinion', undefined]);
    expect(glyphs(root)).toBe('【의견내용】');
  });

  test('removed tools are gone: paragraph window, numbering, font size', async () => {
    const root = await open();
    for (const gone of ['[data-action="panel"]', '[data-action="number"]', '.font-size']) {
      expect(root.querySelector(gone), gone).toBeNull();
    }
    expect(root.querySelectorAll('.tb-group svg.tb-icon').length).toBeGreaterThan(10);
  });

  test('cmd+s saves over the opened file, shift+cmd+s saves as', async () => {
    await open();
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 's', metaKey: true, bubbles: true }));
    await settle(20);
    expect(last('editor_save')).toBeDefined();
    expect(last('editor_save_as')).toBeUndefined();
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'S', metaKey: true, shiftKey: true, bubbles: true }));
    await settle(20);
    expect(last('editor_save_as')).toBeDefined();
  });

  test('close document clears the page; cancelling keeps it', async () => {
    const root = await open();
    closeAnswer = false;
    root.querySelector<HTMLButtonElement>('[data-action="close-document"]')!.click();
    await settle(20);
    expect(root.querySelectorAll('.nk-page').length, '취소하면 그대로').toBeGreaterThan(0);
    closeAnswer = true;
    root.querySelector<HTMLButtonElement>('[data-action="close-document"]')!.click();
    await settle(20);
    expect(root.querySelectorAll('.nk-page')).toHaveLength(0);
    expect(root.querySelector<HTMLButtonElement>('[data-action="save"]')!.disabled, '저장은 다시 잠긴다').toBe(true);
    expect(root.querySelector<HTMLButtonElement>('[data-action="open"]')!.disabled).toBe(false);
  });

  test('alignment buttons set the caret paragraph', async () => {
    const root = await open();
    clickAt(root, 63, 124); // 둘째 문단
    root.querySelector<HTMLButtonElement>('[data-action="align-center"]')!.click();
    await settle(20);
    expect(last('editor_set_alignment')).toEqual(['editor_set_alignment', { paragraphId: 'paragraph:1', align: 'center' }]);
  });

  test('a lost mouseup does not keep dragging the caret (the view would jump back to it)', async () => {
    const root = await open();
    clickAt(root, 49, 104);
    // 뗌을 놓쳤다: 단추를 누르지 않은 채 움직인다.
    const host = root.querySelector('.nk-page')!;
    host.dispatchEvent(new MouseEvent('mousemove', { bubbles: true, buttons: 0, clientX: 80 * SCALE, clientY: 104 * SCALE }));
    expect(root.querySelectorAll('.nk-sel')).toHaveLength(0);
  });

  test('a selection spans paragraphs: painted per line, formatted at once, deleted in one edit', async () => {
    const root = await open();
    clickAt(root, 63, 104); // 첫 문단 「나」 앞(1)
    root.querySelector('.nk-page')!.dispatchEvent(
      new MouseEvent('mousedown', { bubbles: true, button: 0, shiftKey: true, clientX: 63 * SCALE, clientY: 124 * SCALE }),
    ); // 둘째 문단 「마」 앞(1)
    expect(root.querySelectorAll('.nk-sel')).toHaveLength(2);
    root.querySelector('.caret-input')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'b', metaKey: true, bubbles: true }));
    await settle(20);
    expect(last('editor_toggle_format_spans')).toEqual([
      'editor_toggle_format_spans',
      { spans: [['paragraph:0', 1, 3], ['paragraph:1', 0, 1]], format: 'bold' },
    ]);
    type(root, 'Q'); // 덮어 치기 = 지우기 한 번 + 그 자리에 친 글
    await settle(600);
    expect(calls.filter(([command]) => command === 'editor_delete_range')).toEqual([
      ['editor_delete_range', { fromId: 'paragraph:0', fromOffset: 1, toId: 'paragraph:1', toOffset: 1 }],
    ]);
    expect(texts).toEqual(['가Q마']);
  });

  test('right-click opens an editing menu: moves the caret outside the selection, keeps it inside', async () => {
    const root = await open();
    const host = root.querySelector('.nk-page')!;
    const rightClick = (x: number, y: number) =>
      host.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: x * SCALE, clientY: y * SCALE }));
    rightClick(63, 124); // 고른 것 없이 둘째 문단
    await settle(10);
    const first = menus.at(-1)!;
    const entry = (items: MenuItem[], text: string) => items.find((item) => item.text === text)!;
    expect(entry(first, '복사').enabled).toBe(false);
    expect(first.some((item) => item.item === 'Paste')).toBe(true);
    // 첫 문단 1..3 을 고르고 그 안을 우클릭 → 고른 것이 남고 굵게가 고른 곳에 걸린다.
    clickAt(root, 63, 104);
    host.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, button: 0, shiftKey: true, clientX: 87 * SCALE, clientY: 104 * SCALE }));
    rightClick(70, 104);
    await settle(10);
    const second = menus.at(-1)!;
    expect(entry(second, '복사').enabled).toBe(true);
    entry(second, '굵게').action!();
    await settle(20);
    expect(last('editor_toggle_format')).toEqual([
      'editor_toggle_format',
      { paragraphId: 'paragraph:0', start: 1, end: 3, format: 'bold' },
    ]);
    // ⌘T = 새 탭.
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 't', metaKey: true, bubbles: true }));
    await settle(10);
    expect(last('editor_new_tab')).toBeDefined();
  });

  test('closing sends typed text first, then asks the backend (which prompts to save)', async () => {
    const root = await open();
    clickAt(root, 63, 104);
    type(root, '김');
    await handlers.get('close-requested')!({ payload: 'other-tab' }); // 다른 탭 몫은 무시
    expect(last('editor_close')).toBeUndefined();
    await handlers.get('close-requested')!({ payload: 'main' });
    const order = calls.map(([command]) => command).filter((command) => command === 'editor_replace_text' || command === 'editor_close');
    expect(order).toEqual(['editor_replace_text', 'editor_close']);
  });

  test('a character the backend rejects is taken back off the page and explained', async () => {
    const root = await open();
    clickAt(root, 63, 104);
    type(root, '≈');
    await settle(10); // CP949 밖일 수 있는 글자는 바탕에 한 번 물어본 뒤 넣는다
    expect(glyphs(root)).toBe('가≈나다라마');
    await settle(600);
    expect(glyphs(root)).toBe('가나다라마');
    expect(root.querySelector('.editor-error')!.textContent).toContain('「≈」(U+2248)는 통특기 문자 집합');
    type(root, '김'); // 다음 고침은 다시 된다(거절된 글자를 다시 보내지 않는다)
    await settle(600);
    expect(texts[0]).toBe('가김나다');
  });

  test('typing or pasting a character outside CP949 asks in a review table first', async () => {
    const root = await open();
    clickAt(root, 63, 104);
    type(root, 'a\u2212b');
    await settle(10);
    const box = document.querySelector<HTMLFormElement>('.cp949-review')!;
    expect(box).not.toBeNull();
    expect(box.querySelector('.cp949-char')!.textContent).toBe('\u2212');
    expect(box.querySelector<HTMLInputElement>('[data-row="0"]')!.value).toBe('-');
    type(root, '김'); // 표가 떠 있는 동안 친 글은 기다렸다가 뒤에 들어간다
    box.querySelector<HTMLButtonElement>('button[type="submit"]')!.click();
    await settle(10);
    expect(document.querySelector('.cp949-review')).toBeNull();
    expect(glyphs(root)).toBe('가a-b김나다라마');

    // 대응 없는 글자(⁵): 칸이 비면 못 닫는다 · 취소하면 넣지 않는다.
    type(root, 'x\u2075');
    await settle(10);
    const second = document.querySelector<HTMLFormElement>('.cp949-review')!;
    second.querySelector<HTMLButtonElement>('button[type="submit"]')!.click();
    await settle(10);
    expect(second.querySelector('.cp949-error')!.textContent).toContain('U+2075');
    second.querySelector<HTMLButtonElement>('.cp949-cancel')!.click();
    await settle(10);
    expect(document.querySelector('.cp949-review')).toBeNull();
    expect(glyphs(root)).toBe('가a-b김나다라마');
    // 친 글의 반영(지연 저장)까지 끝낸다 — 남겨 두면 뒤 테스트 도중에 나가 그 테스트의 문서를 바꾼다(가짜 문서는 공유).
    await settle(500);
  });

  test('a picture is picked by clicking it and resized by dragging its corner, ratio locked', async () => {
    pageImages = [{ object_code: 3, image: 'image:0', x: 50, top: 200, width: 100, height: 50 }];
    URL.createObjectURL = () => 'blob:x';
    const root = await open();
    const picture = root.querySelector<HTMLImageElement>('img.nk-image')!;
    expect(picture.dataset.image).toBe('image:0');
    picture.getBoundingClientRect = () => ({ left: 65, top: 260, right: 195, bottom: 325 }) as DOMRect;
    picture.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, button: 0, clientX: 100, clientY: 300 }));
    const handle = root.querySelector<HTMLElement>('.nk-image-handle')!;
    expect(handle).not.toBeNull();
    // 오른쪽으로 40px · 아래로 조금 → 가로를 따르고 세로는 비율대로.
    handle.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, button: 0, clientX: 195, clientY: 325 }));
    window.dispatchEvent(new MouseEvent('mousemove', { clientX: 235, clientY: 330 }));
    const width = 100 * SCALE + 40;
    expect(picture.style.width).toBe(`${width}px`);
    expect(parseFloat(picture.style.height)).toBeCloseTo(width / 2);
    window.dispatchEvent(new MouseEvent('mouseup'));
    await settle(10);
    const args = last('editor_resize_image')![1] as { image: string; scale: number };
    expect(args.image).toBe('image:0');
    expect(args.scale).toBeCloseTo(width / (100 * SCALE));
    // 그림 밖을 누르면 고르기가 풀린다.
    clickAt(root, 63, 104);
    expect(root.querySelector('.nk-image-frame')).toBeNull();
  });

  test('picture into a table cell: the menu works in a cell and inserts at the caret offset there', async () => {
    cellLines = [{ cell: 'cell:0:0:1', text: '가나' }];
    const root = await open();
    clickAt(root, 63, 144); // 칸 줄 「가|나」
    await settle(10);
    root.querySelector('.nk-page')!.dispatchEvent(
      new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 63 * SCALE, clientY: 144 * SCALE }),
    );
    await settle(10);
    const item = menus.at(-1)!.find((entry) => entry.text === '이미지 파일 선택하기')!;
    expect(item.enabled).toBe(true);
    item.action!();
    await settle(10);
    expect(last('editor_insert_picture')).toEqual(['editor_insert_picture', { after: 'cell:0:0:1', offset: 1 }]);
    // 문단에서는 칸 자리 없이 문단 뒤로.
    clickAt(root, 63, 104);
    await settle(10);
    root.querySelector('.nk-page')!.dispatchEvent(
      new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 63 * SCALE, clientY: 104 * SCALE }),
    );
    await settle(10);
    menus.at(-1)!.find((entry) => entry.text === '이미지 파일 선택하기')!.action!();
    await settle(10);
    expect(last('editor_insert_picture')).toEqual(['editor_insert_picture', { after: 'paragraph:0', offset: null }]);
  });

  test('picture size by numbers: right-click a picture → mm fields, ratio locked → resize by that scale', async () => {
    // 그린 크기 71.24 × 35.24pt → 표시 72 × 36pt = 25.4 × 12.7mm.
    pageImages = [{ object_code: 3, image: 'image:0', x: 50, top: 200, width: 71.24, height: 35.24 }];
    URL.createObjectURL = () => 'blob:x';
    const root = await open();
    const menu = async (x: number, y: number) => {
      // 다시 그리면 그림 요소가 새로 생긴다.
      const picture = root.querySelector<HTMLImageElement>('img.nk-image')!;
      picture.getBoundingClientRect = () => ({ left: 65, top: 260, right: 195, bottom: 325 }) as DOMRect;
      root.querySelector('.nk-page')!.dispatchEvent(
        new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: x, clientY: y }),
      );
      await settle(10);
      return menus.at(-1)!.find((item) => item.text === '그림 크기…')!;
    };
    expect((await menu(30, 30)).enabled).toBe(false); // 그림 밖
    (await menu(100, 300)).action!();
    const across = root.querySelector<HTMLInputElement>('.picture-width')!;
    const down = root.querySelector<HTMLInputElement>('.picture-height')!;
    expect([across.value, down.value]).toEqual(['25.4', '12.7']);
    across.value = '50.8';
    across.dispatchEvent(new Event('input'));
    expect(down.value).toBe('25.4');
    down.value = '6.35';
    down.dispatchEvent(new Event('input'));
    expect(across.value).toBe('12.7');
    root.querySelector<HTMLFormElement>('.item-ask')!.requestSubmit();
    await settle(10);
    const args = last('editor_resize_image')![1] as { image: string; scale: number };
    expect(args.image).toBe('image:0');
    expect(args.scale).toBeCloseTo(0.5);
    expect(root.querySelector('.item-ask')).toBeNull();
    // 취소는 부르지 않는다.
    const count = calls.filter(([command]) => command === 'editor_resize_image').length;
    (await menu(100, 300)).action!();
    root.querySelector<HTMLButtonElement>('.item-cancel')!.click();
    await settle(10);
    expect(calls.filter(([command]) => command === 'editor_resize_image').length).toBe(count);
  });

  test('replace picture: right-click a picture → 그림 바꾸기… replaces that image (off a picture it is disabled)', async () => {
    pageImages = [{ object_code: 3, image: 'image:0', x: 50, top: 200, width: 71.24, height: 35.24 }];
    URL.createObjectURL = () => 'blob:x';
    const root = await open();
    const menu = async (x: number, y: number) => {
      const picture = root.querySelector<HTMLImageElement>('img.nk-image')!;
      picture.getBoundingClientRect = () => ({ left: 65, top: 260, right: 195, bottom: 325 }) as DOMRect;
      root.querySelector('.nk-page')!.dispatchEvent(
        new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: x, clientY: y }),
      );
      await settle(10);
      return menus.at(-1)!.find((item) => item.text === '그림 바꾸기…')!;
    };
    expect((await menu(30, 30)).enabled).toBe(false);
    (await menu(100, 300)).action!();
    await settle(10);
    expect(last('editor_replace_picture')).toEqual(['editor_replace_picture', { image: 'image:0' }]);
  });

  test('review report: 보정안 → pick a proposal and the typo fixes (right word editable) → applied in one edit', async () => {
    const root = await open();
    root.querySelector<HTMLButtonElement>('[data-action="review"]')!.click();
    await settle(10);
    const box = root.querySelector<HTMLFormElement>('.review-ask')!;
    expect(box.querySelectorAll('input[name="proposal"]').length).toBe(3); // 두 안 + 걸지 않음
    expect(box.textContent).toContain('바뀐 항 1');
    expect(box.textContent).toContain('추적 변경 없음');
    expect(box.textContent).toContain('도 5');
    // 바른 말이 없는 후보 · 한 자리만 고치라는 후보는 꺼져 있다 · 걸릴 곳 수를 보여 준다.
    const checks = [...box.querySelectorAll<HTMLInputElement>('input[type="checkbox"]')];
    expect(checks.map((c) => c.checked)).toEqual([true, false, true, false]);
    expect(box.textContent).toContain('(3곳)');
    expect(box.textContent).toContain('한 자리만');
    box.querySelector<HTMLInputElement>('input[name="proposal"][value="1"]')!.checked = true;
    checks[2].checked = false;
    box.querySelector<HTMLInputElement>('.review-right[data-note="1"]')!.value = '…';
    checks[1].checked = true;
    box.requestSubmit();
    await settle(30);
    expect(last('editor_apply_review')).toEqual([
      'editor_apply_review',
      {
        claims: [{ number: '2', before: ['다'], after: [] }],
        tracked: false,
        notes: [
          { paragraphs: ['0001'], wrong: '가나', right: '가다', sentence: '…', positional: false },
          { paragraphs: ['0002'], wrong: '갉', right: '…', sentence: '…', positional: false },
        ],
      },
    ]);
    expect(root.querySelector('.review-ask')).toBeNull();
    expect(root.querySelector('.editor-status')!.textContent).toContain('삭제 2 · 오기 4곳 정정 · 도면 1개');
    await settle(500);
  });

  test('identification item: right-click submenu → number (next one filled in) → heading + empty paragraph below', async () => {
    texts = ['가나다', '【청구항 1】', '라마'];
    const root = await open();
    clickAt(root, 63, 104); // 첫 문단 가운데 → 아래에 넣는다
    root.querySelector('.nk-page')!.dispatchEvent(
      new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 63 * SCALE, clientY: 104 * SCALE }),
    );
    await settle(10);
    const items = menus.at(-1)!.find((item) => item.text === '식별항목')!.items!;
    expect(items.map((item) => item.text)).toEqual(['도', '표', '청구항', '수학식', '화학식', '반응식', '참고도']);
    items.find((item) => item.text === '청구항')!.action!();
    await settle(10);
    const field = root.querySelector<HTMLInputElement>('.item-number')!;
    expect(field.value).toBe('2');
    field.value = '2-1';
    root.querySelector<HTMLFormElement>('.item-ask')!.requestSubmit();
    expect(root.querySelector('.item-hint')!.textContent).toContain('영문과 숫자만');
    field.value = '1'; // 이미 있는 번호 — 넣고 알린다
    root.querySelector<HTMLFormElement>('.item-ask')!.requestSubmit();
    await settle(30);
    expect(last('editor_insert_item')).toEqual([
      'editor_insert_item',
      { paragraphId: 'paragraph:0', before: false, kind: '청구항', number: '1' },
    ]);
    expect(texts).toEqual(['가나다', '【청구항 1】', '', '【청구항 1】', '라마']);
    expect(root.querySelector('.editor-error')!.textContent).toContain('E-243');
    type(root, '본문'); // 커서는 새 빈 문단에
    await settle(500);
    expect(texts[2]).toBe('본문');
  });

  test('paragraph marks: ↵ at every paragraph end while on, gone when off', async () => {
    texts = ['가나다', '', '라마'];
    const root = await open();
    const marks = () => root.querySelectorAll('.nk-pmark').length;
    expect(marks()).toBe(0);
    root.querySelector<HTMLButtonElement>('[data-action="marks"]')!.click();
    await settle(10);
    expect(marks()).toBe(3); // 빈 문단도
    expect(root.querySelector('[data-action="marks"]')!.getAttribute('aria-pressed')).toBe('true');
    root.querySelector<HTMLButtonElement>('[data-action="marks"]')!.click();
    await settle(10);
    expect(marks()).toBe(0);
  });

  test('식별항목 삭제 = 통특기 단락삭제 창: 종류 + 번호(쉼표 · 범위) · 표제 위면 채워 둔다 · 청구항은 내용까지', async () => {
    texts = ['가나다', '【도 1】', '', '【청구항 3】', '제1항에 있어서,', '라마', '【청구항 5】', '바사', '【청구항 6】', '아자', '【요약서】'];
    const root = await open();
    const rightClick = (y: number) =>
      root.querySelector('.nk-page')!.dispatchEvent(
        new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 55 * SCALE, clientY: y * SCALE }),
      );
    const removal = () => menus.at(-1)!.find((item) => item.text === '식별항목 삭제')!;
    rightClick(124); // 【도 1】 위 — 도 1 을 채워 둔다
    await settle(10);
    expect(removal().enabled).toBe(true);
    removal().action!();
    await settle(10);
    const form = root.querySelector<HTMLFormElement>('.item-remove')!;
    expect(form.querySelector<HTMLSelectElement>('select')!.value).toBe('도');
    expect(form.querySelector<HTMLInputElement>('.item-numbers')!.value).toBe('1');
    form.querySelector<HTMLSelectElement>('select')!.value = '청구항';
    form.querySelector<HTMLInputElement>('.item-numbers')!.value = '3, 5-6';
    form.requestSubmit();
    await settle(30);
    expect(last('editor_remove_items')).toEqual(['editor_remove_items', { kind: '청구항', numbers: ['3', '5', '6'] }]);
    expect(texts).toEqual(['가나다', '【도 1】', '', '【요약서】']);
  });

  test('a final version shows [0001] to the left of a numbered paragraph only', async () => {
    texts = ['【기술분야】', '가나다', '라마'];
    numbers = [null, '0001', null];
    const root = await open();
    const shown = Array.from(root.querySelectorAll('.nk-pnum')).map((mark) => mark.textContent);
    expect(shown).toEqual(['[0001]']);
  });

  test('shift-arrows select and Cmd+B formats the page selection', async () => {
    vi.mocked(invoke).mockClear();
    const root = await open();
    clickAt(root, 49, 104);
    press(root, 'ArrowRight');
    root.querySelector('.caret-input')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', shiftKey: true, bubbles: true }));
    expect(root.querySelectorAll('.nk-sel')).toHaveLength(1);
    root.querySelector('.caret-input')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'b', metaKey: true, bubbles: true }));
    await settle(20);
    expect(last('editor_toggle_format')).toEqual([
      'editor_toggle_format',
      { paragraphId: 'paragraph:0', start: 1, end: 2, format: 'bold' },
    ]);
  });
});
