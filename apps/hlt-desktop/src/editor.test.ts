// @vitest-environment jsdom
// 쪽 위 편집(WYSIWYG) 흐름: 쪽을 눌러 커서 → 치면 바로 보이고 0.4초 뒤 문단 고침 한 번 → 엔터 = 문단 나누기 →
// 문단 첫머리 Backspace = 앞 문단에 붙이기. 다시 굽는 동안 친 키는 버리지 않는다. 백엔드는 가짜다.
import { beforeEach, describe, expect, test, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import type { EditorViewDto, PlannedImageDto, PlannedPageDto } from './contracts';
import { AUTOSAVE_MS, basisDraft, goTarget, mountEditor, toCsv } from './editor';
import { sectionOf } from './editor-view';

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
let marksAnswer: unknown = null;
let issuesAnswer: unknown[] = [];
let parentsAnswer: Record<string, number[]> = {};
let compareAnswer: string | null = null;
let amendAnswer: unknown = null;
let batchAnswer: unknown = null;
let sourceAnswer: unknown = null;
let findError: unknown = null;
let renumberAnswer: unknown[] = [];
let signAnswer: unknown[] = [];
let keysAnswer: (string | null)[] = [];
let basisAnswer: unknown = null;
let submitAnswer: unknown[] = [];
let settingsAnswer: unknown = null;
let historyAnswer: unknown[] = [];
let saveError: unknown = null;
/** 서열목록 넣기 답: 'insert' = 넣는다 · 'cancel' = 파일 고르기 취소 · 그 밖 = 그 값을 던진다. */
let sequenceAnswer: unknown = 'insert';
const calls: [string, unknown][] = [];

beforeEach(() => {
  texts = ['가나다', '라마'];
  pageImages = [];
  cellLines = [];
  numbers = [];
  closeAnswer = true;
  marksAnswer = null;
  issuesAnswer = [];
  parentsAnswer = {};
  compareAnswer = null;
  amendAnswer = null;
  batchAnswer = null;
  sourceAnswer = null;
  findError = null;
  renumberAnswer = [];
  signAnswer = [];
  keysAnswer = [];
  basisAnswer = null;
  submitAnswer = [];
  settingsAnswer = null;
  historyAnswer = [];
  saveError = null;
  sequenceAnswer = 'insert';
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
      case 'editor_amendment_marks':
        return marksAnswer;
      case 'editor_claim_issues':
        return issuesAnswer;
      case 'editor_claim_parents':
        return parentsAnswer;
      case 'editor_compare_pick':
        return compareAnswer;
      case 'editor_amendment_basis':
        return basisAnswer;
      case 'editor_submission_checks':
        return submitAnswer;
      case 'editor_export_fin':
        return '/tmp/x.fin';
      case 'editor_export_amendment':
        return amendAnswer;
      case 'editor_batch_generate':
      case 'editor_batch_check':
        return batchAnswer;
      case 'editor_source_open':
        return sourceAnswer;
      case 'editor_paragraph_keys':
        return keysAnswer;
      case 'editor_sign_table':
        return signAnswer;
      case 'editor_renumber_plan':
        return renumberAnswer;
      case 'editor_replace_texts':
      case 'editor_fix_double_spaces':
        return view(texts);
      case 'editor_save_csv':
        return '/d/표.csv';
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
        if (findError) throw findError;
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
      case 'editor_set_indent':
      case 'editor_toggle_format':
        return view(texts);
      case 'editor_new_opinion':
        texts = ['【의견내용】', ''];
        return view(texts);
      case 'editor_save':
      case 'editor_save_as':
        if (saveError) throw saveError;
        return '/tmp/x.HLT';
      case 'settings_get':
        return settingsAnswer;
      case 'settings_set':
        return args!.settings;
      case 'editor_history_list':
        return historyAnswer;
      case 'editor_history_open':
        return null;
      case 'editor_open_read_only':
        return { ...view(texts), editable: false, read_only: true };
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
      case 'editor_insert_sequence_listing': {
        if (sequenceAnswer === 'cancel') return null;
        if (sequenceAnswer !== 'insert') throw sequenceAnswer;
        const claims = texts.findIndex((text) => text.includes('【청구범위】'));
        const at = claims < 0 ? texts.length : claims;
        texts.splice(at, 0, '【서열목록】', '서열목록 전자파일 첨부(/d/seq.xml)', '');
        return { view: view(texts), body: `paragraph:${at + 1}` };
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

const q = (root: HTMLElement, selector: string) => root.querySelector<HTMLButtonElement>(selector)!;
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
      expect(last('editor_find')).toEqual(['editor_find', { query: { text: '', format: 'bold', case_insensitive: false, whole_word: false, regex: false } }]);
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

  test('review report: 「N곳」 folds the window and walks the places inside those numbered paragraphs (find bar) · leftovers after applying', async () => {
    texts = ['제1 가', '제1 나 제1', '제1 다', '끝'];
    numbers = [null, '0004', '0003', null];
    const root = await open();
    root.querySelector<HTMLButtonElement>('[data-action="review"]')!.click();
    await settle(10);
    const box = root.querySelector<HTMLFormElement>('.review-ask')!;
    // 찾기 막대에 고른 범위가 있어도 「N곳」은 그 번호 문단 전체에서(고른 범위를 「전체」로).
    const range = root.querySelector<HTMLSelectElement>('.find-range')!;
    range.value = 'claims';
    range.dispatchEvent(new Event('change'));
    // 한 자리만 고치라는 정정(꺼짐) 「2곳」 → 창을 접고 찾기 막대로 · 범위 밖 같은 말은 세지 않는다.
    box.querySelector<HTMLButtonElement>('.review-goto[data-note="3"]')!.click();
    await settle(10);
    expect(box.hidden).toBe(true);
    const bar = root.querySelector<HTMLElement>('.find-bar')!;
    expect(bar.hidden).toBe(false);
    expect(root.querySelector<HTMLInputElement>('.find-text')!.value).toBe('제1');
    expect(root.querySelector<HTMLElement>('.find-scope')!.textContent).toContain('범위 【0004】');
    const label = root.querySelector('.find-count')!;
    expect(label.textContent).toMatch(/^1 \/ 2/);
    root.querySelector<HTMLButtonElement>('[data-action="find-next"]')!.click();
    await settle(10);
    expect(label.textContent).toMatch(/^2 \/ 2/);
    // 모두 바꾸기도 범위 안만.
    root.querySelector<HTMLInputElement>('.replace-text')!.value = '제1';
    root.querySelector<HTMLButtonElement>('[data-action="replace-all"]')!.click();
    await settle(10);
    const replaced = last('editor_replace')![1] as { matches: { paragraph_id: string }[] };
    expect(replaced.matches.map((m) => m.paragraph_id)).toEqual(['paragraph:1', 'paragraph:1']);
    // 띠 → 창이 다시 보인다(고른 것 그대로) · 걸면 띠도 사라진다.
    root.querySelector<HTMLButtonElement>('.review-reopen')!.click();
    expect(box.hidden).toBe(false);
    expect(root.querySelector('.review-reopen')).toBeNull();
    box.querySelector<HTMLButtonElement>('.review-goto[data-note="3"]')!.click();
    await settle(10);
    box.requestSubmit();
    await settle(30);
    expect(root.querySelector('.review-reopen')).toBeNull();
    // 걸지 않은 정정 가운데 그 자리에 남은 것(【0004】의 「제1」 2곳 · 【0002】는 문서에 없다).
    expect(root.querySelector<HTMLElement>('.find-scope')!.hidden).toBe(true);
    const left = root.querySelector<HTMLButtonElement>('.review-left')!;
    expect(left.hidden).toBe(false);
    expect(left.textContent).toBe('남은 오기 2곳');
    bar.hidden = true;
    left.click();
    await settle(10);
    expect(bar.hidden).toBe(false);
    expect(root.querySelector<HTMLElement>('.find-scope')!.textContent).toContain('【0004】');
    expect(label.textContent).toMatch(/ \/ 2/);
    // 찾을 글을 새로 치면 범위가 풀려 문서 전체에서.
    const field = root.querySelector<HTMLInputElement>('.find-text')!;
    field.value = '제1';
    field.dispatchEvent(new Event('input'));
    root.querySelector<HTMLButtonElement>('[data-action="find-next"]')!.click();
    await settle(10);
    expect(root.querySelector<HTMLElement>('.find-scope')!.hidden).toBe(true);
    expect(label.textContent).toMatch(/ \/ 4/);
    await settle(500);
  });

  test('⌘G 가기: 식별번호 → caret at that paragraph start (typing goes there) · unknown keeps the box with a hint', async () => {
    texts = ['【기술분야】', '가나다', '라마'];
    numbers = [null, '0001', '0002'];
    const root = await open();
    const go = () => document.dispatchEvent(new KeyboardEvent('keydown', { key: 'g', metaKey: true, bubbles: true }));
    go();
    let box = root.querySelector<HTMLFormElement>('.go-ask')!;
    const field = box.querySelector<HTMLInputElement>('.go-text')!;
    field.value = '9';
    box.requestSubmit();
    expect(root.querySelector('.go-ask')).not.toBeNull();
    expect(box.querySelector('.item-hint')!.textContent).toContain('「9」이(가) 없습니다');
    field.value = '【0002】';
    box.requestSubmit();
    expect(root.querySelector('.go-ask')).toBeNull();
    expect(root.querySelector('.editor-status')!.textContent).toBe('가기: 【0002】');
    type(root, 'X');
    await settle(500);
    expect(last('editor_replace_text')).toEqual(['editor_replace_text', expect.objectContaining({ paragraphId: 'paragraph:2', text: 'X라마' })]);
    go();
    box = root.querySelector<HTMLFormElement>('.go-ask')!;
    box.querySelector('.go-text')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    expect(root.querySelector('.go-ask')).toBeNull();
  });

  test('개요: 마디를 누르면 그 문단에 커서 · 화면 맨 위 문단이 든 절이 강조된다', async () => {
    texts = ['【발명의 설명】', '가나', '【청구범위】', '【청구항 1】', '가나', '【청구항 2】', '라마'];
    const root = await open();
    const label = (name: string) =>
      [...root.querySelectorAll<HTMLButtonElement>('.tree-label')].find((each) => each.textContent === name)!;
    label('청구항 2').click();
    type(root, 'X');
    await settle(500);
    expect(last('editor_replace_text')).toEqual(['editor_replace_text', expect.objectContaining({ paragraphId: 'paragraph:5' })]);
    // 화면 맨 위 = 넷째 줄(【청구항 1】 · 줄 top = 100 + 20 × 3).
    const pagesRoot = root.querySelector<HTMLElement>('.editor-pages')!;
    pagesRoot.getBoundingClientRect = () => ({ top: 0, bottom: 800 }) as DOMRect;
    root.querySelector<HTMLElement>('.nk-page')!.getBoundingClientRect = () => ({ top: -160 * 1.3, bottom: 900 }) as DOMRect;
    pagesRoot.dispatchEvent(new Event('scroll'));
    await settle(50);
    expect([...root.querySelectorAll('.tree-label.current')].map((each) => each.textContent)).toEqual(['청구항 1']);
  });

  test('청구항 나무: 종속항은 첫 인용 항 아래 · 다중 인용 · 종속 수 · 누르면 그 항으로', async () => {
    texts = ['【청구범위】', '【청구항 1】', '가', '【청구항 2】', '나', '【청구항 3】', '다', '【청구항 4】', '라', '【청구항 5】', '마'];
    parentsAnswer = { '1': [], '2': [1], '3': [1, 2], '4': [], '5': [3] };
    const root = await open();
    await settle(50);
    const row = (name: string) =>
      [...root.querySelectorAll<HTMLLIElement>('.tree li')].find((li) => li.querySelector(':scope > .tree-row .tree-label')!.firstChild?.nextSibling?.textContent === name)!;
    const kids = (name: string) =>
      [...row(name).querySelectorAll(':scope > ul > li > .tree-row .tree-label')].map((label) => label.textContent);
    expect(kids('청구범위')).toEqual(['청구항 1종속 3', '청구항 4']);
    expect(kids('청구항 1')).toEqual(['청구항 2', '청구항 3제1·2항 · 종속 1']);
    expect(kids('청구항 2')).toEqual([]);
    expect(kids('청구항 3')).toEqual(['청구항 5']);
    row('청구항 3').querySelector<HTMLButtonElement>('.tree-label')!.click();
    type(root, 'X');
    await settle(500);
    expect(last('editor_replace_text')).toEqual(['editor_replace_text', expect.objectContaining({ paragraphId: 'paragraph:5' })]);
  });

  test('자동 저장: 판이 바뀐 뒤 2분 손을 멈추면 한 번 · 그 사이 고치면 다시 센다', async () => {
    const root = await open();
    vi.useFakeTimers();
    try {
      const saves = () => calls.filter(([c]) => c === 'editor_autosave').length;
      clickAt(root, 63, 104);
      type(root, 'X');
      await vi.advanceTimersByTimeAsync(500); // 친 글을 보내 판이 바뀐다
      vi.advanceTimersByTime(60_000);
      type(root, 'Y');
      await vi.advanceTimersByTimeAsync(500); // 다시 고침 → 처음부터 센다
      vi.advanceTimersByTime(AUTOSAVE_MS - 60_000);
      expect(saves()).toBe(0);
      vi.advanceTimersByTime(60_000);
      expect(saves()).toBe(1);
    } finally {
      vi.useRealTimers();
    }
  });

  test('일괄: 메뉴 → 폴더 HLT 만들기(진행 · 결과 표 · 요약) · 폴더 점검(종류별 열 · CSV)', async () => {
    const root = await open();
    q(root, '[data-action="batch"]').click();
    await settle(10);
    const items = menus[menus.length - 1] as unknown as { text: string; action: () => void }[];
    expect(items.map((item) => item.text)).toEqual(['폴더 HLT 만들기…', '폴더 제출 전 점검…']);
    batchAnswer = [
      { file: 'a.docx', status: '만듦', detail: 'a.HLT' },
      { file: 'b.xml', status: '있음', detail: 'b.HLT' },
      { file: 'c.xml', status: '실패', detail: 'structure_invalid' },
    ];
    items[0].action();
    handlers.get('batch-progress')!({ payload: { at: 1, total: 3, file: 'b.xml' } });
    expect(root.querySelector('.batch-progress')!.textContent).toContain('일괄 2/3 · b.xml');
    await settle(10);
    expect(root.querySelector<HTMLElement>('.batch-progress')!.hidden).toBe(true);
    expect(root.querySelector('.editor-status')!.textContent).toBe('폴더 HLT 만들기 — 만듦 1 · 있음 1 · 실패 1');
    expect([...root.querySelectorAll('.batch-body tbody tr')].map((tr) => tr.textContent)).toEqual([
      'a.docx만듦a.HLT',
      'b.xml있음b.HLT',
      'c.xml실패structure_invalid',
    ]);
    q(root, '.batch-close').click();
    expect(root.querySelector('.batch-shade')).toBeNull();

    batchAnswer = [
      { file: 'a.HLT', counts: { sign_shared: 2, citation: 1 }, error: null },
      { file: 'z.fin', counts: {}, error: 'io_error' },
    ];
    items[1].action();
    await settle(10);
    const head = [...root.querySelectorAll('.batch-body th')].map((th) => th.textContent);
    expect(head[0]).toBe('파일');
    expect(head.slice(-2)).toEqual(['합계', '열지 못함']);
    const first = [...root.querySelectorAll('.batch-body tbody tr')][0];
    expect([...first.querySelectorAll('td')].map((td) => td.textContent)).toEqual(['a.HLT', '0', '0', '0', '0', '0', '2', '1', '0', '0', '0', '0', '0', '0', '0', '3', '']);
    expect(root.querySelector('.editor-status')!.textContent).toBe('폴더 제출 전 점검 — 2개 파일 · 걸린 파일 1');
    q(root, '.batch-csv').click();
    await settle(10);
    const [, args] = last('editor_save_csv')!;
    expect((args as { name: string }).name).toBe('제출 전 점검.csv');
    expect((args as { text: string }).text.split('\r\n')[1]).toBe('"a.HLT","0","0","0","0","0","2","1","0","0","0","0","0","0","0","3",""');
  });

  test('toCsv: 따옴표 · 쉼표 · 줄바꿈이 든 칸', () => {
    expect(toCsv([['a,b', '그는 "예"'], ['줄\n바꿈', '']])).toBe('"a,b","그는 ""예"""\r\n"줄\n바꿈",""');
  });

  test('원문과 나란히: 짝 수 · 짝 없는 원문은 흐리게 · 원문을 누르면 그 문단 · 스크롤을 따라 원문 · 다시 누르면 닫기', async () => {
    texts = ['【발명의 설명】', '【기술분야】', '가', '나', '【청구범위】', '【청구항 1】', '장치.'];
    sourceAnswer = [
      { key: null, text: 'TITLE' },
      { key: 'p:0001', text: '[0001] One.' },
      { key: 'p:0002', text: '[0002] Two.' },
      { key: 'p:0009', text: '[0009] Nine.' },
      { key: 'claim:1', text: 'A device.' },
    ];
    keysAnswer = [null, null, 'p:0001', 'p:0002', null, 'claim:1', 'claim:1'];
    const root = await open();
    q(root, '[data-action="source"]').click();
    await settle(10);
    const pane = root.querySelector<HTMLElement>('.editor-source')!;
    expect(pane.hidden).toBe(false);
    expect(root.querySelector('.editor-status')!.textContent).toBe('원문 4곳 중 3곳 짝지음(식별번호 · 청구항 번호)');
    expect([...pane.querySelectorAll('.unpaired')].map((p) => p.textContent)).toEqual(['[0009] Nine.']);
    [...pane.querySelectorAll<HTMLElement>('p')].find((p) => p.textContent === 'A device.')!.click();
    type(root, 'X');
    await settle(500);
    expect(last('editor_replace_text')).toEqual(['editor_replace_text', expect.objectContaining({ paragraphId: 'paragraph:5' })]);
    // 고친 뒤 판이 바뀌면 짝을 다시 받는다(이제 p:0009 도 짝이 있다).
    keysAnswer = [null, null, 'p:0001', 'p:0002', null, 'claim:1', 'claim:1', 'p:0009'];
    type(root, 'Y');
    await settle(500);
    expect(pane.querySelectorAll('.unpaired').length).toBe(0);
    // 화면 맨 위 = 다섯째 줄(【청구범위】 · 짝 이름 없음 → 앞 문단 「나」 · p:0002).
    const pagesRoot = root.querySelector<HTMLElement>('.editor-pages')!;
    pagesRoot.getBoundingClientRect = () => ({ top: 0, bottom: 800 }) as DOMRect;
    root.querySelector<HTMLElement>('.nk-page')!.getBoundingClientRect = () => ({ top: -180 * 1.3, bottom: 900 }) as DOMRect;
    pagesRoot.dispatchEvent(new Event('scroll'));
    await settle(50);
    expect(pane.querySelector('.current')!.textContent).toBe('[0002] Two.');
    q(root, '[data-action="source"]').click();
    await settle(10);
    expect(pane.hidden).toBe(true);
  });

  test('찾기 옵션: Aa · 단어 · .* 토글이 찾기 조건에 실리고 · 정규식 모두 바꾸기는 조건을 넘긴다 · 잘못된 정규식은 까닭', async () => {
    texts = ['제1항 제2항'];
    const root = await open();
    q(root, '[data-action="find"]').click();
    const field = root.querySelector<HTMLInputElement>('.find-text')!;
    field.value = '제1항';
    q(root, '.find-option[data-option="case_insensitive"]').click();
    q(root, '.find-option[data-option="regex"]').click();
    q(root, '[data-action="find-next"]').click();
    await settle(10);
    expect(last('editor_find')![1]).toEqual({
      query: { text: '제1항', format: null, case_insensitive: true, whole_word: false, regex: true },
    });
    root.querySelector<HTMLInputElement>('.replace-text')!.value = '청구항 $1';
    q(root, '[data-action="replace-all"]').click();
    await settle(10);
    expect((last('editor_replace')![1] as { pattern: unknown }).pattern).toEqual({
      text: '제1항', format: null, case_insensitive: true, whole_word: false, regex: true,
    });
    // 정규식을 끄면 넘기지 않는다.
    q(root, '.find-option[data-option="regex"]').click();
    field.value = '제2항';
    q(root, '[data-action="replace-all"]').click();
    await settle(10);
    expect((last('editor_replace')![1] as { pattern: unknown }).pattern).toBeNull();
    findError = { code: 'structure_invalid', details: { reason: 'find_pattern', message: 'unclosed group' } };
    field.value = '(';
    q(root, '[data-action="find-next"]').click();
    await settle(10);
    expect(root.querySelector('.find-count')!.textContent).toBe('찾을 글 오류: unclosed group');
  });

  test('도구 → 청구항 번호 다시 매기기: 미리 보기(바뀐 곳만) · 적용은 한 번에 · 이미 차례면 알림 · 최종본이면 경고', async () => {
    texts = ['【청구항 1】', '장치.', '【청구항 3】', '제1항에 있어서, 부재.'];
    const root = await open();
    const tool = async (label: string) => {
      q(root, '[data-action="tools"]').click();
      await settle(10);
      const items = menus[menus.length - 1] as unknown as { text: string; action: () => void }[];
      items.find((item) => item.text === label)!.action();
      await settle(10);
    };
    renumberAnswer = [];
    await tool('청구항 번호 다시 매기기…');
    expect(root.querySelector('.editor-status')!.textContent).toBe('청구항 번호가 이미 차례대로입니다.');
    const long = '부재를 포함하고, 상기 부재는 기판 위에 놓이는 것을 특징으로 하는, 제3항에 있어서 그 밖의 부재를 더 포함하는 장치.';
    renumberAnswer = [
      { id: 'paragraph:2', before: '【청구항 3】', after: '【청구항 2】' },
      { id: 'paragraph:3', before: long, after: long.replace('제3항', '제2항') },
    ];
    await tool('청구항 번호 다시 매기기…');
    const shade = root.querySelector('.renumber-shade')!;
    expect(shade.querySelector('strong')!.textContent).toBe('청구항 번호 다시 매기기 — 2곳');
    expect([...shade.querySelectorAll('tbody td')].map((td) => td.textContent)).toEqual([
      '【청구항 3】',
      '【청구항 2】',
      '…으로 하는, 제3항에 있어서 그…',
      '…으로 하는, 제2항에 있어서 그…',
    ]);
    expect(shade.querySelector('.renumber-warn')).toBeNull();
    q(root, '.renumber-apply').click();
    await settle(10);
    expect((last('editor_replace_texts')![1] as { edits: string[][] }).edits[0]).toEqual(['paragraph:2', '【청구항 2】']);
    expect(root.querySelector('.renumber-shade')).toBeNull();
  });

  test('번호 다시 매기기: 최종본(식별번호가 있는 문서)이면 보정 실무 경고', async () => {
    texts = ['【청구항 1】', '장치.', '【청구항 3】', '제1항에 있어서.'];
    numbers = ['0001', null, null, null];
    const root = await open();
    renumberAnswer = [{ id: 'paragraph:2', before: '【청구항 3】', after: '【청구항 2】' }];
    q(root, '[data-action="tools"]').click();
    await settle(10);
    (menus[menus.length - 1] as unknown as { text: string; action: () => void }[])[0].action();
    await settle(10);
    expect(root.querySelector('.renumber-warn')!.textContent).toContain('보정에서는 지운 청구항의 번호를 다시 매기지 않고');
  });

  test('도구 → 부호 목록: 표(부호 · 이름(수) · 부호의 설명 · 수) · 가기는 처음 나온 문단으로', async () => {
    texts = ['가나다', '몸체(10)', '덮개'];
    const root = await open();
    signAnswer = [
      { sign: '10', names: [['몸체', 2], ['본체', 1]], listed: '몸체', count: 3, first: 'paragraph:1' },
      { sign: '30', names: [], listed: '남은 부재', count: 0, first: 'paragraph:2' },
    ];
    q(root, '[data-action="tools"]').click();
    await settle(10);
    (menus[menus.length - 1] as unknown as { text: string; action: () => void }[]).find((item) => item.text === '부호 목록')!.action();
    await settle(10);
    const shade = root.querySelector('.sign-shade')!;
    expect(shade.querySelector('strong')!.textContent).toBe('부호 목록 — 2개');
    const first = [...shade.querySelectorAll('tbody tr')][0];
    expect([...first.querySelectorAll('td')].slice(0, 4).map((td) => td.textContent)).toEqual(['10', '몸체(2) · 본체(1)', '몸체', '3']);
    q(shade as HTMLElement, '.table-go').click();
    type(root, 'X');
    await settle(500);
    expect(last('editor_replace_text')).toEqual(['editor_replace_text', expect.objectContaining({ paragraphId: 'paragraph:1' })]);
    expect(root.querySelector('.sign-shade')).toBeNull();
  });

  test('식별번호 미리 보기: 번호 없는 문서는 매겨질 번호를 흐리게 · ⌘G 도 그 번호로 · 번호 있는 문서는 제 번호', async () => {
    texts = ['【기술분야】', '가나다', '라마'];
    keysAnswer = [null, 'p:0001', 'p:0002'];
    const root = await open();
    await settle(20);
    expect([...root.querySelectorAll('.nk-pnum.planned')].map((mark) => mark.textContent)).toEqual(['[0001]', '[0002]']);
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'g', metaKey: true, bubbles: true }));
    const box = root.querySelector<HTMLFormElement>('.go-ask')!;
    box.querySelector<HTMLInputElement>('.go-text')!.value = '2';
    box.requestSubmit();
    expect(root.querySelector('.go-ask')).toBeNull();
    type(root, 'X');
    await settle(500);
    expect(last('editor_replace_text')).toEqual(['editor_replace_text', expect.objectContaining({ paragraphId: 'paragraph:2' })]);
  });

  test('식별번호 미리 보기: 제 번호가 있는 문서(최종본)에는 미리 본 번호를 쓰지 않는다', async () => {
    texts = ['【기술분야】', '가나다', '라마'];
    numbers = [null, '0007', null];
    keysAnswer = [null, 'p:0007', 'p:0002'];
    const root = await open();
    await settle(20);
    expect(root.querySelectorAll('.nk-pnum.planned').length).toBe(0);
    expect([...root.querySelectorAll('.nk-pnum')].map((mark) => mark.textContent)).toEqual(['[0007]']);
  });

  test('고친 곳 이동: ⌘] 다음 · ⌘[ 앞 · 끝에서 돌아간다 · 연 뒤 고친 것으로 묻는다', async () => {
    texts = ['가', '나', '다', '라'];
    const root = await open();
    const key = (k: string) => document.dispatchEvent(new KeyboardEvent('keydown', { key: k, metaKey: true, bubbles: true }));
    const status = () => root.querySelector('.editor-status')!.textContent;
    marksAnswer = { paragraphs: [], changed: [], added: [], removed: [] };
    key(']');
    await settle(10);
    expect(status()).toBe('연 뒤 고친 곳이 없습니다.');
    marksAnswer = {
      paragraphs: [
        { id: 'paragraph:2', inserted: [[0, 1]], deleted: [] },
        { id: 'paragraph:0', inserted: [[0, 1]], deleted: [] },
        { id: 'paragraph:2', inserted: [[0, 1]], deleted: [] },
      ],
      changed: [],
      added: [],
      removed: [],
    };
    key(']');
    await settle(10);
    expect(status()).toBe('고친 곳 1/2');
    expect(last('editor_amendment_marks')).toEqual(['editor_amendment_marks', { original: false, file: false }]);
    key(']');
    await settle(10);
    expect(status()).toBe('고친 곳 2/2');
    key(']');
    await settle(10);
    expect(status()).toBe('고친 곳 1/2 (처음으로 돌아왔습니다)');
    key('[');
    await settle(10);
    expect(status()).toBe('고친 곳 2/2 (끝으로 갔습니다)');
    type(root, 'X');
    await settle(500);
    expect(last('editor_replace_text')).toEqual(['editor_replace_text', expect.objectContaining({ paragraphId: 'paragraph:2' })]);
  });

  test('들여쓰기 · 들여쓰기 취소: 커서 문단에 본문 들여쓰기 · 0 (툴바 단추 · 우클릭과 같은 길)', async () => {
    const root = await open();
    clickAt(root, 63, 104);
    q(root, '[data-action="outdent"]').click();
    await settle(10);
    expect(last('editor_set_indent')).toEqual(['editor_set_indent', { paragraphId: 'paragraph:0', indent: 0 }]);
    q(root, '[data-action="indent"]').click();
    await settle(10);
    expect((last('editor_set_indent')![1] as { indent: number }).indent).toBeGreaterThan(0);
    // ⇧Tab 도 들여쓰기 취소다 — 탭 문자를 넣지 않는다.
    calls.length = 0;
    root.querySelector('.caret-input')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', shiftKey: true, bubbles: true }));
    await settle(10);
    expect(last('editor_set_indent')).toEqual(['editor_set_indent', { paragraphId: 'paragraph:0', indent: 0 }]);
    await settle(500);
    expect(glyphs(root)).not.toContain('\t');
    expect(calls.some(([c]) => c === 'editor_replace_text'), '탭 문자를 넣지 않는다').toBe(false);
  });

  const SETTINGS = {
    autosave_minutes: 5,
    history_keep: 10,
    history_days: 30,
    zoom: 1.2,
    marks_default: false,
    snippets: [{ name: '첫머리', text: '의견' }],
  };
  const keyDown = (key: string) => document.dispatchEvent(new KeyboardEvent('keydown', { key, metaKey: true, bubbles: true }));

  test('설정: 배율 · 문단부호 기본 · 자동 저장 간격을 시작할 때 걸고 · ⌘+ ⌘− ⌘0 은 배율을 저장 · 다른 탭이 바꾸면 따른다', async () => {
    settingsAnswer = SETTINGS;
    const root = await open();
    await settle(10);
    const pageWidth = () => Math.round(parseFloat(root.querySelector<HTMLElement>('.nk-page')!.style.width) * 10) / 10;
    // 배율은 문서 쪽만(툴바 · 창 전체는 그대로) — 쪽 상자 폭이 배율만큼.
    expect(pageWidth()).toBe(Math.round(595 * SCALE * 1.2 * 10) / 10);
    expect(calls.some(([c]) => c === 'editor_set_zoom'), '창 전체 확대는 쓰지 않는다').toBe(false);
    expect(q(root, '[data-action="marks"]').getAttribute('aria-pressed')).toBe('false');
    vi.useFakeTimers();
    try {
      const saves = () => calls.filter(([c]) => c === 'editor_autosave').length;
      clickAt(root, 63, 104);
      type(root, 'X');
      await vi.advanceTimersByTimeAsync(500);
      vi.advanceTimersByTime(AUTOSAVE_MS);
      expect(saves(), '기본 2분이 아니라 설정의 5분').toBe(0);
      vi.advanceTimersByTime(5 * 60_000 - AUTOSAVE_MS);
      expect(saves()).toBe(1);
    } finally {
      vi.useRealTimers();
    }
    // 앞 테스트들이 띄운 편집기도 document 키를 듣는다 — 키는 「이 값이 저장됐다」만, 걸음은 이 편집기 단추로 잰다.
    const zooms = () => calls.filter(([c]) => c === 'settings_set').map(([, a]) => (a as { settings: { zoom: number } }).settings.zoom);
    keyDown('=');
    await settle(10);
    expect(zooms()).toContain(1.3);
    expect(pageWidth()).toBe(Math.round(595 * SCALE * 1.3 * 10) / 10);
    calls.length = 0;
    q(root, '[data-action="zoom-out"]').click();
    q(root, '[data-action="zoom-out"]').click();
    await settle(10);
    expect(zooms(), '연달아 눌러도 한 걸음씩').toEqual([1.2, 1.1]);
    q(root, '[data-action="zoom-reset"]').click();
    await settle(10);
    expect(zooms().at(-1)).toBe(1);
    expect(pageWidth()).toBe(Math.round(595 * SCALE * 10) / 10);
    expect(q(root, '.zoom-level').textContent).toBe('100%');
    // 다른 탭이 문단부호 기본을 켰다.
    handlers.get('settings-changed')!({ payload: { ...SETTINGS, marks_default: true } });
    expect(q(root, '[data-action="marks"]').getAttribute('aria-pressed')).toBe('true');
    await settle(500);
  });

  test('설정 창(⌘,): 값을 고쳐 저장하면 한 번에 저장 · 잘못된 칸은 앞 값', async () => {
    settingsAnswer = SETTINGS;
    const root = await open();
    await settle(10);
    keyDown(',');
    const form = root.querySelector<HTMLFormElement>('.settings-ask')!;
    form.querySelector<HTMLInputElement>('[name="days"]')!.value = '7';
    form.querySelector<HTMLInputElement>('[name="keep"]')!.value = '';
    form.querySelector<HTMLInputElement>('[name="zoom"]')!.value = '150';
    form.querySelector<HTMLInputElement>('[name="marks"]')!.checked = true;
    form.querySelector<HTMLButtonElement>('button[type="submit"]')!.click();
    await settle(10);
    expect(root.querySelector('.settings-shade')).toBeNull();
    expect((last('settings_set')![1] as { settings: unknown }).settings).toEqual({
      ...SETTINGS,
      history_days: 7,
      history_keep: 10,
      zoom: 1.5,
      marks_default: true,
    });
  });

  test('서열목록 넣기: 단추 → 파일 고르기 · 취소면 그대로 · ST.26 이 아니면 알림', async () => {
    const root = await open();
    await settle(10);
    const button = q(root, '[data-action="sequence-list"]') as HTMLButtonElement;
    expect(button.disabled).toBe(false);
    sequenceAnswer = 'cancel';
    button.click();
    await settle(10);
    expect(calls.filter(([c]) => c === 'editor_insert_sequence_listing').length).toBe(1);
    expect(root.querySelector('.editor-status')!.textContent).not.toContain('넣었습니다');
    expect(glyphs(root)).not.toContain('서열목록');
    sequenceAnswer = { code: 'unsupported_format', phase: 'sequence_listing', details: { reason: 'sequence_not_st26' } };
    button.click();
    await settle(10);
    expect(root.querySelector('.editor-status')!.textContent).toContain('ST.26(XML) 서열목록 파일이 아닙니다');
    sequenceAnswer = 'insert';
    button.click();
    await settle(10);
    expect(glyphs(root)).toContain('【서열목록】서열목록전자파일첨부(/d/seq.xml)');
    expect(root.querySelector('.editor-status')!.textContent).toContain('서열목록을 넣었습니다');
  });

  test('상용구: 창에서 넣기 → 커서 자리에(붙여넣기와 같은 길) · 새 상용구는 설정에 · ⌘K 에 「상용구: 이름」', async () => {
    settingsAnswer = { ...SETTINGS, zoom: 1 }; // 누를 자리(clickAt)가 배율 1 기준
    const root = await open();
    await settle(10);
    q(root, '[data-action="snippets"]').click();
    await settle(10);
    root.querySelector<HTMLButtonElement>('.snippet-insert')!.click();
    await settle(10);
    expect(root.querySelector('.editor-status')!.textContent).toBe('상용구를 넣을 자리를 먼저 누르세요.');
    clickAt(root, 63, 104);
    q(root, '[data-action="snippets"]').click();
    await settle(10);
    root.querySelector<HTMLButtonElement>('.snippet-insert')!.click();
    await settle(10);
    expect(glyphs(root)).toBe('가의견나다라마');
    q(root, '[data-action="snippets"]').click();
    await settle(10);
    root.querySelector<HTMLButtonElement>('.snippet-new')!.click();
    const form = root.querySelector<HTMLFormElement>('.snippet-form')!;
    form.querySelector<HTMLInputElement>('[name="name"]')!.value = '맺음';
    form.querySelector<HTMLTextAreaElement>('[name="text"]')!.value = '끝\r\n둘째 줄';
    form.querySelector<HTMLButtonElement>('button[type="submit"]')!.click();
    await settle(10);
    expect((last('settings_set')![1] as { settings: { snippets: unknown } }).settings.snippets).toEqual([
      { name: '첫머리', text: '의견' },
      { name: '맺음', text: '끝\n둘째 줄' },
    ]);
    root.querySelector<HTMLButtonElement>('.snippet-shade .batch-close')!.click();
    keyDown('k');
    const items = [...root.querySelectorAll('.palette-list li')].map((li) => li.textContent);
    expect(items).toContain('상용구: 맺음');
    await settle(500);
  });

  test('읽기 전용으로 열기: 고칠 수 없고 까닭을 알린다 · 이전 판 목록 → 열기는 목록 차례로', async () => {
    const root = await open();
    q(root, '[data-action="open-read-only"]').click();
    await settle(10);
    expect(root.querySelector('.editor-status')!.textContent).toContain('읽기 전용(원본 보호)');
    clickAt(root, 63, 104);
    type(root, 'X');
    await settle(10);
    expect(glyphs(root)).toBe('가나다라마');
    expect(root.querySelector('.editor-status')!.textContent).toContain('읽기 전용으로 연 문서입니다');

    q(root, '[data-action="history"]').click();
    await settle(10);
    expect(root.querySelector('.editor-status')!.textContent).toContain('이전 판이 없습니다');
    historyAnswer = [
      { saved: '2026-10-24 13:00:00', size: 4096 },
      { saved: '2026-10-24 11:00:00', size: 2048 },
    ];
    q(root, '[data-action="history"]').click();
    await settle(10);
    const rows = [...root.querySelectorAll('.history-shade tbody tr')].map((tr) => tr.textContent);
    expect(rows).toEqual(['2026-10-24 13:00:004 KB열기', '2026-10-24 11:00:002 KB열기']);
    root.querySelectorAll<HTMLButtonElement>('.history-open')[1].click();
    await settle(10);
    expect(last('editor_history_open')).toEqual(['editor_history_open', { index: 1 }]);
  });

  test('저장 뒤 되읽기가 어긋나면 까닭을 사람 말로 알린다', async () => {
    const root = await open();
    saveError = { code: 'verification_failed', phase: 'save_verify', details: { reason: 'save_verify:text' } };
    q(root, '[data-action="save"]').click();
    await settle(10);
    expect(root.querySelector('.editor-error')!.textContent).toContain('저장한 파일의 글이 화면과 다릅니다');
  });

  test('⌘K 명령 찾기: 툴바 · 도구를 이름으로 거르고 ↑↓ Enter 로 실행 · Esc 로 닫기', async () => {
    texts = ['【청구범위】', '【청구항 1】', '장치.'];
    const root = await open();
    const key = (target: EventTarget, k: string, meta = false) =>
      target.dispatchEvent(new KeyboardEvent('keydown', { key: k, metaKey: meta, bubbles: true }));
    key(document, 'k', true);
    const field = root.querySelector<HTMLInputElement>('.palette-text')!;
    expect(document.activeElement).toBe(field);
    const items = () => [...root.querySelectorAll('.palette-list li')].map((li) => li.textContent);
    expect(items()).toContain('문서 통계');
    expect(items()).toContain('저장');
    field.value = '번호 다시';
    field.dispatchEvent(new Event('input'));
    // 설명(툴바 단추의 title)으로도 걸린다 — 「도구」 단추 설명에 「번호 다시 매기기」가 있다.
    // 이름에 든 것이 먼저 · 설명에만 든 것(「도구」 단추 설명의 「번호 다시 매기기」)은 뒤.
    expect(items()).toEqual(['청구항 번호 다시 매기기…', '도면 번호 다시 매기기…', '도구']);
    key(field, 'ArrowDown');
    expect(root.querySelector('.palette-list li.active')!.textContent).toBe('도면 번호 다시 매기기…');
    key(field, 'ArrowDown');
    key(field, 'ArrowDown');
    expect(root.querySelector('.palette-list li.active')!.textContent, '끝에서 처음으로').toBe('청구항 번호 다시 매기기…');
    key(field, 'Escape');
    expect(root.querySelector('.palette-shade')).toBeNull();
    key(document, 'k', true);
    const again = root.querySelector<HTMLInputElement>('.palette-text')!;
    again.value = '통계';
    again.dispatchEvent(new Event('input'));
    key(again, 'Enter');
    await settle(10);
    expect(root.querySelector('.palette-shade')).toBeNull();
    expect(root.querySelector('.stats-shade strong')!.textContent).toBe('문서 통계');
  });

  test('툴바: 묶음 이름 · 보이는 단추 아이콘이 겹치지 않는다 · 예전 data-action 은 모두 남는다', async () => {
    const root = await open();
    const groups = [...root.querySelectorAll('.editor-toolbar .tb-group')].map((group) => group.getAttribute('aria-label'));
    expect(groups).toEqual(['파일', '편집', '글자', '문단', '넣기', '보기', '도구', '보정(OA 대응)', '내보내기', '도움말']);
    const icons = [...root.querySelectorAll('.editor-toolbar .tb-group button svg')].map((svg) => svg.innerHTML);
    expect(new Set(icons).size, '같은 아이콘을 두 단추가 쓰지 않는다').toBe(icons.length);
    const kept = ['open', 'new-opinion', 'import', 'source', 'tools', 'batch', 'new-tab', 'save', 'save-as', 'close-document', 'undo', 'redo', 'find',
      'align-justified', 'align-center', 'align-left', 'indent', 'outdent', 'math', 'chem', 'table', 'marks', 'pdf', 'docx', 'fin', 'review', 'amend-marks', 'basis', 'amend'];
    for (const action of kept) expect(root.querySelector(`[data-action="${action}"]`), action).not.toBeNull();
    // 새로 만들기 ▾ = 새 의견서 · HLT 만들기 · 새 탭
    q(root, '[data-action="new-menu"]').click();
    await settle(10);
    const items = menus[menus.length - 1] as unknown as { text: string; action: () => void }[];
    expect(items.map((item) => item.text)).toEqual(['새 의견서', '워드/XML로 HLT 만들기…', '새 탭']);
  });

  test('메뉴 막대 → 화면: 툴바 단추 · 도구 · 도움말 · 꺼진 단추는 문서를 먼저', async () => {
    const root = await open();
    const send = async (payload: string) => {
      handlers.get('menu-action')!({ payload });
      await settle(10);
    };
    await send('action:find');
    expect(root.querySelector<HTMLElement>('.find-bar')!.hidden).toBe(false);
    await send('tool:문서 통계');
    expect(root.querySelector('.stats-shade strong')!.textContent).toBe('문서 통계');
    await send('help:shortcuts');
    expect(root.querySelector('.shortcut-shade strong')!.textContent).toBe('단축키 모음');
    await send('help:tours');
    expect(root.querySelector('.help-shade')).not.toBeNull();
    root.querySelector<HTMLButtonElement>('[data-action="save"]')!.disabled = true;
    await send('action:save');
    expect(root.querySelector('.editor-status')!.textContent).toBe('「저장」: 문서를 먼저 여세요.');
  });

  test('메뉴 막대(Rust)의 이름 · 할 일이 화면과 맞다', async () => {
    const source = (await import('../src-tauri/src/menu.rs?raw')).default;
    const root = await open();
    const actions = [...source.matchAll(/action\("([a-z-]+)"/g), ...source.matchAll(/"action:([a-z-]+)"/g)].map((m) => m[1]);
    expect(actions.length).toBeGreaterThan(15);
    for (const action of actions) expect(root.querySelector(`[data-action="${action}"]`), action).not.toBeNull();
    const tools = [...source.slice(source.indexOf('pub const TOOLS'), source.indexOf('];')).matchAll(/"([^"]+)"/g)].map((m) => m[1]);
    q(root, '[data-action="tools"]').click();
    await settle(10);
    const menu = (menus[menus.length - 1] as unknown as { text: string }[]).map((item) => item.text);
    expect(tools).toEqual(menu);
  });

  test('도움말: 기능 투어 목록 → 투어(비추기 · 말풍선 · 다음/이전/Esc) · 찾기 투어는 끝나면 찾기 막대를 되돌린다', async () => {
    const root = await open();
    q(root, '[data-action="help"]').click();
    await settle(10);
    const list = [...root.querySelectorAll<HTMLButtonElement>('.help-tour')];
    expect(list.length).toBe(21);
    expect(list[0].querySelector('strong')!.textContent).toBe('전체 둘러보기');
    expect([...root.querySelectorAll('.help-ask h3')].map((h) => h.textContent)).toEqual(['처음이라면', '파일', '편집', '보기', '도구', '점검', '보정(OA 대응)']);
    expect(root.querySelector('.tour-invite'), '도움말을 열면 처음 실행 권유는 닫힌다').toBeNull();
    list.find((button) => button.dataset.tour === 'find')!.click();
    expect(root.querySelector('.help-shade')).toBeNull();
    const bar = root.querySelector<HTMLElement>('.find-bar')!;
    expect(bar.hidden, '찾기 투어가 막대를 연다').toBe(false);
    const title = () => root.querySelector('.tour-title')!.textContent;
    const count = () => root.querySelector('.tour-count')!.textContent;
    expect([title(), count()]).toEqual(['찾을 글', '찾기 옵션(단어 · 정규식) · 1/4']);
    q(root, '.tour-next').click();
    expect(title()).toBe('단어');
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }));
    expect(title()).toBe('정규식');
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowLeft', bubbles: true }));
    expect(title()).toBe('단어');
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    expect(root.querySelector('.tour')).toBeNull();
    expect(bar.hidden, '열어 둔 찾기 막대를 되돌린다').toBe(true);
  });

  test('투어: 모든 단계의 대상이 화면에 있다(화면을 바꾸면 투어가 깨지지 않게)', async () => {
    const { TOURS } = await import('./tour');
    const root = await open();
    for (const tour of TOURS) {
      for (const step of tour.steps) {
        expect(root.querySelector(step.target), `${tour.title} · ${step.title} · ${step.target}`).not.toBeNull();
      }
    }
  });

  test('처음 실행: 투어 권유 띠 · 닫으면 다음엔 없다', async () => {
    localStorage.clear();
    let root = await open();
    expect(root.querySelector('.tour-invite')).not.toBeNull();
    q(root, '.tour-invite-close').click();
    expect(root.querySelector('.tour-invite')).toBeNull();
    root = await open();
    expect(root.querySelector('.tour-invite')).toBeNull();
    localStorage.clear();
    root = await open();
    q(root, '.tour-invite-go').click();
    expect(root.querySelector('.tour-count')!.textContent).toBe('전체 둘러보기 · 1/10');
    q(root, '.tour-end').click(); // 투어 키 듣기를 풀어 다음 테스트에 새지 않게
  });

  test('find range: 상세설명만 · 청구항만 (replace-all too) · 고른 곳만 frozen at pick time · kept when typing anew', async () => {
    texts = ['【발명의 설명】', '가나가나가', '【청구범위】', '【청구항 1】', '가나', '【요약서】', '가'];
    const root = await open();
    q(root, '[data-action="find"]').click();
    const field = root.querySelector<HTMLInputElement>('.find-text')!;
    const range = root.querySelector<HTMLSelectElement>('.find-range')!;
    const label = root.querySelector('.find-count')!;
    const pick = (value: string) => {
      range.value = value;
      range.dispatchEvent(new Event('change'));
    };
    const next = async () => {
      q(root, '[data-action="find-next"]').click();
      await settle(10);
    };
    field.value = '가';
    await next();
    expect(label.textContent).toMatch(/^1 \/ 5/);
    pick('claims');
    await next();
    expect(label.textContent).toMatch(/^청구항 · 1 \/ 1/);
    q(root, '[data-action="replace-all"]').click();
    await settle(10);
    const replaced = last('editor_replace')![1] as { matches: { paragraph_id: string }[] };
    expect(replaced.matches.map((m) => m.paragraph_id)).toEqual(['paragraph:4']);
    expect(label.textContent).toBe('청구항 · 1곳 바꿈');
    pick('description');
    await next();
    expect(label.textContent).toMatch(/^상세설명 · \d \/ 3/);
    // 새로 쳐도 고른 범위는 남는다(보정안 번호 범위만 풀린다).
    field.value = '가';
    field.dispatchEvent(new Event('input'));
    await next();
    expect(range.value).toBe('description');
    expect(label.textContent).toMatch(/\/ 3/);
    // 고른 곳만: 둘째 문단 1..3(「나가」)을 고르고 고른다 → 그 안의 「가」 하나 · 다음 찾기로 선택이 옮겨 가도 그대로.
    clickAt(root, 63, 124);
    root.querySelector('.nk-page')!.dispatchEvent(
      new MouseEvent('mousedown', { bubbles: true, button: 0, shiftKey: true, clientX: 87 * SCALE, clientY: 124 * SCALE }),
    );
    pick('span');
    await next();
    expect(label.textContent).toMatch(/^고른 곳 · 1 \/ 1/);
    await next();
    expect(label.textContent).toMatch(/^고른 곳 · 1 \/ 1 · …가나가나가…/);
    // 범위를 고른 뒤 쪽에서 다른 곳(청구항 「가나」)을 골라도 모두 바꾸기는 얼린 범위 안만.
    clickAt(root, 51, 184);
    root.querySelector('.nk-page')!.dispatchEvent(
      new MouseEvent('mousedown', { bubbles: true, button: 0, shiftKey: true, clientX: 75 * SCALE, clientY: 184 * SCALE }),
    );
    q(root, '[data-action="replace-all"]').click();
    await settle(10);
    expect(last('editor_replace')![1]).toEqual(
      expect.objectContaining({ matches: [expect.objectContaining({ paragraph_id: 'paragraph:1', start: 2, end: 3 })] }),
    );
    await settle(500);
  });

  test('find range: 고른 곳만 without a selection → hint and back to 전체 · a missing section says so', async () => {
    texts = ['【의견내용】', '가나'];
    const root = await open();
    q(root, '[data-action="find"]').click();
    const range = root.querySelector<HTMLSelectElement>('.find-range')!;
    const label = root.querySelector('.find-count')!;
    range.value = 'span';
    range.dispatchEvent(new Event('change'));
    expect(range.value).toBe('');
    expect(label.textContent).toContain('범위를 먼저 고르세요');
    range.value = 'claims';
    range.dispatchEvent(new Event('change'));
    root.querySelector<HTMLInputElement>('.find-text')!.value = '가';
    q(root, '[data-action="find-next"]').click();
    await settle(10);
    expect(label.textContent).toBe('이 문서에는 청구항 절이 없습니다');
  });

  test('제출 전 점검: issues → window (가기 folds it and goes · 멈추기 stops) · 그래도 만들기 makes · none → straight to the file', async () => {
    const root = await open();
    const make = async () => {
      q(root, '[data-action="fin"]').click();
      await settle(10);
    };
    await make(); // 걸린 것이 없다 → 창 없이 바로
    expect(root.querySelector('.submit-ask')).toBeNull();
    expect(calls.filter(([c]) => c === 'editor_export_fin').length).toBe(1);
    submitAnswer = [
      { kind: 'figure_missing', paragraph_id: 'paragraph:1', detail: '「도 5」을 말하는데 【도 5】 도면이 없습니다' },
      { kind: 'unnumbered', paragraph_id: null, detail: '식별번호가 하나도 없습니다 — 「문단번호 넣기」를 누르세요' },
      { kind: 'sign_names', paragraph_id: 'paragraph:0', detail: '부호 10: 몸체(3) · 본체(1)' },
      { kind: 'sign_shared', paragraph_id: 'paragraph:0', detail: '「몸체」: 부호 10(4) · 20(1)' },
    ];
    await make();
    let box = root.querySelector<HTMLFormElement>('.submit-ask')!;
    expect(box.textContent).toContain('제출 전 점검 — 4건');
    expect([...box.querySelectorAll('legend')].map((l) => l.textContent)).toEqual([
      '빈 식별번호',
      '도면이 없는 「도 N」',
      '같은 부호 · 다른 이름(괄호 안 = 쓰인 수)',
      '같은 이름 · 다른 부호(한두 번만 다른 부호)',
    ]);
    expect(box.querySelectorAll('.submit-go').length).toBe(3); // 문서 전체의 일은 「가기」가 없다
    box.querySelector<HTMLButtonElement>('.submit-go')!.click();
    expect(box.hidden).toBe(true);
    type(root, 'X');
    await settle(500);
    expect(last('editor_replace_text')).toEqual(['editor_replace_text', expect.objectContaining({ paragraphId: 'paragraph:1', text: 'X라마' })]);
    q(root, '.review-reopen').click();
    expect(box.hidden).toBe(false);
    q(box, '.item-cancel').click();
    await settle(10);
    expect(root.querySelector('.submit-ask')).toBeNull();
    expect(root.querySelector('.review-reopen')).toBeNull();
    expect(calls.filter(([c]) => c === 'editor_export_fin').length).toBe(1);
    expect(root.querySelector('.editor-status')!.textContent).toContain('멈췄습니다');
    await make();
    box = root.querySelector<HTMLFormElement>('.submit-ask')!;
    box.requestSubmit();
    await settle(10);
    expect(calls.filter(([c]) => c === 'editor_export_fin').length).toBe(2);
    // 대표도 없음은 창을 띄우지 않고 만든 뒤 알림만.
    submitAnswer = [{ kind: 'representative_missing', paragraph_id: null, detail: '【대표도】가 없다' }];
    await make();
    expect(root.querySelector('.submit-ask')).toBeNull();
    expect(calls.filter(([c]) => c === 'editor_export_fin').length).toBe(3);
    expect(root.querySelector('.editor-status')!.textContent).toContain('알림: 【대표도】가 없습니다');
  });

  test('제출 전 점검: 띄어쓰기 · 「한 칸으로 모두 고치기」 fixes once and drops only the double-space rows', async () => {
    const root = await open();
    submitAnswer = [
      { kind: 'spacing', paragraph_id: 'paragraph:0', detail: '공백 두 칸: 「있다.␣␣다음」' },
      { kind: 'spacing', paragraph_id: 'paragraph:1', detail: '전각 공백: 「가□나」' },
    ];
    q(root, '[data-action="fin"]').click();
    await settle(10);
    const box = root.querySelector<HTMLFormElement>('.submit-ask')!;
    expect(box.querySelector('legend')!.textContent).toContain('띄어쓰기');
    submitAnswer = [submitAnswer[1]]; // 고친 뒤 다시 점검한 답
    q(box, '.submit-fix-spaces').click();
    await settle(10);
    expect(calls.filter(([c]) => c === 'editor_fix_double_spaces').length).toBe(1);
    expect([...box.querySelectorAll('li')].map((li) => li.textContent)).toEqual(['가기 전각 공백: 「가□나」']);
    expect(box.querySelector('.submit-fix-spaces')).toBeNull();
    expect(box.textContent).toContain('한 칸으로 고쳤습니다');
    q(box, '.item-cancel').click();
    await settle(10);
    // 표 칸은 고치지 않는다 — 다시 점검해도 남은 줄은 두고 알린다.
    const cell = { kind: 'spacing', paragraph_id: 'paragraph:2', detail: '공백 두 칸: 「칸␣␣글」' };
    submitAnswer = [cell];
    q(root, '[data-action="fin"]').click();
    await settle(10);
    const again = root.querySelector<HTMLFormElement>('.submit-ask')!;
    q(again, '.submit-fix-spaces').click();
    await settle(10);
    expect(again.querySelectorAll('li').length).toBe(1);
    expect(again.textContent).toContain('표 칸 1곳은 직접 고치세요');
    q(again, '.item-cancel').click();
    await settle(10);
    // 공백 두 칸이 없으면 단추도 없다.
    submitAnswer = [{ kind: 'spacing', paragraph_id: 'paragraph:1', detail: '전각 공백: 「가□나」' }];
    q(root, '[data-action="fin"]').click();
    await settle(10);
    expect(root.querySelector('.submit-ask .submit-fix-spaces')).toBeNull();
  });

  test('amendment base: 연 뒤 고친 것 by default · 원출원 re-asks with original (marks and basis) · messages name the base', async () => {
    const root = await open();
    marksAnswer = { paragraphs: [], changed: [], added: [], removed: [] };
    q(root, '[data-action="amend-marks"]').click();
    await settle(10);
    expect(last('editor_amendment_marks')).toEqual(['editor_amendment_marks', { original: false, file: false }]);
    expect(root.querySelector('.editor-status')!.textContent).toContain('연 뒤 고친 것이 없습니다');
    marksAnswer = { paragraphs: [{ id: 'paragraph:0', inserted: [[0, 1]], deleted: [] }], changed: ['【0001】'], added: [], removed: [] };
    const base = root.querySelector<HTMLSelectElement>('.amend-base')!;
    base.value = 'original';
    base.dispatchEvent(new Event('change'));
    await settle(10);
    expect(last('editor_amendment_marks')).toEqual(['editor_amendment_marks', { original: true, file: false }]);
    expect(root.querySelector('.editor-status')!.textContent).toBe('보정 표시(원출원) — 바뀐 것: 【0001】');
    q(root, '[data-action="basis"]').click();
    await settle(10);
    expect(last('editor_amendment_basis')).toEqual(['editor_amendment_basis', { original: true }]);
  });

  test('다른 파일: 고르면 보정 표시를 켜고 그 파일에 견준다 · 취소하면 앞 기준 · 상태줄에 파일 이름', async () => {
    const root = await open();
    marksAnswer = { paragraphs: [], changed: ['【0002】'], added: [], removed: [] };
    const base = root.querySelector<HTMLSelectElement>('.amend-base')!;
    const pick = async (value: string) => {
      base.value = value;
      base.dispatchEvent(new Event('change'));
      await settle(10);
    };
    await pick('file'); // 취소
    expect(base.value).toBe('');
    expect(calls.some(([c]) => c === 'editor_amendment_marks')).toBe(false);
    compareAnswer = '1차본.HLT';
    await pick('file');
    expect(q(root, '[data-action="amend-marks"]').getAttribute('aria-pressed')).toBe('true');
    expect(last('editor_amendment_marks')).toEqual(['editor_amendment_marks', { original: false, file: true }]);
    expect(root.querySelector('.editor-status')!.textContent).toBe('보정 표시(파일: 1차본.HLT) — 바뀐 것: 【0002】');
    await pick('original');
    compareAnswer = null;
    await pick('file'); // 취소 → 원출원으로
    expect(base.value).toBe('original');
  });

  test('보정 근거: 위 3개 후보만 보이고 나머지는 「더 보기」(체크는 그대로 · 접기)', async () => {
    const root = await open();
    const hit = (number: string, score: number) => ({ number, score, exact: false, quote: `${number} 문장`, figures: [] });
    basisAnswer = [{ claim: 1, added: false, phrases: [{ text: '새 말', hits: ['0001', '0002', '0003', '0004', '0005'].map((n) => hit(n, 0.9)) }] }];
    q(root, '[data-action="basis"]').click();
    await settle(10);
    const more = root.querySelector<HTMLElement>('.basis-more')!;
    const toggle = q(root, '.basis-more-toggle');
    expect(more.hidden).toBe(true);
    expect(more.querySelectorAll('input[type="checkbox"]').length).toBe(2);
    expect(toggle.textContent).toBe('후보 2개 더 보기');
    toggle.click();
    expect(more.hidden).toBe(false);
    expect(toggle.textContent).toBe('후보 접기');
    // 더 본 후보를 고르면 초안에 들어간다.
    const fifth = root.querySelector<HTMLInputElement>('input[data-h="4"]')!;
    fifth.checked = true;
    fifth.dispatchEvent(new Event('change', { bubbles: true }));
    expect(root.querySelector<HTMLTextAreaElement>('.basis-draft')!.value).toContain('【0005】');
    toggle.click();
    expect(more.hidden).toBe(true);
  });

  test('amendment marks: 보정 표시 draws inserted text, deletion wedges and change bars (only for a final .fin)', async () => {
    const root = await open();
    const toggle = root.querySelector<HTMLButtonElement>('[data-action="amend-marks"]')!;
    toggle.click(); // 최종본이 아니다(null) → 꺼지고 알린다
    await settle(10);
    expect(toggle.getAttribute('aria-pressed')).toBe('false');
    expect(root.querySelector('.editor-status')!.textContent).toContain('최종본(.fin)');
    marksAnswer = {
      paragraphs: [{ id: 'paragraph:0', inserted: [[1, 3]], deleted: [[0, '지운']] }],
      changed: ['【0001】'],
      added: [],
      removed: ['청구항 2'],
    };
    issuesAnswer = [{ claim: 3, kind: 'removed', cited: [2] }];
    toggle.click();
    await settle(10);
    expect(toggle.getAttribute('aria-pressed')).toBe('true');
    const ins = root.querySelector<HTMLElement>('.nk-ins')!;
    expect(ins.style.left).toBe(`${62 * SCALE}px`); // 「나다」: 글자 1 경계 x = 50 + 12
    expect(ins.style.width).toBe(`${24 * SCALE}px`);
    expect(root.querySelector<HTMLElement>('.nk-del')!.title).toBe('지움: 지운');
    expect(root.querySelectorAll('.nk-chg').length).toBe(1); // 바뀐 문단 줄에만
    const status = root.querySelector('.editor-status')!.textContent!;
    expect(status).toContain('바뀐 것: 【0001】');
    expect(status).toContain('지운 것: 청구항 2');
    expect(status).toContain('청구항 인용 문제 1건');
    toggle.click();
    await settle(10);
    expect(root.querySelector('.nk-ins')).toBeNull();
  });

  test('보정 근거: candidates per new phrase (best ones checked) → draft follows the checks → copy · new opinion', async () => {
    const root = await open();
    root.querySelector<HTMLButtonElement>('[data-action="basis"]')!.click();
    await settle(10);
    expect(root.querySelector('.editor-status')!.textContent).toContain('최종본(.fin)');
    basisAnswer = [
      {
        claim: 1,
        added: false,
        phrases: [
          {
            text: '고무 바퀴',
            hits: [
              { number: '0002', score: 1, exact: true, quote: '고무 바퀴가 있다.', figures: ['3'] },
              { number: '0003', score: 0.7, exact: false, quote: '바퀴는 고무다.', figures: [] },
            ],
          },
        ],
      },
    ];
    root.querySelector<HTMLButtonElement>('[data-action="basis"]')!.click();
    await settle(10);
    const box = root.querySelector<HTMLFormElement>('.basis-ask')!;
    const checks = [...box.querySelectorAll<HTMLInputElement>('input[type="checkbox"]')];
    expect(checks.map((c) => c.checked)).toEqual([true, false]); // 통째로 든 1위만
    const draft = box.querySelector<HTMLTextAreaElement>('.basis-draft')!;
    expect(draft.value).toContain('단락 【0002】 및 도 3');
    checks[1].checked = true;
    checks[1].dispatchEvent(new Event('change', { bubbles: true }));
    expect(draft.value).toContain('단락 【0002】~【0003】 및 도 3');
    const written: string[] = [];
    Object.defineProperty(navigator, 'clipboard', { value: { writeText: async (text: string) => void written.push(text) }, configurable: true });
    box.querySelector<HTMLButtonElement>('.basis-copy')!.click();
    await settle(10);
    expect(written[0]).toBe(draft.value);
    expect(root.querySelector('.basis-ask')).toBeNull();
    root.querySelector<HTMLButtonElement>('[data-action="basis"]')!.click();
    await settle(10);
    const again = root.querySelector<HTMLFormElement>('.basis-ask')!;
    const text = again.querySelector<HTMLTextAreaElement>('.basis-draft')!.value;
    again.querySelector<HTMLButtonElement>('.basis-opinion')!.click();
    await settle(10);
    expect(last('editor_new_opinion')).toEqual(['editor_new_opinion', { body: text }]);
  });

  test('보정서: citation problems are listed first — 취소 stops, 그래도 만들기 goes on', async () => {
    texts = ['가나다', '【청구항 3】', '제2항에 있어서'];
    issuesAnswer = [
      { claim: 3, kind: 'removed', cited: [2] },
      { claim: 3, kind: 'and_citation', cited: [1, 2] },
    ];
    const root = await open();
    root.querySelector<HTMLButtonElement>('[data-action="amend"]')!.click();
    await settle(10);
    const box = root.querySelector<HTMLFormElement>('.citation-ask')!;
    expect(box.textContent).toContain('지운 청구항 2');
    expect(box.textContent).toContain('특허로 점검 페이지는 못 잡는 것');
    box.querySelector<HTMLButtonElement>('.item-cancel')!.click();
    await settle(10);
    expect(calls.some(([command]) => command === 'editor_export_amendment')).toBe(false);
    root.querySelector<HTMLButtonElement>('[data-action="amend"]')!.click();
    await settle(10);
    root.querySelector<HTMLFormElement>('.citation-ask')!.requestSubmit();
    await settle(10);
    expect(calls.some(([command]) => command === 'editor_export_amendment')).toBe(true);
    // 다 만들면 상태줄에 대비표 자리도.
    issuesAnswer = [];
    amendAnswer = { dta: '/d/보정.dta', fin: '/d/x.fin', order: 1, items: [], table: '/d/보정_대비표.docx' };
    root.querySelector<HTMLButtonElement>('[data-action="amend"]')!.click();
    await settle(10);
    expect(root.querySelector('.editor-status')!.textContent).toContain('대비표 /d/보정_대비표.docx');
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

  test('paragraph marks: on by default — ↵ at every paragraph end, gone when off, back when on', async () => {
    texts = ['가나다', '', '라마'];
    const root = await open();
    const marks = () => root.querySelectorAll('.nk-pmark').length;
    const pressed = () => root.querySelector('[data-action="marks"]')!.getAttribute('aria-pressed');
    expect(marks()).toBe(3); // 빈 문단도
    expect(pressed()).toBe('true');
    root.querySelector<HTMLButtonElement>('[data-action="marks"]')!.click();
    await settle(10);
    expect(marks()).toBe(0);
    expect(pressed()).toBe('false');
    root.querySelector<HTMLButtonElement>('[data-action="marks"]')!.click();
    await settle(10);
    expect(marks()).toBe(3);
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

describe('basisDraft', () => {
  test('writes the opinion paragraph: ranges, figures, quotes, the particle after a claim number, and the closing line', () => {
    const hit = (number: string, figures: string[] = []) => ({ number, score: 1, exact: true, quote: `${number} 문장.`, figures });
    const text = basisDraft([
      {
        claim: 1,
        added: false,
        phrases: [
          { text: 'A', hits: [hit('0070', ['9']), hit('0071', ['10']), hit('0072')] },
          { text: 'B', hits: [hit('0015', ['3a']), hit('0070')] },
        ],
      },
      { claim: 4, added: true, phrases: [{ text: 'C', hits: [hit('0020')] }] },
      { claim: 5, added: true, phrases: [{ text: 'D', hits: [] }] },
    ]);
    expect(text.split('\n')).toEqual([
      '청구항 1의 보정 사항(“A”, “B”)은 원출원 명세서의 단락 【0015】, 【0070】~【0072】 및 도 3a, 9~10에 기재된 내용에 근거한 것입니다.',
      '- 【0070】: "0070 문장."',
      '- 【0071】: "0071 문장."',
      '- 【0072】: "0072 문장."',
      '- 【0015】: "0015 문장."',
      '',
      '새로 추가한 청구항 4는 원출원 명세서의 단락 【0020】에 기재된 내용에 근거한 것입니다.',
      '- 【0020】: "0020 문장."',
      '',
      '새로 추가한 청구항 5는 (근거 단락을 고르지 않았습니다 — 직접 쓰세요.)', // 5 = 오 → 는
      '따라서 본 보정은 신규사항 추가에 해당하지 않습니다.',
    ]);
  });
});

describe('goTarget', () => {
  const p = (id: string, text: string, wipo_id: number | null = null, number: string | null = null) => ({ id, text, wipo_id, number });
  test('identification numbers: final numbers first, else wipo_id of a body paragraph (a heading keeps stale ids)', () => {
    const fin = [p('a', '【0049】'), p('b', '본문', 49, '0049'), p('c', '본문', null, '0050')];
    for (const query of ['49', '0049', '【0049】', '[0049]', ' 0049 ']) expect(goTarget(query, fin)).toBe('b');
    expect(goTarget('50', fin)).toBe('c');
    const hlt = [p('h', '【발명의 설명】', 7), p('x', '본문', 7)];
    expect(goTarget('7', hlt)).toBe('x');
    expect(goTarget('8', hlt)).toBeNull();
  });
  test('claims · figures · headings', () => {
    const doc = [p('t', '【기술분야】'), p('c7', '【청구항 7】'), p('c17', '【청구항 17】'), p('f', '【도 3a】')];
    for (const query of ['청구항 7', '청구항7', '제7항', '제 7 항', '7항']) expect(goTarget(query, doc)).toBe('c7');
    expect(goTarget('17항', doc)).toBe('c17');
    expect(goTarget('도 3a', doc)).toBe('f');
    expect(goTarget('도3a', doc)).toBe('f');
    expect(goTarget('기술분야', doc)).toBe('t');
    expect(goTarget('【기술분야】', doc)).toBe('t');
    expect(goTarget('청구항 8', doc)).toBeNull();
    expect(goTarget('', doc)).toBeNull();
  });
});

describe('sectionOf', () => {
  test('big headings switch the section; before the first one is other', () => {
    const ps = ['【발명의 명칭】', '【발명의 설명】', '본문', '【특허청구범위】', '【청구항 1】', '가', '【요약서】', '나', '【청구범위】', '다', '【도면】', '라'].map(
      (text, i) => ({ id: `p${i}`, text }),
    );
    expect([...sectionOf(ps).values()]).toEqual([
      'other', 'description', 'description', 'claims', 'claims', 'claims', 'other', 'other', 'claims', 'claims', 'other', 'other',
    ]);
  });
});
