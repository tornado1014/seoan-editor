// @vitest-environment jsdom
import { describe, expect, test, vi } from 'vitest';
import { installDrop, routeDrop, type DragEvent, type DropRoute } from './drop';

vi.mock('@tauri-apps/api/webview', () => ({ getCurrentWebview: () => ({}) }));

function harness() {
  const root = document.createElement('div');
  const pane = document.createElement('div');
  pane.hidden = false;
  pane.getBoundingClientRect = () => ({ left: 100, right: 200, top: 0, bottom: 100 }) as DOMRect;
  let emit: (event: DragEvent) => void = () => {};
  const log: string[] = [];
  let busy = false;
  const run = vi.fn(async (route: DropRoute, path: string) => {
    expect(busy).toBe(false); // 겹쳐 불리면 안 된다
    busy = true;
    log.push(`start ${route} ${path}`);
    await new Promise((resolve) => setTimeout(resolve, 5));
    log.push(`end ${path}`);
    busy = false;
  });
  installDrop(root, pane, run, (handler) => void (emit = handler));
  return { root, pane, run, log, emit: (event: DragEvent) => emit(event) };
}
const at = (x: number, y = 10) => ({ x, y });
const settle = () => new Promise((resolve) => setTimeout(resolve, 60));

describe('놓은 파일 길 가르기', () => {
  test('확장자와 놓은 자리로 길을 가른다', () => {
    expect(routeDrop('/a/명세서.HLT', false)).toBe('open');
    expect(routeDrop('/a/x.fin', false)).toBe('open');
    expect(routeDrop('/a/x.hlz', true)).toBe('open'); // 원문 창 위여도 서안 문서는 연다
    expect(routeDrop('/a/x.docx', false)).toBe('import');
    expect(routeDrop('/a/x.XML', false)).toBe('import');
    expect(routeDrop('/a/x.hml', false)).toBe('import');
    expect(routeDrop('/a/x.docx', true)).toBe('source');
    for (const ext of ['pdf', 'xlf', 'xliff', 'txlf', 'mxliff']) expect(routeDrop(`/a/x.${ext}`, true)).toBe('source');
    expect(routeDrop('/a/x.pdf', false)).toBe('unknown'); // 원문 창 밖의 pdf 는 열 것이 아니다
    expect(routeDrop('/a/폴더', false)).toBe('unknown');
  });

  test('여럿을 놓으면 하나씩 차례로 — 앞 것이 끝난 뒤 다음', async () => {
    const h = harness();
    h.emit({ type: 'drop', paths: ['/a/1.HLT', '/a/2.docx', '/a/3.fin'], position: at(10) });
    await settle();
    expect(h.log).toEqual([
      'start open /a/1.HLT', 'end /a/1.HLT',
      'start import /a/2.docx', 'end /a/2.docx',
      'start open /a/3.fin', 'end /a/3.fin',
    ]);
  });

  test('모르는 파일은 run 을 부르되 route 가 unknown', async () => {
    const h = harness();
    h.emit({ type: 'drop', paths: ['/a/메모.txt'], position: at(10) });
    await settle();
    expect(h.run).toHaveBeenCalledWith('unknown', '/a/메모.txt');
  });

  test('원문 창 위에 놓은 docx 는 source · 밖이면 import · 숨겨져 있으면 import', async () => {
    const h = harness();
    h.emit({ type: 'drop', paths: ['/a/x.docx'], position: at(150) });
    h.emit({ type: 'drop', paths: ['/a/y.docx'], position: at(10) });
    h.pane.hidden = true;
    h.emit({ type: 'drop', paths: ['/a/z.docx'], position: at(150) });
    await settle();
    expect(h.run.mock.calls.map((call) => call[0])).toEqual(['source', 'import', 'import']);
  });

  test('끌어오는 동안 테두리가 켜지고 떠나면 꺼진다', () => {
    const h = harness();
    h.emit({ type: 'over', position: at(150) });
    expect(h.root.classList.contains('drop-target')).toBe(true);
    expect(h.pane.classList.contains('drop-source')).toBe(true);
    h.emit({ type: 'leave' });
    expect(h.root.classList.contains('drop-target')).toBe(false);
  });
});
