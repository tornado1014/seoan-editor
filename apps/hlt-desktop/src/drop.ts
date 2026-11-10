// 놓은 파일 → 확장자 · 놓은 자리로 기존 길(열기 · 불러오기 · 원문)에 보낸다. 이벤트 원천은 인자로 갈아 끼울 수 있다(테스트).
import { getCurrentWebview } from '@tauri-apps/api/webview';

export type DropRoute = 'open' | 'import' | 'source' | 'unknown';

const OPEN = ['hlt', 'fin', 'hlz'];
const IMPORT = ['docx', 'xml', 'hml'];
const SOURCE = ['docx', 'pdf', 'xlf', 'xliff', 'txlf', 'mxliff'];

export function routeDrop(path: string, overSource: boolean): DropRoute {
  const name = path.split(/[\\/]/).pop() ?? '';
  const dot = name.lastIndexOf('.');
  const ext = dot > 0 ? name.slice(dot + 1).toLowerCase() : '';
  if (OPEN.includes(ext)) return 'open';
  if (overSource && SOURCE.includes(ext)) return 'source';
  if (IMPORT.includes(ext)) return 'import';
  return 'unknown';
}

export type DragEvent =
  | { type: 'enter' | 'over'; position: { x: number; y: number } }
  | { type: 'drop'; paths: string[]; position: { x: number; y: number } }
  | { type: 'leave' };
export type DropSource = (handler: (event: DragEvent) => void) => Promise<unknown> | unknown;

const tauriSource: DropSource = (handler) =>
  getCurrentWebview().onDragDropEvent((event) => handler(event.payload as DragEvent));

export function installDrop(
  root: HTMLElement,
  sourcePane: HTMLElement,
  run: (route: DropRoute, path: string) => Promise<void>,
  source: DropSource = tauriSource,
): void {
  // 물리 좌표 → 화면 좌표.
  const overSource = (position: { x: number; y: number }) => {
    if (sourcePane.hidden) return false;
    const scale = window.devicePixelRatio || 1;
    const rect = sourcePane.getBoundingClientRect();
    const x = position.x / scale;
    const y = position.y / scale;
    return x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom;
  };
  let queue: Promise<void> = Promise.resolve();
  try {
    void Promise.resolve(
      source((event) => {
        if (event.type === 'leave') {
          root.classList.remove('drop-target', 'drop-source');
          return;
        }
        const over = overSource(event.position);
        if (event.type !== 'drop') {
          root.classList.add('drop-target');
          sourcePane.classList.toggle('drop-source', over);
          return;
        }
        root.classList.remove('drop-target');
        sourcePane.classList.remove('drop-source');
        // 여럿은 앞 것이 끝난 뒤 다음(탭 · 대화상자가 겹치지 않게).
        for (const path of event.paths) {
          const route = routeDrop(path, over);
          queue = queue.then(() => run(route, path)).catch(() => {});
        }
      }),
    ).catch(() => {});
  } catch {
    // 웹뷰 밖(시험 · 브라우저)에서는 놓기를 걸지 않는다.
  }
}
