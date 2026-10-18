// 편집 화면(P5): 도구 막대 · 구조 창 · 쪽 화면 · 문단 고치기 창.
// 쪽은 보일 때 한 쪽씩 받아 그린다. 고치면 문서 전체가 다시 구워지고 보이는 쪽을 다시 받는다.
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { Menu } from '@tauri-apps/api/menu';
import {
  editorImage,
  editorInsertFigure,
  editorLog,
  editorMergeParagraph,
  editorCellText,
  editorCellPosition,
  editorDeleteTable,
  editorInsertTable,
  editorTableEdit,
  type TableOperationDto,
  type CellPositionDto,
  editorExportAmendment,
  editorExportFin,
  editorFind,
  editorReplace,
  type FindFormatDto,
  type FindMatchDto,
  type ReplaceFormatDto,
  editorReplaceCellText,
  editorToggleFormat,
  editorToggleFormatSpans,
  editorDeleteRange,
  editorSetSize,
  type CharacterFormatDto,
  editorOpen,
  editorImport,
  editorNewOpinion,
  editorNewTab,
  editorInsertItem,
  editorRemoveItems,
  editorClose,
  editorOpenPending,
  editorPage,
  editorPrintPdf,
  editorRedo,
  editorReplaceText,
  editorReplaceTextPageBreak,
  editorSetAlignment,
  editorSave,
  editorSaveAs,
  editorCloseDocument,
  editorUndo,
  editorView,
  type Alignment,
  type EditorParagraphDto,
  type EditorViewDto,
  type PlannedPageDto,
  type AmendmentDoneDto,
} from './contracts';
import { outlineTree, renderPage, richParagraph, selectOffsets, selectionOffsets } from './editor-view';
import type { OutlineNode } from './editor-view';
import {
  type Caret,
  caretLines,
  caretPoint,
  hit,
  lineEdge,
  previewDelete,
  previewInsert,
  selectionBoxes,
  vertical,
} from './page-caret';
import {
  type Figure,
  openStructureDrawer,
  rasterize,
  renderChemistry,
  renderLatex,
} from './figure-editor';

const SCALE = 1.3;

/** 도구 막대 아이콘(Lucide 모양 · MIT). 선 굵기 2 · 24 격자 · 글자색을 따른다. */
const ICONS: Record<string, string[]> = {
  open: ['m6 14 1.5-2.9A2 2 0 0 1 9.24 10H20a2 2 0 0 1 1.94 2.5l-1.54 6a2 2 0 0 1-1.95 1.5H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H18a2 2 0 0 1 2 2v2'],
  opinion: ['M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z', 'M13 8H7', 'M17 12H7'],
  import: ['M4 22h14a2 2 0 0 0 2-2V7l-5-5H6a2 2 0 0 0-2 2v4', 'M14 2v4a2 2 0 0 0 2 2h4', 'M2 15h10', 'm9 18 3-3-3-3'],
  tab: ['M5 12h14', 'M12 5v14'],
  saveAs: ['M13 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h10.2a2 2 0 0 1 1.4.6l3.8 3.8a2 2 0 0 1 .6 1.4V11', 'M7 3v4a1 1 0 0 0 1 1h7', 'M18 15v6', 'M15 18h6'],
  close: ['M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z', 'M14 2v4a2 2 0 0 0 2 2h4', 'm14.5 12.5-5 5', 'm9.5 12.5 5 5'],
  save: ['M15.2 3a2 2 0 0 1 1.4.6l3.8 3.8a2 2 0 0 1 .6 1.4V19a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z', 'M17 21v-7a1 1 0 0 0-1-1H8a1 1 0 0 0-1 1v7', 'M7 3v4a1 1 0 0 0 1 1h7'],
  undo: ['M9 14 4 9l5-5', 'M4 9h10.5a5.5 5.5 0 0 1 5.5 5.5a5.5 5.5 0 0 1-5.5 5.5H11'],
  redo: ['m15 14 5-5-5-5', 'M20 9H9.5A5.5 5.5 0 0 0 4 14.5A5.5 5.5 0 0 0 9.5 20H13'],
  find: ['M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16z', 'm21 21-4.3-4.3'],
  justify: ['M3 6h18', 'M3 12h18', 'M3 18h18'],
  center: ['M21 6H3', 'M17 12H7', 'M19 18H5'],
  left: ['M21 6H3', 'M15 12H3', 'M17 18H3'],
  math: ['M18 7V4H6l6 8-6 8h12v-3'],
  chem: ['M10 2v7.527a2 2 0 0 1-.211.896L4.72 20.55a1 1 0 0 0 .9 1.45h12.76a1 1 0 0 0 .9-1.45l-5.069-10.127A2 2 0 0 1 14 9.527V2', 'M8.5 2h7', 'M7 16h10'],
  table: ['M3 3h18v18H3z', 'M3 9h18', 'M3 15h18', 'M9 3v18', 'M15 3v18'],
  marks: ['M13 4v16', 'M17 4v16', 'M19 4H9.5a4.5 4.5 0 0 0 0 9H13'],
  print: ['M6 18H4a2 2 0 0 1-2-2v-5a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2v5a2 2 0 0 1-2 2h-2', 'M6 9V3a1 1 0 0 1 1-1h10a1 1 0 0 1 1 1v6', 'M6 14h12v8H6z'],
  send: ['m22 2-7 20-4-9-9-4Z', 'M22 2 11 13'],
  amend: ['M12.5 22H18a2 2 0 0 0 2-2V7l-5-5H6a2 2 0 0 0-2 2v9.5', 'M14 2v4a2 2 0 0 0 2 2h4', 'M13.378 15.626a1 1 0 1 0-3.004-3.004l-5.01 5.012a2 2 0 0 0-.506.854l-.837 2.87a.5.5 0 0 0 .62.62l2.87-.837a2 2 0 0 0 .854-.506z'],
};
const icon = (name: string) =>
  `<svg class="tb-icon" viewBox="0 0 24 24" aria-hidden="true">${ICONS[name].map((d) => `<path d="${d}"/>`).join('')}</svg>`;

export function mountEditor(root: HTMLElement): void {
  root.innerHTML = `
    <div class="editor">
      <div class="editor-toolbar" role="toolbar">
        <div class="tb-group" role="group" aria-label="파일">
          <button type="button" data-action="open" title="HLT · 제출파일(.fin · .hlz) 열기">${icon('open')}<span>열기</span></button>
          <button type="button" data-action="new-opinion" title="빈 의견서(【의견내용】)를 새로 만든다">${icon('opinion')}<span>새 의견서</span></button>
          <button type="button" data-action="import" title="워드 · KIPO XML · HML 을 불러와 새 HLT 로 만든다">${icon('import')}<span>불러와 HLT 만들기</span></button>
          <button type="button" data-action="new-tab" title="새 탭 (⌘T)">${icon('tab')}</button>
          <button type="button" data-action="save" disabled title="저장 — 연 HLT 에 덮어쓰기 (⌘S)">${icon('save')}<span>저장</span></button>
          <button type="button" data-action="save-as" disabled title="다른 이름으로 저장 (⇧⌘S)">${icon('saveAs')}</button>
          <button type="button" data-action="close-document" disabled title="문서 닫기">${icon('close')}<span>닫기</span></button>
        </div>
        <div class="tb-group" role="group" aria-label="편집">
          <button type="button" data-action="undo" disabled title="되돌리기 (⌘Z)">${icon('undo')}</button>
          <button type="button" data-action="redo" disabled title="다시 하기 (⇧⌘Z)">${icon('redo')}</button>
          <button type="button" data-action="find" disabled title="찾기·바꾸기 (⌘F)">${icon('find')}<span>찾기</span></button>
        </div>
        <div class="tb-group panel-formats" role="group" aria-label="글자모양">
          <button type="button" data-format="bold" title="굵게 (⌘B)"><b>가</b></button>
          <button type="button" data-format="italic" title="기울임 (⌘I)"><i>가</i></button>
          <button type="button" data-format="underline" title="밑줄 (⌘U)"><u>가</u></button>
          <button type="button" data-format="superscript" title="위첨자">x<sup>2</sup></button>
          <button type="button" data-format="subscript" title="아래첨자">x<sub>2</sub></button>
        </div>
        <div class="tb-group panel-aligns" role="group" aria-label="문단 정렬">
          <button type="button" data-action="align-justified" disabled title="양쪽 정렬 (커서가 있는 문단)">${icon('justify')}</button>
          <button type="button" data-action="align-center" disabled title="가운데 정렬 (커서가 있는 문단)">${icon('center')}</button>
          <button type="button" data-action="align-left" disabled title="왼쪽 정렬 (커서가 있는 문단)">${icon('left')}</button>
        </div>
        <div class="tb-group" role="group" aria-label="넣기">
          <button type="button" data-action="math" disabled title="수식 넣기(LaTeX)">${icon('math')}<span>수식</span></button>
          <button type="button" data-action="chem" disabled title="화학식 넣기(구조식)">${icon('chem')}<span>화학식</span></button>
          <button type="button" data-action="table" disabled title="표 넣기(커서 문단 아래 · 고치기는 표 칸 위 우클릭)">${icon('table')}<span>표</span></button>
          <button type="button" data-action="marks" aria-pressed="false" title="문단부호 보기">${icon('marks')}</button>
        </div>
        <div class="tb-group" role="group" aria-label="내보내기">
          <button type="button" data-action="pdf" disabled title="인쇄 PDF">${icon('print')}<span>PDF</span></button>
          <button type="button" data-action="fin" disabled title="제출파일(.hlz · .fin) 만들기">${icon('send')}<span>제출파일</span></button>
          <button type="button" data-action="amend" disabled title="보정서 만들기">${icon('amend')}<span>보정서</span></button>
        </div>
        <span class="editor-status" aria-live="polite"></span>
        <span class="editor-error" role="alert" hidden title="눌러서 닫기"></span>
      </div>
      <div class="find-bar" hidden>
        <label>찾을 글 <input class="find-text" type="text" placeholder="(비우면 서식만으로)"></label>
        <select class="find-format" aria-label="찾을 서식">
          <option value="">서식 상관없음</option>
          <option value="superscript">위첨자</option>
          <option value="subscript">아래첨자</option>
          <option value="bold">굵게</option>
          <option value="italic">기울임</option>
          <option value="underline">밑줄</option>
          <option value="plain">서식 없음</option>
        </select>
        <button type="button" data-action="find-next">다음 찾기</button>
        <span class="sep"></span>
        <label>바꿀 글 <input class="replace-text" type="text"></label>
        <select class="replace-format" aria-label="바꿀 서식">
          <option value="keep">서식 그대로</option>
          <option value="superscript">위첨자로</option>
          <option value="subscript">아래첨자로</option>
          <option value="normal">보통 글자로(첨자 없앰)</option>
          <option value="bold">굵게 더하기</option>
          <option value="italic">기울임 더하기</option>
          <option value="underline">밑줄 더하기</option>
          <option value="clear">서식 모두 없앰</option>
        </select>
        <button type="button" data-action="replace-one">바꾸기</button>
        <button type="button" data-action="replace-all">모두 바꾸기</button>
        <span class="find-count"></span>
        <button type="button" data-action="find-close" title="닫기">✕</button>
      </div>
      <textarea class="caret-input" aria-label="쪽 위 입력" autocomplete="off" spellcheck="false"></textarea>
      <div class="editor-body">
        <nav class="editor-outline" aria-label="구조"></nav>
        <div class="editor-pages" tabindex="0"></div>
        <aside class="editor-panel" hidden>
          <p class="panel-title"></p>
          <div class="panel-text" contenteditable="plaintext-only" spellcheck="false"></div>
          <p class="panel-hint">엔터 = 문단 나누기 · 「_」 = 그림 자리(지우면 그림이 빠진다)</p>
          <div class="panel-actions">
            <button type="button" data-action="apply">적용</button>
            <button type="button" data-action="close">닫기</button>
          </div>
        </aside>
      </div>
      <div class="figure-dialog" hidden>
        <div class="figure-box">
          <p class="figure-title"></p>
          <div class="figure-draw-toggle" hidden><button type="button" data-action="figure-draw">그리기 판 열기</button></div>
          <div class="figure-drawer" hidden></div>
          <textarea class="figure-input" rows="5" spellcheck="false"></textarea>
          <div class="figure-preview"></div>
          <label>크기(pt) 가로 <input class="figure-width" type="number" min="1" max="500" step="1">
            세로 <input class="figure-height" type="number" min="1" max="700" step="1"></label>
          <p class="figure-error" aria-live="polite"></p>
          <div class="panel-actions">
            <button type="button" data-action="figure-insert">넣기</button>
            <button type="button" data-action="figure-cancel">취소</button>
          </div>
        </div>
      </div>
    </div>`;
  const q = <T extends Element>(selector: string) => root.querySelector<T>(selector)!;
  const pagesRoot = q<HTMLDivElement>('.editor-pages');
  const outlineRoot = q<HTMLElement>('.editor-outline');
  const status = q<HTMLSpanElement>('.editor-status');
  const panel = q<HTMLElement>('.editor-panel');
  const panelTitle = q<HTMLParagraphElement>('.panel-title');
  const panelText = q<HTMLDivElement>('.panel-text');
  /** 편집 창 글(줄바꿈 = 문단 나누기). */
  const panelValue = () => panelText.innerText.replace(/\n$/, '');
  const setPanel = (text: string, spans: EditorParagraphDto['spans'] = []) =>
    panelText.replaceChildren(richParagraph(text, spans));
  const dialog = q<HTMLElement>('.figure-dialog');
  const figureTitle = q<HTMLParagraphElement>('.figure-title');
  const figureInput = q<HTMLTextAreaElement>('.figure-input');
  const figurePreview = q<HTMLDivElement>('.figure-preview');
  const figureWidth = q<HTMLInputElement>('.figure-width');
  const figureHeight = q<HTMLInputElement>('.figure-height');
  const figureError = q<HTMLParagraphElement>('.figure-error');
  let figureKind: 'math' | 'chemistry' = 'math';
  const drawer = q<HTMLDivElement>('.figure-drawer');
  const drawToggle = q<HTMLDivElement>('.figure-draw-toggle');
  let drawerOpen = false;
  let figure: Figure | null = null;
  let previewTimer = 0;

  async function preview() {
    try {
      figure =
        figureKind === 'math'
          ? await renderLatex(figureInput.value)
          : await renderChemistry(figureInput.value);
      figurePreview.innerHTML = figure.svg;
      figureWidth.value = figure.widthPt.toFixed(0);
      figureHeight.value = figure.heightPt.toFixed(0);
      figureError.textContent = '';
    } catch (error) {
      figure = null;
      figurePreview.replaceChildren();
      figureError.textContent = String(error instanceof Error ? error.message : error);
    }
  }
  figureInput.addEventListener('input', () => {
    window.clearTimeout(previewTimer);
    previewTimer = window.setTimeout(() => void preview(), 250);
  });
  // 가로를 바꾸면 세로는 비율대로.
  figureWidth.addEventListener('input', () => {
    if (figure) figureHeight.value = ((Number(figureWidth.value) * figure.heightPt) / figure.widthPt).toFixed(0);
  });

  function openFigure(kind: 'math' | 'chemistry') {
    if (!selected) {
      say('넣을 자리를 먼저 고르세요 — 쪽에서 문단을 누르면 그 뒤에 넣습니다.');
      return;
    }
    figureKind = kind;
    figureTitle.textContent =
      kind === 'math' ? '수식 넣기 — LaTeX (예: \\frac{a+b}{c} = x^2)' : '화학식 넣기 — SMILES 또는 Molfile (예: c1ccccc1O)';
    figureInput.value = kind === 'math' ? 'x = \\frac{-b \\pm \\sqrt{b^2-4ac}}{2a}' : 'c1ccc2c(c1)ccc(n2)O';
    dialog.hidden = false;
    drawToggle.hidden = kind !== 'chemistry';
    drawer.hidden = !(kind === 'chemistry' && drawerOpen);
    void preview();
    figureInput.focus();
  }

  let view: EditorViewDto | null = null;
  let selected: string | null = null;
  /** 칸을 고치는 중이면 칸 id(서식 단추·엔터는 문단에서만). */
  let selectedCell: string | null = null;
  let lastSelection: [number, number] | null = null;
  // 찾기 결과(지금 판 기준) · 지금 보고 있는 결과 차례.
  let found: FindMatchDto[] = [];
  let foundAt = -1;
  let foundKey = '';
  /** 찾은 판(다시 구우면 오프셋이 낡는다 — 판이 바뀌면 다시 찾는다). */
  let foundView: EditorViewDto | null = null;
  let pages = new Map<number, PlannedPageDto>();
  const images = new Map<number, string>();
  const observer = new IntersectionObserver(
    (entries) => {
      for (const entry of entries) {
        if (entry.isIntersecting) void drawPage(Number((entry.target as HTMLElement).dataset.index));
      }
    },
    { root: pagesRoot, rootMargin: '600px 0px' },
  );

  const say = (text: string) => (status.textContent = text);
  // 실패는 상태 줄(곧 덮인다)이 아니라 따로 남기고(눌러서 닫기) 기록 파일에도 쓴다 — 원인을 나중에 볼 수 있게.
  const errorBox = q<HTMLSpanElement>('.editor-error');
  errorBox.addEventListener('click', () => (errorBox.hidden = true));
  /** 끝까지 보여야 하는 알림(눌러서 닫기). */
  const warn = (text: string) => {
    say(text);
    errorBox.textContent = text;
    errorBox.hidden = false;
  };
  const fail = (label: string, error: unknown) => {
    const text = `${label} 실패: ${describe(error)}`;
    say(text);
    errorBox.textContent = text;
    errorBox.hidden = false;
    void editorLog(`${label} 실패 ${JSON.stringify(error)}`).catch(() => {});
  };
  const enable = () => {
    for (const action of ['save', 'save-as', 'close-document', 'undo', 'redo', 'find', 'pdf', 'fin', 'amend', 'math', 'chem', 'table', 'align-justified', 'align-center', 'align-left']) {
      q<HTMLButtonElement>(`[data-action="${action}"]`).disabled = !view;
    }
  };

  async function drawPage(index: number) {
    const host = pagesRoot.querySelector<HTMLElement>(`.nk-page[data-index="${index}"]`);
    if (!host || host.dataset.drawn === '1') return;
    host.dataset.drawn = '1';
    let page = pages.get(index);
    if (!page) {
      page = await editorPage(index);
      pages.set(index, page);
    }
    for (const image of page.images) {
      if (!images.has(image.object_code)) {
        const bytes = await editorImage(image.object_code);
        images.set(image.object_code, URL.createObjectURL(new Blob([bytes], { type: 'image/png' })));
      }
    }
    host.replaceChildren(renderPage(page, SCALE, (code) => images.get(code) ?? null, panel.hidden ? null : selected));
    drawNumbers(host, page);
    drawMarks(host, page);
    paint();
  }

  /** 식별번호(최종본을 연 문서): 통특기처럼 문단 첫 줄 왼쪽 여백에 [0001](본문 왼쪽 끝 바로 앞에 오른쪽 맞춤). */
  function drawNumbers(host: HTMLElement, page: PlannedPageDto) {
    const lines = page.lines.filter((line) => line.paragraph && line.stops?.length);
    if (lines.length === 0) return;
    const margin = Math.min(...lines.map((line) => line.stops![0]));
    for (const line of lines) {
      if (line.start !== 0) continue;
      const number = paragraphOf(line.paragraph!)?.number;
      if (!number) continue;
      const mark = document.createElement('span');
      mark.className = 'nk-pnum';
      mark.textContent = `[${number}]`;
      mark.style.right = `${(pageWidth() - margin + 3) * SCALE}px`;
      mark.style.top = `${(line.top + 2) * SCALE}px`;
      mark.style.fontSize = `${8 * SCALE}px`;
      host.append(mark);
    }
  }
  const pageWidth = () => view?.page_width ?? 595;

  /** 문단부호 보기: 문단이 끝나는 줄 끝에 ↵(빈 문단 포함 · 화면에만 — 인쇄 · 저장엔 없다). */
  let showMarks = false;
  function drawMarks(host: HTMLElement, page: PlannedPageDto) {
    if (!showMarks) return;
    for (const line of page.lines) {
      if (!line.paragraph || line.start === undefined || !line.stops?.length) continue;
      if (line.start + line.stops.length - 1 !== charsOf(line.paragraph).length) continue;
      // ↵ 를 그림으로 그린다(글꼴 글자는 화살표가 글줄 가운데에 떠 있다) — 바닥이 글자 바닥선(마침표 높이)에 닿게.
      const mark = document.createElement('span');
      mark.className = 'nk-pmark';
      mark.innerHTML = PARAGRAPH_MARK;
      mark.style.left = `${(line.stops.at(-1)! + 1) * SCALE}px`;
      mark.style.top = `${(line.top + BASELINE - MARK_HEIGHT) * SCALE}px`;
      mark.style.width = `${MARK_WIDTH * SCALE}px`;
      mark.style.height = `${MARK_HEIGHT * SCALE}px`;
      host.append(mark);
    }
  }

  function layout(next: EditorViewDto, keepScroll: boolean) {
    const scroll = pagesRoot.scrollTop;
    view = next;
    pages = new Map();
    observer.disconnect();
    // 쪽 수가 같으면 쪽 상자를 그대로 두고 새 쪽이 오면 바꿔 그린다(다시 굽고 빈 쪽이 깜박이지 않게).
    const hosts = pagesRoot.querySelectorAll<HTMLElement>('.nk-page');
    if (keepScroll && hosts.length === next.page_count) {
      for (const host of hosts) {
        host.dataset.drawn = '0';
        observer.observe(host);
      }
    } else {
      pagesRoot.replaceChildren();
      for (let index = 0; index < next.page_count; index += 1) {
        const page = document.createElement('div');
        page.className = 'nk-page';
        page.dataset.index = String(index);
        page.style.width = `${next.page_width * SCALE}px`;
        page.style.height = `${next.page_height * SCALE}px`;
        pagesRoot.append(page);
        observer.observe(page);
      }
    }
    if (keepScroll) pagesRoot.scrollTop = scroll;
    renderOutline(next);
    say(`${next.title} · ${next.page_count}쪽${next.editable ? '' : ' · 보기 전용(이 서식은 고칠 수 없다)'}`);
    enable();
    watchPreparing(next);
  }

  /** 문서 닫기 뒤: 쪽 · 구조 창 · 커서 · 찾기 결과를 비우고 처음 화면으로(단추는 다시 잠근다). */
  function clearDocument() {
    view = null;
    pages = new Map();
    observer.disconnect();
    pagesRoot.replaceChildren();
    outlineRoot.replaceChildren();
    images.clear();
    caret = anchor = null;
    selected = selectedCell = null;
    panel.hidden = true;
    found = [];
    foundAt = -1;
    foundKey = '';
    foundView = null;
    q<HTMLElement>('.find-bar').hidden = true;
    paint();
    enable();
    say('문서를 닫았습니다.');
  }

  /** 접어 둔 마디(경로 = 위 마디 이름들 — 다시 그려도 문단 id 와 달리 그대로다). */
  const collapsed = new Set<string>();

  /** 구조 창: 통특기 왼쪽 트리처럼 [−]/[+] 접기 · 책 아이콘 · 점선. 마디를 누르면 그 쪽으로. */
  function renderOutline(next: EditorViewDto) {
    const build = (node: OutlineNode, path: string): HTMLLIElement => {
      const key = `${path}/${node.label}`;
      const item = document.createElement('li');
      item.classList.toggle('collapsed', collapsed.has(key));
      const row = document.createElement('div');
      row.className = 'tree-row';
      const twisty = document.createElement('button');
      twisty.type = 'button';
      twisty.className = 'tree-twisty';
      if (node.children.length) {
        twisty.textContent = collapsed.has(key) ? '+' : '−';
        twisty.setAttribute('aria-label', '접기 · 펴기');
        twisty.addEventListener('click', () => {
          if (collapsed.has(key)) collapsed.delete(key);
          else collapsed.add(key);
          item.classList.toggle('collapsed', collapsed.has(key));
          twisty.textContent = collapsed.has(key) ? '+' : '−';
        });
      } else twisty.classList.add('leaf');
      const label = document.createElement('button');
      label.type = 'button';
      label.className = node.id ? 'tree-label' : 'tree-label root';
      if (node.id) label.innerHTML = BOOK_ICON;
      label.append(node.label);
      label.addEventListener('click', () => {
        const index = node.id ? next.paragraph_pages[node.id] : 0;
        pagesRoot.querySelector(`.nk-page[data-index="${index}"]`)?.scrollIntoView();
      });
      row.append(twisty, label);
      item.append(row);
      if (node.children.length) {
        const list = document.createElement('ul');
        list.append(...node.children.map((child) => build(child, key)));
        item.append(list);
      }
      return item;
    };
    const tree = document.createElement('ul');
    tree.className = 'tree';
    tree.append(build(outlineTree(next.title, next.paragraphs), ''));
    outlineRoot.replaceChildren(tree);
  }

  // 고침은 줄을 세워 차례로 보낸다 — 처리 중에 누른 단추도 버리지 않는다(앞 고침이 끝나면 이어서).
  let queue: Promise<void> = Promise.resolve();
  let waiting = 0;
  const busy = () => {
    root.classList.toggle('busy', waiting > 0);
    if (waiting > 1) say(`처리 중 · 대기 ${waiting - 1}`);
  };
  /** `apply` 가 거짓이면 받은 보기를 버린다(다시 굽는 동안 쪽 위에서 더 친 글이 있다 — 다음 굽기가 그린다). */
  function run(label: string, action: () => Promise<EditorViewDto>, apply: () => boolean = () => true): Promise<void> {
    waiting += 1;
    busy();
    const job = queue.then(async () => {
      say(`${label} 중…${waiting > 1 ? ` · 대기 ${waiting - 1}` : ''}`);
      try {
        const next = await action();
        if (apply()) layout(next, true);
        else if (waiting <= 1) say('');
      } catch (error) {
        fail(`${label}`, error);
      } finally {
        waiting -= 1;
        busy();
      }
    });
    queue = job;
    return job;
  }

  // 큰 문서는 편집 모델을 뒤에서 만든다 — 다 될 때까지 상태를 알리고 2초마다 묻는다.
  let preparingTimer = 0;
  function watchPreparing(next: EditorViewDto) {
    window.clearTimeout(preparingTimer);
    if (!next.preparing) return;
    say(`${next.title} · ${next.page_count}쪽 · 편집 준비 중(큰 문서는 수십 초) — 그동안 보기·찾기는 된다`);
    preparingTimer = window.setTimeout(async () => {
      try {
        const latest = await editorView();
        if (!latest.preparing) {
          view = { ...latest };
          say(`${latest.title} · ${latest.page_count}쪽 · 편집 준비 끝`);
        } else watchPreparing(latest);
      } catch {
        // 문서를 닫았다.
      }
    }, 2000);
  }

  function openPanel(paragraphId: string) {
    const paragraph = view?.paragraphs.find((item) => item.id === paragraphId);
    if (!paragraph || !view) return;
    selected = paragraphId;
    selectedCell = null;
    panel.hidden = false;
    panelTitle.textContent = `${paragraph.wipo_id ? `[${String(paragraph.wipo_id).padStart(4, '0')}] ` : ''}${paragraph.id}`;
    setPanel(paragraph.text, paragraph.spans);
    panelText.contentEditable = view.editable ? 'plaintext-only' : 'false';
    for (const line of pagesRoot.querySelectorAll<HTMLElement>('.nk-line')) {
      line.classList.toggle('selected', line.dataset.paragraph === paragraphId);
    }
    panelText.focus();
  }

  // 표 칸은 아직 칸 창으로 고친다(쪽 위 커서는 본문 문단만).
  pagesRoot.addEventListener('click', (event) => {
    const line = (event.target as HTMLElement).closest<HTMLElement>('.nk-line');
    if (line?.dataset.cell) void openCell(line.dataset.cell, line.dataset.paragraph ?? null);
  });

  async function openCell(cellId: string, paragraphId: string | null) {
    if (!view) return;
    try {
      setPanel(await editorCellText(cellId));
    } catch (error) {
      fail(`칸 읽기`, error);
      return;
    }
    selected = paragraphId;
    selectedCell = cellId;
    panel.hidden = false;
    panelTitle.textContent = `표 칸 ${cellId}`;
    panelText.contentEditable = view.editable ? 'plaintext-only' : 'false';
    for (const line of pagesRoot.querySelectorAll<HTMLElement>('.nk-line')) {
      line.classList.toggle('selected', line.dataset.cell === cellId);
    }
    panelText.focus();
  }

  // ── 쪽 위 편집(WYSIWYG) ────────────────────────────────────
  // 쪽 그림이 곧 편집 화면이다. 커서 · 고른 구간은 쪽 계획의 글자 경계(`stops`)로 그리고, 키 입력은 숨은 입력 칸
  // (한글 조합 포함)으로 받는다. 친 글은 그 자리에서 바로 보이게 임시로 끼우고(줄은 안 흘린다), 0.4초 쉬면
  // 문단을 다시 굽어 통특기 줄나눔으로 바로잡는다. 문단 나누기 · 붙이기 · 서식은 다시 굽기를 기다리고,
  // 그동안 누른 키는 버리지 않고 뒤에 이어 처리한다.
  const input = q<HTMLTextAreaElement>('.caret-input');
  let caret: Caret | null = null;
  /** 고른 구간의 다른 끝(같은 문단). 커서와 같으면 고른 것이 없다. */
  let anchor: Caret | null = null;
  let goalX: number | null = null;
  let composing = false;
  let reveal = false;
  /** 다시 굽지 않은 글 고침이 있는 문단 · 임시 고침 수(다시 굽는 동안 더 쳤는가). */
  let dirtyId: string | null = null;
  let edits = 0;
  let commitTimer = 0;
  let holding = 0;
  const backlog: (() => void)[] = [];

  const paragraphOf = (id: string) => view?.paragraphs.find((item) => item.id === id);
  const charsOf = (id: string) => Array.from(paragraphOf(id)?.text ?? '');
  const neighbour = (id: string, step: -1 | 1) => {
    const list = view?.paragraphs ?? [];
    return list[list.findIndex((item) => item.id === id) + step]?.id ?? null;
  };
  const order = (id: string) => (view?.paragraphs ?? []).findIndex((item) => item.id === id);
  /** 고른 구간(문단 차례대로 from → to · 여러 문단에 걸칠 수 있다). 고른 것이 없으면 null. */
  const span = (): { from: Caret; to: Caret } | null => {
    if (!caret || !anchor) return null;
    const [a, c] = [order(anchor.paragraph), order(caret.paragraph)];
    if (a === c && anchor.offset === caret.offset) return null;
    return a < c || (a === c && anchor.offset < caret.offset) ? { from: anchor, to: caret } : { from: caret, to: anchor };
  };
  /** 고른 구간을 문단마다 [id, start, end) 로. */
  const pieces = (selection: { from: Caret; to: Caret }): [string, number, number][] => {
    const list = (view?.paragraphs ?? []).slice(order(selection.from.paragraph), order(selection.to.paragraph) + 1);
    return list.map((item, k) => [
      item.id,
      k === 0 ? selection.from.offset : 0,
      k === list.length - 1 ? selection.to.offset : charsOf(item.id).length,
    ]);
  };
  /** 한 문단 안의 고른 구간 [a, b). 여러 문단에 걸치면 null. */
  const range = (): [number, number] | null => {
    const selection = span();
    return selection && selection.from.paragraph === selection.to.paragraph
      ? [selection.from.offset, selection.to.offset]
      : null;
  };
  const pageHost = (index: number) => pagesRoot.querySelector<HTMLElement>(`.nk-page[data-index="${index}"]`);
  const act = (job: () => void) => (holding > 0 ? backlog.push(job) : job());

  function overlay(host: HTMLElement, className: string, x: number, top: number, width: number) {
    const element = document.createElement('div');
    element.className = className;
    element.style.left = `${x * SCALE}px`;
    element.style.top = `${(top - 1) * SCALE}px`;
    element.style.height = `${14 * SCALE}px`;
    if (width) element.style.width = `${width * SCALE}px`;
    host.append(element);
    return element;
  }

  /** 커서 · 고른 구간 · 조합 중인 글을 다시 그린다. */
  function paint() {
    for (const element of pagesRoot.querySelectorAll('.nk-caret, .nk-sel, .nk-compose')) element.remove();
    if (!caret) return;
    const lines = caretLines(pages);
    const selection = span();
    for (const [id, a, b] of selection ? pieces(selection) : []) {
      for (const box of selectionBoxes(lines, id, a, b)) {
        const host = pageHost(box.page);
        if (host) overlay(host, 'nk-sel', box.x, box.top, box.width);
      }
    }
    const point = caretPoint(lines, caret);
    const host = point && pageHost(point.page);
    if (!point || !host) return;
    const mark = overlay(host, 'nk-caret', point.x, point.top, 0);
    if (composing && input.value) {
      const compose = overlay(host, 'nk-compose', point.x, point.top, 0);
      compose.textContent = input.value;
      compose.style.fontSize = `${12 * SCALE}px`;
    }
    const rect = mark.getBoundingClientRect();
    input.style.left = `${rect.left}px`;
    input.style.top = `${rect.top}px`;
    if (reveal) {
      reveal = false;
      mark.scrollIntoView({ block: 'nearest' });
    }
  }

  function redraw(index: number) {
    const host = pageHost(index);
    const page = pages.get(index);
    if (host && page && host.dataset.drawn === '1') {
      host.replaceChildren(renderPage(page, SCALE, (code) => images.get(code) ?? null, null));
      drawNumbers(host, page);
      drawMarks(host, page);
    }
  }

  function place(next: Caret, extend: boolean, keepGoal = false) {
    if (dirtyId && dirtyId !== next.paragraph) void commit();
    if (!extend || !anchor) anchor = next;
    caret = next;
    selected = next.paragraph;
    if (!keepGoal) goalX = null;
    reveal = true;
    paint();
  }

  function canEdit(): boolean {
    if (!view?.editable) {
      say('이 서식은 보기 전용입니다.');
      return false;
    }
    return true;
  }

  function schedule() {
    window.clearTimeout(commitTimer);
    commitTimer = window.setTimeout(() => void commit(), 400);
  }

  /** 쪽 위에서 친 글을 문단 고침으로 보낸다(다시 굽기). 고칠 것이 없으면 앞 고침이 끝나기만 기다린다. */
  function commit(): Promise<void> {
    window.clearTimeout(commitTimer);
    if (!dirtyId) return queue;
    const id = dirtyId;
    const text = charsOf(id).join('');
    dirtyId = null;
    const sent = edits;
    return run(
      '고치기',
      async () => {
        try {
          return await editorReplaceText(id, text);
        } catch (error) {
          // 바탕이 거절하면(통특기에 없는 글자 등) 화면을 바탕 판으로 되돌린다 — 안 그러면 친 글이 남아 그 문단 고침이 계속 실패한다.
          dirtyId = null;
          layout(await editorView(), true);
          // 커서는 빠진 글자 수만큼 물러난다.
          if (caret?.paragraph === id) {
            const length = charsOf(id).length;
            const removed = Array.from(text).length - length;
            caret = { paragraph: id, offset: Math.max(0, Math.min(length, caret.offset - removed)) };
            anchor = caret;
          }
          throw error;
        }
      },
      () => edits === sent && !dirtyId,
    );
  }

  /** 문단 차례가 바뀌는 고침(나누기 · 붙이기)과 서식: 친 글을 먼저 보내고, 끝날 때까지 키를 모았다가 이어 처리한다. */
  async function structural(label: string, action: () => Promise<{ next: EditorViewDto; caret: Caret | null }>) {
    holding += 1;
    try {
      await commit();
      let after: Caret | null = null;
      await run(label, async () => {
        const result = await action();
        after = result.caret;
        return result.next;
      });
      if (after) {
        caret = after;
        anchor = after;
      }
      reveal = true;
      paint();
    } finally {
      holding -= 1;
      while (holding === 0 && backlog.length > 0) backlog.shift()!();
    }
  }

  function setText(id: string, chars: string[]) {
    const paragraph = paragraphOf(id);
    if (!paragraph) return;
    paragraph.text = chars.join('');
    dirtyId = id;
    edits += 1;
  }

  function deleteRange(a: number, b: number) {
    if (!caret || a >= b) return;
    const id = caret.paragraph;
    const chars = charsOf(id);
    chars.splice(a, b - a);
    setText(id, chars);
    const page = previewDelete(pages, id, a, b);
    if (page !== null) redraw(page);
    caret = { paragraph: id, offset: a };
    anchor = caret;
    if (page === null) void commit();
    else schedule();
  }

  /** 고른 것을 지운다. 여러 문단이면 다시 굽기 한 번(되돌리기 한 번) — 표 · 표제가 걸리면 백엔드가 거부한다. */
  function deleteSelection() {
    const selection = span();
    if (!selection || !canEdit()) return;
    const { from, to } = selection;
    if (from.paragraph === to.paragraph) return deleteRange(from.offset, to.offset);
    void structural('지우기', async () => ({
      next: await editorDeleteRange(from.paragraph, from.offset, to.paragraph, to.offset),
      caret: from,
    }));
  }

  function insertText(text: string) {
    if (!caret || !text || !canEdit()) return;
    if (span() && !range()) {
      // 여러 문단 덮어 치기: 지우기가 끝난 뒤 그 자리에 친다(그동안 키는 모아 둔다).
      // 지우기가 거부되면(표제 · 표) 고른 것이 남는다 — 그때는 친 글을 버린다(다시 지우기를 되풀이하지 않게).
      deleteSelection();
      return act(() => {
        if (!span() || range()) insertText(text);
      });
    }
    const selection = range();
    if (selection) deleteRange(...selection);
    const id = caret.paragraph;
    const chars = charsOf(id);
    const added = Array.from(text);
    if (text.includes('\n')) {
      // 문단 나누기(엔터 · 여러 줄 붙여넣기): 새 문단 차례는 다시 굽고 나서 안다.
      chars.splice(caret.offset, 0, ...added);
      const pieces = chars.join('').split('\n');
      const tail = Array.from(text.split('\n').at(-1)!).length;
      void structural('문단 나누기', async () => {
        const next = await editorReplaceText(id, chars.join(''));
        const index = next.paragraphs.findIndex((item) => item.id === id) + pieces.length - 1;
        const target = next.paragraphs[index];
        return { next, caret: target ? { paragraph: target.id, offset: tail } : null };
      });
      return;
    }
    chars.splice(caret.offset, 0, ...added);
    setText(id, chars);
    const page = previewInsert(pages, caret, text);
    if (page !== null) redraw(page);
    caret = { paragraph: id, offset: caret.offset + added.length };
    anchor = caret;
    reveal = true;
    paint();
    schedule();
  }

  /** 강제 쪽나눔: 고른 것을 지우고 커서 자리에서 나눈다 — 뒤 조각이 새 쪽 첫머리(다시 굽기 한 번 · 되돌리기 한 번). */
  function pageBreak() {
    if (!caret || !canEdit()) return;
    if (span() && !range()) return say('쪽나눔: 여러 문단을 고른 채로는 못 나눕니다.');
    const selection = range();
    if (selection) deleteRange(...selection);
    const id = caret.paragraph;
    const chars = charsOf(id);
    chars.splice(caret.offset, 0, '\n');
    void structural('쪽나눔', async () => {
      const next = await editorReplaceTextPageBreak(id, chars.join(''));
      const target = next.paragraphs[next.paragraphs.findIndex((item) => item.id === id) + 1];
      return { next, caret: target ? { paragraph: target.id, offset: 0 } : null };
    });
  }

  /** 문단 첫머리 Backspace(앞 문단에 붙이기) · 끝 Delete(뒤 문단을 붙이기). */
  function merge(into: string, from: string) {
    if (!canEdit()) return;
    const offset = charsOf(into).length;
    void structural('문단 붙이기', async () => ({
      next: await editorMergeParagraph(from),
      caret: { paragraph: into, offset },
    }));
  }

  function key(name: string, shift: boolean) {
    if (!caret) return;
    const length = charsOf(caret.paragraph).length;
    const selection = span();
    const lines = caretLines(pages);
    switch (name) {
      case 'ArrowLeft':
      case 'ArrowRight': {
        const step = name === 'ArrowLeft' ? -1 : 1;
        if (selection && !shift) return place(step < 0 ? selection.from : selection.to, false);
        const offset = caret.offset + step;
        if (offset >= 0 && offset <= length) return place({ ...caret, offset }, shift);
        const other = neighbour(caret.paragraph, step);
        if (other && !shift) place({ paragraph: other, offset: step < 0 ? charsOf(other).length : 0 }, false);
        return;
      }
      case 'ArrowUp':
      case 'ArrowDown': {
        goalX ??= caretPoint(lines, caret)?.x ?? 0;
        return place(vertical(lines, caret, name === 'ArrowUp' ? -1 : 1, goalX), shift, true);
      }
      case 'Home':
      case 'End':
        return place(lineEdge(lines, caret, name === 'End'), shift);
      case 'Backspace':
        if (!canEdit()) return;
        if (selection) return deleteSelection();
        if (caret.offset > 0) return deleteRange(caret.offset - 1, caret.offset);
        {
          const previous = neighbour(caret.paragraph, -1);
          if (previous) merge(previous, caret.paragraph);
        }
        return;
      case 'Delete':
        if (!canEdit()) return;
        if (selection) return deleteSelection();
        if (caret.offset < length) return deleteRange(caret.offset, caret.offset + 1);
        {
          const next = neighbour(caret.paragraph, 1);
          if (next) merge(caret.paragraph, next);
        }
        return;
      case 'Enter':
        return insertText('\n');
      case 'Escape':
        anchor = caret;
        return paint();
    }
  }

  const pointCaret = (event: MouseEvent): Caret | null => {
    const host = (event.target as HTMLElement).closest<HTMLElement>('.nk-page');
    if (!host) return null;
    const box = host.getBoundingClientRect();
    return hit(caretLines(pages), Number(host.dataset.index), (event.clientX - box.left) / SCALE, (event.clientY - box.top) / SCALE);
  };
  let dragging = false;
  pagesRoot.addEventListener('mousedown', (event) => {
    if (event.button !== 0 || !view) return;
    if ((event.target as HTMLElement).closest<HTMLElement>('.nk-line')?.dataset.cell) return;
    const next = pointCaret(event);
    if (!next) return;
    event.preventDefault();
    const shift = event.shiftKey;
    act(() => place(next, shift));
    input.focus({ preventScroll: true });
    dragging = true;
  });
  window.addEventListener('mousemove', (event) => {
    // 뗌(mouseup)을 놓치면(창 밖에서 뗌 · 창이 앞으로 오는 첫 클릭) 끌기가 남아 손만 움직여도 커서로 화면이 끌려간다 — 단추를 보고 끝낸다.
    if (dragging && !(event.buttons & 1)) dragging = false;
    if (!dragging) return;
    const next = pointCaret(event);
    if (next) act(() => place(next, true));
  });
  window.addEventListener('mouseup', () => (dragging = false));
  // 스크롤하면 기다리던 「커서 보이기」(다시 굽기가 끝나면 커서로 옮기기)를 버린다 — 사용자가 다른 곳을 보러 갔다.
  pagesRoot.addEventListener('wheel', () => (reveal = false), { passive: true });
  pagesRoot.addEventListener('dblclick', (event) => {
    // 낱말 고르기: 공백 사이.
    const at = pointCaret(event);
    if (!at) return;
    const chars = charsOf(at.paragraph);
    let a = at.offset;
    let b = at.offset;
    while (a > 0 && chars[a - 1] !== ' ') a -= 1;
    while (b < chars.length && chars[b] !== ' ') b += 1;
    act(() => {
      place({ ...at, offset: a }, false);
      place({ ...at, offset: b }, true);
    });
  });

  input.addEventListener('keydown', (event) => {
    if (composing || event.isComposing || !caret) return;
    const meta = event.metaKey || event.ctrlKey;
    if (meta && ['b', 'i', 'u'].includes(event.key.toLowerCase())) {
      event.preventDefault();
      const format = ({ b: 'bold', i: 'italic', u: 'underline' } as const)[event.key.toLowerCase() as 'b' | 'i' | 'u'];
      void pageFormat(format);
      return;
    }
    // 복사 · 잘라내기: 숨은 입력칸에는 고른 글이 없어 WebKit 이 copy/cut 이벤트를 보내지 않는다 — 키를 직접 받는다.
    if (meta && ['c', 'x'].includes(event.key.toLowerCase()) && span()) {
      event.preventDefault();
      clip(event.key.toLowerCase() === 'x');
      return;
    }
    if (meta && event.key.toLowerCase() === 'z') {
      event.preventDefault();
      void history(event.shiftKey ? 'redo' : 'undo');
      return;
    }
    if (meta && event.key.toLowerCase() === 'a') {
      event.preventDefault();
      selectAll();
      return;
    }
    // 강제 쪽나눔(워드 Ctrl+Enter · 맥 ⌘+Enter): 커서 자리에서 문단을 나누고 뒤 조각을 새 쪽으로.
    if (meta && event.key === 'Enter') {
      event.preventDefault();
      act(() => pageBreak());
      return;
    }
    if (meta) return;
    // Tab: 포커스를 옮기지 않고 탭 문자를 넣는다(통특기도 본문에 0x09 를 그대로 둔다).
    if (event.key === 'Tab' && !event.altKey) {
      event.preventDefault();
      act(() => insertText('\t'));
      return;
    }
    if (['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End', 'Backspace', 'Delete', 'Enter', 'Escape'].includes(event.key)) {
      event.preventDefault();
      const { key: name, shiftKey } = event;
      act(() => key(name, shiftKey));
    }
  });
  // 한글 조합: 조합 중인 글은 커서 자리에 덧그리고, 조합이 끝나면 넣는다. 입력 칸은 늘 넣은 뒤 비운다
  // (WebKit 은 마지막 input 이 compositionend 앞에 올 때가 있다 — 어느 쪽이 먼저 와도 한 번만 넣는다).
  input.addEventListener('compositionstart', () => (composing = true));
  input.addEventListener('compositionend', () => {
    composing = false;
    const text = input.value;
    input.value = '';
    act(() => insertText(text));
    paint();
  });
  input.addEventListener('input', () => {
    if (composing) return paint();
    const text = input.value;
    input.value = '';
    if (text) act(() => insertText(text));
  });
  const selectedText = () => {
    const selection = span();
    return selection ? pieces(selection).map(([id, a, b]) => charsOf(id).slice(a, b).join('')).join('\n') : null;
  };
  function clip(cut: boolean) {
    if (!span()) return;
    void navigator.clipboard.writeText(selectedText() ?? '');
    if (cut && canEdit()) act(() => key('Backspace', false));
  }

  function selectAll() {
    const list = view?.paragraphs ?? [];
    if (list.length === 0) return;
    const last = list[list.length - 1].id;
    act(() => {
      place({ paragraph: list[0].id, offset: 0 }, false);
      place({ paragraph: last, offset: charsOf(last).length }, true);
    });
  }

  const compare = (a: Caret, b: Caret) => order(a.paragraph) - order(b.paragraph) || a.offset - b.offset;
  /** 우클릭 메뉴(macOS 메뉴). 고른 곳 밖을 누르면 커서를 그리로 옮긴다(편집기들처럼) · 안이면 고른 것을 둔다. */
  pagesRoot.addEventListener('contextmenu', (event) => {
    event.preventDefault();
    if (!view) return;
    const at = pointCaret(event);
    const selection = span();
    const inside =
      at &&
      selection &&
      compare(at, selection.from) >= 0 &&
      compare(at, selection.to) <= 0;
    if (at && !inside) act(() => place(at, false));
    input.focus({ preventScroll: true });
    const cell = (event.target as HTMLElement).closest<HTMLElement>('.nk-line')?.dataset.cell ?? null;
    void contextMenu(cell).catch((error) => fail('메뉴', error));
  });
  /** 식별항목 넣기: 번호를 묻고(같은 종류 가장 큰 번호 + 1 을 채워 둔다) 커서 문단 아래(문단 맨 앞이면 위)에 표제 + 빈 문단. */
  async function insertItem(kind: string) {
    if (!caret || !canEdit()) return;
    const at = caret;
    const numbers = (view?.paragraphs ?? [])
      .map((paragraph) => paragraph.text.match(/^【(.+) ([A-Za-z0-9]+)】$/))
      .filter((match) => match?.[1] === kind)
      .map((match) => match![2]);
    const suggested = String(Math.max(0, ...numbers.map((number) => parseInt(number, 10) || 0)) + 1);
    const number = await askNumber(kind, suggested);
    input.focus({ preventScroll: true });
    if (!number) return;
    const before = at.offset === 0 && charsOf(at.paragraph).length > 0;
    await structural('식별항목 넣기', async () => {
      const result = await editorInsertItem(at.paragraph, before, kind, number);
      return { next: result.view, caret: { paragraph: result.body, offset: 0 } };
    });
    if (numbers.includes(number)) {
      warn(`【${kind} ${number}】이 이미 있습니다 — 같은 번호는 보정서 만들 때 통특기가 거절합니다(E-243)`);
    }
  }

  /** 식별항목 삭제(통특기 「단락삭제」 창): 종류 + 번호(쉼표 · 범위)를 묻고 지운다. 커서가 표제 위면 그 종류 · 번호를 채워 둔다.
   *  【청구항 N】은 내용 문단까지 지운다(통특기와 같다) · 뒤 번호는 그대로. 커서는 지운 첫 표제의 앞 문단 끝으로. */
  async function removeItem() {
    if (!canEdit()) return;
    const here = caret ? charsOf(caret.paragraph).join('').match(/^【(.+) ([A-Za-z0-9]+)】$/) : null;
    const asked = await askRemoval(here?.[1] ?? '청구항', here?.[2] ?? '');
    input.focus({ preventScroll: true });
    if (!asked) return;
    const first = (view?.paragraphs ?? []).findIndex((paragraph) => paragraph.text === `【${asked.kind} ${asked.numbers[0]}】`);
    const previous = first > 0 ? view!.paragraphs[first - 1].id : null;
    const offset = previous ? charsOf(previous).length : 0;
    await structural('식별항목 삭제', async () => ({
      next: await editorRemoveItems(asked.kind, asked.numbers),
      caret: previous ? { paragraph: previous, offset } : null,
    }));
  }

  /** 단락삭제 창(통특기와 같은 꼴): 삭제 대상 종류 · 삭제 번호(「3, 5-7」). */
  function askRemoval(kind: string, number: string): Promise<{ kind: string; numbers: string[] } | null> {
    return new Promise((resolve) => {
      const box = document.createElement('form');
      box.className = 'item-ask item-remove';
      box.innerHTML = `<label>삭제 대상 <select>${ITEM_KINDS.map((k) => `<option value="${k}">【${k} 】</option>`).join('')}</select></label>
        <label>삭제 번호 <input class="item-numbers" value="${number}" size="10" autocomplete="off" spellcheck="false"></label>
        <button type="submit">단락삭제 실행</button><button type="button" class="item-cancel">취소</button><span class="item-hint">예: 3, 5-7</span>`;
      const select = box.querySelector('select')!;
      select.value = kind;
      const field = box.querySelector<HTMLInputElement>('.item-numbers')!;
      const done = (value: { kind: string; numbers: string[] } | null) => {
        box.remove();
        resolve(value);
      };
      box.addEventListener('submit', (event) => {
        event.preventDefault();
        const numbers = parseNumbers(field.value);
        if (numbers) done({ kind: select.value, numbers });
        else box.querySelector('.item-hint')!.textContent = '번호는 「3, 5-7」처럼 적습니다';
      });
      box.querySelector('.item-cancel')!.addEventListener('click', () => done(null));
      box.addEventListener('keydown', (event) => {
        if (event.key === 'Escape') done(null);
      });
      root.append(box);
      field.focus();
      field.select();
    });
  }

  /** 읽기 전용 보정서 보기(통특기 보정서생성 뒤 「…dta - 읽기 전용」 창): 【보정대상 항목】 · 【보정방법】 · 【보정내용】. */
  function showAmendment(done: AmendmentDoneDto) {
    const box = document.createElement('div');
    box.className = 'amend-view';
    const rows = done.items
      .map(
        (item) => `<p><b>【보정대상 항목】</b> ${escapeHtml(item.target)}</p><p><b>【보정방법】</b> ${escapeHtml(item.method)}</p>${
          item.content ? `<p><b>【보정내용】</b></p><pre>${escapeHtml(item.content)}</pre>` : ''
        }`,
      )
      .join('');
    box.innerHTML = `<header>${escapeHtml(done.dta)} - 읽기 전용<button type="button">닫기</button></header><div class="amend-body">${rows}</div>`;
    box.querySelector('button')!.addEventListener('click', () => box.remove());
    root.append(box);
  }

  /** 식별항목 번호 묻기(작은 입력 칸 · Enter = 넣기 · Esc = 취소). 영문 · 숫자만. */
  function askNumber(kind: string, suggested: string): Promise<string | null> {
    return new Promise((resolve) => {
      const box = document.createElement('form');
      box.className = 'item-ask';
      box.innerHTML = `<label>【${kind} <input class="item-number" value="${suggested}" size="6" autocomplete="off" spellcheck="false">】</label>
        <button type="submit">넣기</button><button type="button" class="item-cancel">취소</button><span class="item-hint">영문 · 숫자</span>`;
      const field = box.querySelector<HTMLInputElement>('.item-number')!;
      const done = (value: string | null) => {
        box.remove();
        resolve(value);
      };
      box.addEventListener('submit', (event) => {
        event.preventDefault();
        if (/^[A-Za-z0-9]+$/.test(field.value)) done(field.value);
        else box.querySelector('.item-hint')!.textContent = '번호는 영문과 숫자만 됩니다';
      });
      box.querySelector('.item-cancel')!.addEventListener('click', () => done(null));
      field.addEventListener('keydown', (event) => {
        if (event.key === 'Escape') done(null);
      });
      root.append(box);
      field.focus();
      field.select();
    });
  }

  /** 우클릭한 표 칸 기준의 표 메뉴(줄 · 칸 번호는 0 부터 · 넣기 번호 = 새 줄/칸이 설 자리). */
  function tableItems(at: CellPositionDto) {
    const table_id = at.table_id;
    const item = (text: string, operation: TableOperationDto, enabled = true) => ({
      text,
      enabled,
      action: () => void structural(text, async () => ({ next: await editorTableEdit(operation), caret: null })),
    });
    const lastRow = at.row + at.rowspan - 1;
    const lastColumn = at.column + at.colspan - 1;
    return [
      item('위에 줄 넣기', { kind: 'insert_row', table_id, index: at.row }),
      item('아래에 줄 넣기', { kind: 'insert_row', table_id, index: lastRow + 1 }),
      item('줄 지우기', { kind: 'delete_row', table_id, index: at.row }, at.rows > 1),
      { item: 'Separator' as const },
      item('왼쪽에 칸 넣기', { kind: 'insert_column', table_id, index: at.column }),
      item('오른쪽에 칸 넣기', { kind: 'insert_column', table_id, index: lastColumn + 1 }),
      item('칸 지우기', { kind: 'delete_column', table_id, index: at.column }, at.columns > 1),
      { item: 'Separator' as const },
      item(
        '오른쪽 칸과 합치기',
        { kind: 'merge', table_id, row: at.row, column: at.column, row_end: lastRow, column_end: lastColumn + 1 },
        lastColumn + 1 < at.columns,
      ),
      item(
        '아래 칸과 합치기',
        { kind: 'merge', table_id, row: at.row, column: at.column, row_end: lastRow + 1, column_end: lastColumn },
        lastRow + 1 < at.rows,
      ),
      item('합친 칸 나누기', { kind: 'split', table_id, row: at.row, column: at.column }, at.rowspan > 1 || at.colspan > 1),
      { item: 'Separator' as const },
      {
        text: '표 지우기',
        action: () => void structural('표 지우기', async () => ({ next: await editorDeleteTable(table_id), caret: null })),
      },
    ];
  }

  /** 표 그리기: 줄 · 칸 수를 묻고 커서 문단(없으면 고른 문단) 아래에 새 표 → 첫 칸을 칸 창으로 연다. */
  async function insertTable() {
    const after = caret?.paragraph ?? selected;
    if (!after || !canEdit()) {
      say('표: 넣을 자리(문단)를 먼저 누르세요.');
      return;
    }
    const size = await askTableSize();
    input.focus({ preventScroll: true });
    if (!size) return;
    let cell: string | null = null;
    await structural('표 넣기', async () => {
      const result = await editorInsertTable(after, size.rows, size.columns);
      cell = result.cell;
      return { next: result.view, caret: null };
    });
    if (cell) void openCell(cell, null);
  }

  function askTableSize(): Promise<{ rows: number; columns: number } | null> {
    return new Promise((resolve) => {
      const box = document.createElement('form');
      box.className = 'item-ask';
      box.innerHTML = `<label>줄 <input class="table-rows" type="number" min="1" max="200" value="2" size="4"></label>
        <label>칸 <input class="table-columns" type="number" min="1" max="40" value="2" size="4"></label>
        <button type="submit">표 넣기</button><button type="button" class="item-cancel">취소</button><span class="item-hint">줄 1–200 · 칸 1–40</span>`;
      const rows = box.querySelector<HTMLInputElement>('.table-rows')!;
      const columns = box.querySelector<HTMLInputElement>('.table-columns')!;
      const done = (value: { rows: number; columns: number } | null) => {
        box.remove();
        resolve(value);
      };
      box.addEventListener('submit', (event) => {
        event.preventDefault();
        const size = { rows: Number(rows.value), columns: Number(columns.value) };
        const ok = Number.isInteger(size.rows) && Number.isInteger(size.columns) && size.rows >= 1 && size.rows <= 200 && size.columns >= 1 && size.columns <= 40;
        if (ok) done(size);
        else box.querySelector('.item-hint')!.textContent = '줄은 1–200, 칸은 1–40 사이 정수';
      });
      box.querySelector('.item-cancel')!.addEventListener('click', () => done(null));
      box.addEventListener('keydown', (event) => {
        if (event.key === 'Escape') done(null);
      });
      root.append(box);
      rows.focus();
      rows.select();
    });
  }

  async function contextMenu(cell: string | null) {
    const has = span() !== null;
    const editable = Boolean(view?.editable);
    const at = cell && editable ? await editorCellPosition(cell).catch(() => null) : null;
    const onItem = caret !== null && ITEM_HEADING.test(charsOf(caret.paragraph).join(''));
    const format = (text: string, accelerator: string | undefined, value: CharacterFormatDto) => ({
      text,
      accelerator,
      enabled: has && editable,
      action: () => void pageFormat(value),
    });
    const button = (action: string) => () => q<HTMLButtonElement>(`[data-action="${action}"]`).click();
    const menu = await Menu.new({
      items: [
        { text: '잘라내기', accelerator: 'CmdOrCtrl+X', enabled: has && editable, action: () => clip(true) },
        { text: '복사', accelerator: 'CmdOrCtrl+C', enabled: has, action: () => clip(false) },
        // 붙여넣기는 macOS 기본 항목 — 웹뷰에 paste 를 보내 숨은 입력 칸의 paste 처리로 들어온다(클립보드 읽기 권한이 필요 없다).
        { item: 'Paste', text: '붙여넣기' },
        { text: '모두 선택', accelerator: 'CmdOrCtrl+A', action: selectAll },
        { item: 'Separator' },
        format('굵게', 'CmdOrCtrl+B', 'bold'),
        format('기울임', 'CmdOrCtrl+I', 'italic'),
        format('밑줄', 'CmdOrCtrl+U', 'underline'),
        format('위첨자', undefined, 'superscript'),
        format('아래첨자', undefined, 'subscript'),
        { item: 'Separator' },
        { text: '되돌리기', accelerator: 'CmdOrCtrl+Z', action: () => void history('undo') },
        { text: '다시 하기', accelerator: 'Shift+CmdOrCtrl+Z', action: () => void history('redo') },
        { item: 'Separator' },
        {
          text: '식별항목',
          enabled: caret !== null && editable,
          items: ITEM_KINDS.map((kind) => ({ text: kind, action: () => void insertItem(kind) })),
        },
        { text: '식별항목 삭제', enabled: editable, action: () => void removeItem() },
        { text: '표', enabled: at !== null, items: at ? tableItems(at) : [] },
        { item: 'Separator' },
        { text: '찾기·바꾸기', accelerator: 'CmdOrCtrl+F', action: button('find') },
      ],
    });
    await menu.popup();
  }

  input.addEventListener('paste', (event) => {
    event.preventDefault();
    const text = event.clipboardData?.getData('text/plain').replace(/\r\n?/g, '\n') ?? '';
    act(() => insertText(text));
  });

  /** 쪽에서 고른 글자에 서식을 건다(고른 것은 그대로 남는다). */
  async function pageFormat(format: CharacterFormatDto, size?: number) {
    const whole = span();
    if (whole && !range() && !size) {
      if (!canEdit()) return;
      const keep = { caret, anchor };
      await structural('글자모양', async () => ({ next: await editorToggleFormatSpans(pieces(whole), format), caret: null }));
      caret = keep.caret;
      anchor = keep.anchor;
      return paint();
    }
    const selection = range();
    if (!caret || !selection) {
      say('서식: 쪽에서 글자를 끌어 고른 뒤 누르세요.');
      return;
    }
    if (!canEdit()) return;
    const id = caret.paragraph;
    const keep = { caret, anchor };
    await structural(size ? '글자 크기' : '글자모양', async () => ({
      next: await (size
        ? editorSetSize(id, selection[0], selection[1], size)
        : editorToggleFormat(id, selection[0], selection[1], format)),
      caret: null,
    }));
    caret = keep.caret;
    anchor = keep.anchor;
    paint();
  }

  /** 되돌리기 · 다시 하기: 친 글을 먼저 보낸다. 커서는 문단이 남아 있으면 그 자리(글 길이 안으로). */
  async function history(which: 'undo' | 'redo') {
    await structural(which === 'undo' ? '되돌리기' : '다시 하기', async () => {
      const next = await (which === 'undo' ? editorUndo() : editorRedo());
      const paragraph = caret && next.paragraphs.find((item) => item.id === caret!.paragraph);
      return {
        next,
        caret: paragraph ? { paragraph: paragraph.id, offset: Math.min(caret!.offset, Array.from(paragraph.text).length) } : null,
      };
    });
  }

  const countLabel = q<HTMLSpanElement>('.find-count');
  const findArgs = (): [string, FindFormatDto | null] => [
    q<HTMLInputElement>('.find-text').value,
    (q<HTMLSelectElement>('.find-format').value || null) as FindFormatDto | null,
  ];
  const findKey = () => JSON.stringify(findArgs());
  const replaceText = () => q<HTMLInputElement>('.replace-text').value;
  const replaceFormat = () => q<HTMLSelectElement>('.replace-format').value as ReplaceFormatDto;

  const FORMAT_LABEL: Record<string, string> = {
    superscript: '위첨자', subscript: '아래첨자', bold: '굵게', italic: '기울임', underline: '밑줄', plain: '서식 없는',
  };

  /** 지금 조건으로 다시 찾는다(판이 바뀌었거나 조건이 바뀌었을 때). */
  async function research() {
    const [text, format] = findArgs();
    found = await editorFind(text, format);
    foundKey = findKey();
    foundView = view;
    foundAt = -1;
  }

  /** 커서 뒤 첫 결과(없으면 처음으로 돌아간다). */
  function nextFrom(at: Caret | null): number {
    if (!at) return 0;
    const here = order(at.paragraph);
    const index = found.findIndex((match) => {
      const there = order(match.paragraph_id);
      return there > here || (there === here && match.start >= at.offset);
    });
    return index < 0 ? 0 : index;
  }

  /** 다음 결과로: 조건이나 판이 바뀌었으면 다시 찾는다. 커서 뒤 첫 결과를 쪽 위에서 고른다. */
  async function findNext() {
    if (!view) return;
    await commit();
    const [text, format] = findArgs();
    if (!text && !format) {
      countLabel.textContent = '찾을 글이나 서식을 정하세요';
      return;
    }
    if (foundKey !== findKey() || foundView !== view) await research();
    if (found.length === 0) {
      const what = [text && `「${text}」`, format && `${FORMAT_LABEL[format]} 글자`].filter(Boolean).join(' · ');
      countLabel.textContent = `${what}: 찾는 것이 없습니다`;
      return;
    }
    foundAt = nextFrom(caret);
    show(found[foundAt]);
  }

  /** 결과를 쪽 위에서 고른다 — 찾기 칸에 포커스를 둔 채(문단 창을 열면 다음 Enter 가 그 창에 줄바꿈으로 들어갔다). */
  function show(match: FindMatchDto) {
    countLabel.textContent = `${foundAt + 1} / ${found.length} · …${match.context}…`;
    const page = view?.paragraph_pages[match.paragraph_id];
    if (page !== undefined) pageHost(page)?.scrollIntoView({ block: 'center' });
    anchor = { paragraph: match.paragraph_id, offset: match.start };
    caret = { paragraph: match.paragraph_id, offset: match.end };
    selected = match.paragraph_id;
    goalX = null;
    reveal = true;
    paint();
  }

  /** 지금 쪽 위에서 고른 것이 그 결과인가(보여 준 것만 바꾼다). */
  const showing = (match: FindMatchDto | undefined) =>
    !!match &&
    foundKey === findKey() &&
    foundView === view &&
    anchor?.paragraph === match.paragraph_id &&
    anchor.offset === match.start &&
    caret?.paragraph === match.paragraph_id &&
    caret.offset === match.end;

  q<HTMLInputElement>('.find-text').addEventListener('keydown', (event) => {
    if (event.key === 'Enter' && !event.isComposing) {
      event.preventDefault();
      void findNext();
    }
  });
  q<HTMLInputElement>('.replace-text').addEventListener('keydown', (event) => {
    if (event.key === 'Enter' && !event.isComposing) {
      event.preventDefault();
      q<HTMLButtonElement>('[data-action="replace-one"]').click();
    }
  });
  q<HTMLElement>('.find-bar').addEventListener('keydown', (event) => {
    if (event.key === 'Escape') {
      q<HTMLElement>('.find-bar').hidden = true;
      input.focus({ preventScroll: true });
    }
  });
  document.addEventListener('keydown', (event) => {
    if ((event.metaKey || event.ctrlKey) && ['t', 'n'].includes(event.key.toLowerCase()) && !event.shiftKey) {
      event.preventDefault();
      void editorNewTab().catch((error) => fail('새 탭', error));
      return;
    }
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 's' && view) {
      event.preventDefault();
      q<HTMLButtonElement>(`[data-action="${event.shiftKey ? 'save-as' : 'save'}"]`).click();
      return;
    }
    if ((event.metaKey || event.ctrlKey) && event.key === 'f' && view) {
      event.preventDefault();
      q<HTMLElement>('.find-bar').hidden = false;
      q<HTMLInputElement>('.find-text').focus();
    }
  });

  /** 글자모양은 고치기 창에서 고른 글자에 건다(문단만 · 아직 적용 안 한 글 고침이 있으면 막는다). */
  function formatTarget(): { id: string; start: number; end: number } | null {
    if (!selected || selectedCell || panel.hidden) {
      say('글자모양: 쪽에서 문단을 누르고, 고치기 창에서 글자를 고른 뒤 누르세요.');
      return null;
    }
    const [start, end] = selectionOffsets(panelText) ?? lastSelection ?? [0, 0];
    if (start === end) {
      say('글자모양을 걸 글자를 고치기 창에서 먼저 고르세요.');
      return null;
    }
    const id = selected;
    if (panelValue() !== view?.paragraphs.find((item) => item.id === id)?.text) {
      say('글을 먼저 「적용」한 뒤 글자모양을 거세요.');
      return null;
    }
    return { id, start, end };
  }

  root.querySelector('.panel-formats')!.addEventListener('click', async (event) => {
    const format = (event.target as HTMLElement).closest<HTMLButtonElement>('button[data-format]')
      ?.dataset.format as CharacterFormatDto | undefined;
    if (!format) return;
    if (panel.hidden) return void pageFormat(format);
    const target = formatTarget();
    if (!target) return;
    await run('글자모양', () => editorToggleFormat(target.id, target.start, target.end, format));
    openPanel(target.id);
    selectOffsets(panelText, target.start, target.end);
  });

  // 도구 막대를 누르면 편집 창 선택이 풀린다 — 마지막 선택을 기억해 둔다.
  document.addEventListener('selectionchange', () => {
    const offsets = selectionOffsets(panelText);
    if (offsets) lastSelection = offsets;
  });

  // Finder 에서 연 파일: 실행 직후 쌓인 것 한 번 · 이후엔 알림마다.
  async function openPending() {
    try {
      const opened = await editorOpenPending();
      if (!opened) return;
      images.clear();
      panel.hidden = true;
      caret = null;
      layout(opened, false);
    } catch (error) {
      fail('열기', error);
    }
  }
  void openPending();
  void listen('open-file', () => void openPending()).catch(() => {});
  // 닫기 · 앱 끝내기: 친 글을 먼저 보내고(고침이 끝날 때까지 기다린다) 바탕에 맡긴다 — 고친 것이 있으면 바탕이 저장을 묻는다.
  const myLabel = getCurrentWindow().label;
  void listen<string>('close-requested', async (event) => {
    if (event.payload !== myLabel) return;
    await commit();
    await queue;
    try {
      await editorClose();
    } catch (error) {
      fail('저장', error);
    }
  }).catch(() => {});

  root.addEventListener('click', async (event) => {
    const action = (event.target as HTMLElement).closest<HTMLButtonElement>('button[data-action]')
      ?.dataset.action;
    // 쪽 위에서 친 글을 먼저 보낸다(저장 · 제출 · 찾기가 옛 글을 보지 않게).
    if (action && action !== 'undo' && action !== 'redo') await commit();
    switch (action) {
      case 'marks': {
        showMarks = !showMarks;
        q<HTMLButtonElement>('[data-action="marks"]').setAttribute('aria-pressed', String(showMarks));
        for (const index of pages.keys()) redraw(index);
        paint();
        break;
      }
      case 'new-tab':
        await editorNewTab().catch((error) => fail('새 탭', error));
        break;
      case 'open': {
        say('여는 중…');
        try {
          const opened = await editorOpen();
          if (opened) {
            images.clear();
            panel.hidden = true;
            layout(opened, false);
          } else say('');
        } catch (error) {
          fail(`열기`, error);
        }
        break;
      }
      case 'new-opinion': {
        say('의견서 만드는 중…');
        try {
          const opened = await editorNewOpinion();
          if (opened) {
            images.clear();
            panel.hidden = true;
            layout(opened, false);
          } else say('');
        } catch (error) {
          fail('새 의견서', error);
        }
        break;
      }
      case 'import': {
        say('HLT 만드는 중…');
        try {
          const opened = await editorImport();
          if (opened) {
            images.clear();
            panel.hidden = true;
            layout(opened, false);
          } else say('');
        } catch (error) {
          fail('HLT 만들기', error);
        }
        break;
      }
      case 'save':
      case 'save-as': {
        try {
          const path = await (action === 'save' ? editorSave() : editorSaveAs());
          if (path) say(`저장함: ${path}`);
        } catch (error) {
          fail(`저장`, error);
        }
        break;
      }
      case 'close-document': {
        try {
          if (await editorCloseDocument()) clearDocument();
        } catch (error) {
          fail('문서 닫기', error);
        }
        break;
      }
      case 'pdf': {
        try {
          const path = await editorPrintPdf();
          if (path) say(`PDF: ${path}`);
        } catch (error) {
          fail(`PDF`, error);
        }
        break;
      }
      case 'fin': {
        say('제출파일 만드는 중…');
        try {
          const path = await editorExportFin();
          say(path ? `제출파일: ${path}` : '');
        } catch (error) {
          fail('제출파일', error);
        }
        break;
      }
      case 'amend': {
        // 통특기와 같게: 최종본에서만 · 고친 것이 없으면 「변경된 내용이 없습니다.」 · 끝나면 읽기 전용 보정서.
        say('보정서 만드는 중…');
        try {
          const done = await editorExportAmendment();
          if (done) {
            say(`보정서(${done.order}차) 생성작업을 완료하였습니다. ${done.dta} · 최종본 갱신 ${done.fin}`);
            showAmendment(done);
          } else say('');
        } catch (error) {
          fail('보정서', error);
        }
        break;
      }
      case 'undo':
      case 'redo':
        await history(action);
        break;
      case 'align-justified':
      case 'align-center':
      case 'align-left': {
        if (!caret || !canEdit()) {
          say('정렬: 쪽에서 문단을 먼저 누르세요.');
          break;
        }
        const at = caret;
        const align = action.slice('align-'.length) as Alignment;
        await structural('정렬', async () => ({ next: await editorSetAlignment(at.paragraph, align), caret: at }));
        break;
      }
      case 'apply':
        if (selectedCell) {
          const cell = selectedCell;
          await run('칸 고치기', () => editorReplaceCellText(cell, panelValue()));
          panel.hidden = true;
        } else if (selected) {
          const id = selected;
          await run('고치기', () => editorReplaceText(id, panelValue()));
          panel.hidden = true;
        }
        break;
      case 'math':
        openFigure('math');
        break;
      case 'chem':
        openFigure('chemistry');
        break;
      case 'table':
        void insertTable();
        break;
      case 'find': {
        const bar = q<HTMLElement>('.find-bar');
        bar.hidden = !bar.hidden;
        if (!bar.hidden) q<HTMLInputElement>('.find-text').focus();
        break;
      }
      case 'find-close':
        q<HTMLElement>('.find-bar').hidden = true;
        break;
      case 'find-next':
        await findNext();
        break;
      case 'replace-one': {
        // 쪽 위에 보여 준 결과만 바꾼다 — 아직 안 보여 줬으면(조건 · 판이 바뀌었으면) 먼저 찾아 보여 준다.
        const current = found[foundAt];
        if (!showing(current)) {
          await findNext();
          break;
        }
        const replacement = replaceText();
        const format = replaceFormat();
        await run('바꾸기', () => editorReplace([current!], replacement, format));
        // 서식만 바꾸면(바꿀 글 비움) 글 길이 그대로.
        const length = replacement === '' && format !== 'keep' ? current!.end - current!.start : Array.from(replacement).length;
        caret = anchor = { paragraph: current!.paragraph_id, offset: current!.start + length };
        await findNext();
        break;
      }
      case 'replace-all': {
        const all = await editorFind(...findArgs());
        if (all.length === 0) {
          countLabel.textContent = '찾는 것이 없습니다';
          break;
        }
        await run(`모두 바꾸기(${all.length}곳)`, () => editorReplace(all, replaceText(), replaceFormat()));
        foundKey = '';
        foundAt = -1;
        anchor = caret;
        paint();
        countLabel.textContent = `${all.length}곳 바꿈`;
        break;
      }
      case 'figure-draw': {
        // 구조식 그리기 판(JSME): 그리면 Molfile 이 입력 칸으로 들어가 RDKit 으로 다시 그린다(출력 모양은 SMILES 입력과 같다).
        drawer.hidden = false;
        if (!drawerOpen) {
          drawerOpen = true;
          try {
            await openStructureDrawer(drawer, (molfile) => {
              figureInput.value = molfile;
              void preview();
            });
          } catch (error) {
            figureError.textContent = String(error instanceof Error ? error.message : error);
          }
        }
        break;
      }
      case 'figure-cancel':
        dialog.hidden = true;
        break;
      case 'figure-insert': {
        if (!figure || !selected) break;
        const width = Number(figureWidth.value);
        const height = Number(figureHeight.value);
        const after = selected;
        const kind = figureKind;
        const svg = figure.svg;
        dialog.hidden = true;
        await run(kind === 'math' ? '수식 넣기' : '화학식 넣기', async () =>
          editorInsertFigure(after, kind, await rasterize(svg, width, height), width, height),
        );
        break;
      }
      case 'close':
        panel.hidden = true;
        selected = null;
        break;
    }
  });
}

/** 문단부호 ↵ (pt): 줄 윗변에서 글자 바닥선까지(12pt 바탕체) · 그림 크기. */
const BASELINE = 9.8;
const MARK_WIDTH = 7;
const MARK_HEIGHT = 6;
const PARAGRAPH_MARK =
  '<svg viewBox="0 0 7 6" width="100%" height="100%" aria-hidden="true"><path d="M6.2 0.4V4.6H1.2M2.8 3.1 1.1 4.6 2.8 6" fill="none" stroke="currentColor" stroke-width="0.8" stroke-linecap="round" stroke-linejoin="round"/></svg>';

/** 구조 창 책 아이콘(통특기 트리의 청록 책). */
const BOOK_ICON =
  '<svg class="tree-icon" viewBox="0 0 16 16" aria-hidden="true"><path d="M2 3.5 8 2l6 1.5v9L8 14l-6-1.5z" fill="#4f9aa8"/><path d="M8 2v12" stroke="#2c6470" stroke-width="1"/><path d="M3.5 4.6 7 3.8v8.3l-3.5.8z" fill="#e8f3f5"/><path d="M12.5 4.6 9 3.8v8.3l3.5.8z" fill="#d3e8ec"/></svg>';

/** 통특기 우클릭 「식별항목」 종류(NKEditor.exe 리소스 차례). */
const ITEM_KINDS = ['도', '표', '청구항', '수학식', '화학식', '반응식', '참고도'];
const ITEM_HEADING = new RegExp(`^【(${ITEM_KINDS.join('|')}) [A-Za-z0-9]+】$`);

/** 「3, 5-7」 → ['3', '5', '6', '7'] · 틀리면 null. 범위는 숫자만(번호 1000 개까지). */
export function parseNumbers(text: string): string[] | null {
  const out: string[] = [];
  for (const part of text.split(',').map((piece) => piece.trim()).filter(Boolean)) {
    const range = part.match(/^(\d+)\s*-\s*(\d+)$/);
    if (range) {
      const [from, to] = [Number(range[1]), Number(range[2])];
      if (to < from || to - from > 1000) return null;
      for (let n = from; n <= to; n += 1) out.push(String(n));
    } else if (/^[A-Za-z0-9]+$/.test(part)) out.push(part);
    else return null;
  }
  return out.length ? out : null;
}

function escapeHtml(text: string): string {
  return text.replace(/[&<>"]/g, (ch) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[ch]!);
}

function describe(error: unknown): string {
  if (typeof error === 'object' && error !== null && 'phase' in error) {
    const value = error as { code?: string; phase?: string; details?: Record<string, string> };
    const reason = value.details?.reason;
    const plain: Record<string, string> = {
      heading_paragraph: '표제(【 】) 문단이 걸려 있어 할 수 없습니다',
      table_paragraph: '표 문단이 걸려 있어 할 수 없습니다',
      'table_skipped:last_row': '표의 마지막 줄은 지울 수 없습니다(표를 없애려면 「표 지우기」)',
      'table_skipped:last_column': '표의 마지막 칸은 지울 수 없습니다(표를 없애려면 「표 지우기」)',
      'table_skipped:cell_contains_image': '그림이 든 칸은 지울 수 없습니다',
      'table_skipped:merge_crosses_boundary': '다른 합친 칸에 걸쳐 있어 합칠 수 없습니다',
      'table_skipped:not_merged': '합친 칸이 아닙니다',
      'table_skipped:size_limit': '표는 200줄 · 40칸까지입니다',
      'table_skipped:index_out_of_range': '표 밖의 자리입니다',
      'table_skipped:no_change': '바뀐 것이 없습니다',
      amendment_empty: '변경된 내용이 없습니다',
      amendment_needs_final: '최종본(.fin)으로 연 문서에서만 보정서를 만듭니다(열기에서 .fin 을 고르세요)',
      item_missing: '그 번호의 식별항목이 없습니다',
    };
    if (reason && plain[reason]) return plain[reason];
    if (value.code === 'cp949_invalid' && value.details?.char) {
      const code = value.details.char;
      const ch = String.fromCodePoint(parseInt(code.slice(2), 16));
      return `「${ch}」(${code})는 통특기 문자 집합(KS X 1001 · CP949)에 없는 글자라 넣을 수 없습니다`;
    }
    return `${value.code ?? ''} · ${value.phase ?? ''}${reason ? ` · ${reason}` : ''}`;
  }
  return String(error);
}
