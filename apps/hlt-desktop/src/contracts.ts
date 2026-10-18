import { invoke } from '@tauri-apps/api/core';

export type U64 = string;

export interface SelectedFileDto {
  file_handle: string;
  display_name: string;
}

export interface VerifyRequestDto {
  request_id: string;
  file_handle: string;
}

export interface ProgressEventDto {
  request_id: string;
  phase: string;
  compressed_bytes: U64;
  inflated_bytes: U64;
  items: U64;
  fraction: number;
}

export interface FirstBlockDto {
  final_block: boolean;
  kind: 'stored' | 'fixed' | 'dynamic';
  hlit: number | null;
  hdist: number | null;
  hclen: number | null;
  window_ok: boolean;
}

export interface VerifyResultDto {
  source: string;
  file_size: U64;
  input_sha256: string;
  header_ok: boolean;
  magic_ok: boolean;
  header_len: U64;
  stream_start: U64;
  stream_end: U64;
  footer_offset: U64;
  footer_crc: U64;
  computed_crc: U64;
  footer_size: U64;
  computed_size: U64;
  footer_crc_ok: boolean;
  footer_size_ok: boolean;
  inflate_ok: boolean;
  inflated: U64;
  compressed_bytes: U64;
  window_ok: boolean;
  first_block: FirstBlockDto;
}

export interface CoreErrorDto {
  code: string;
  message: string;
  phase: string;
  source: string | null;
  offset: U64 | null;
  expected: string | null;
  actual: string | null;
  retryable: boolean;
  details: Record<string, string>;
}

export function selectHlt(): Promise<SelectedFileDto | null> {
  return invoke<SelectedFileDto | null>('select_hlt');
}

export function verifyFile(request: VerifyRequestDto): Promise<VerifyResultDto> {
  return invoke<VerifyResultDto>('verify_file', { request });
}

export function cancelVerify(requestId: string): Promise<void> {
  return invoke<void>('cancel_verify', { requestId });
}

// ── 편집 화면(P5) ─────────────────────────────────────────────

export interface PlannedGlyphDto {
  ch: string;
  x: number;
  size: number;
  /** 밑줄 1 · 굵게 2 · 기울임 4 · 위첨자 16 · 아래첨자 32 */
  style: number;
  drop: number;
}

export interface PlannedLineDto {
  top: number;
  glyphs: PlannedGlyphDto[];
  paragraph: string | null;
  cell?: string;
  /** 커서용: 이 줄 첫 글자의 문단 글 안 자리 · 글자 경계 x(글자 수 + 1). 표 칸 줄 · 쪽번호엔 없다. */
  start?: number;
  stops?: number[];
  /** 밑줄 구간 [x 시작, x 끝](pt) — 공백까지 잇는다. */
  underlines?: [number, number][];
}

export interface PlannedImageDto {
  object_code: number;
  x: number;
  top: number;
  width: number;
  height: number;
}

/** 표 칸 테두리 상자(pt · 빈 칸 포함). */
export interface PlannedCellBoxDto {
  cell: string;
  x: number;
  top: number;
  width: number;
  height: number;
}

export interface PlannedPageDto {
  number: number;
  lines: PlannedLineDto[];
  images: PlannedImageDto[];
  cells?: PlannedCellBoxDto[];
}

export interface EditorParagraphDto {
  id: string;
  text: string;
  wipo_id: number | null;
  /** 식별번호(최종본 .fin 을 연 문서만 · 통특기는 문단 왼쪽에 [0001]). */
  number?: string | null;
  has_objects: boolean;
  spans: { start: number; end: number; style: number; size?: number }[];
}

export interface EditorViewDto {
  title: string;
  editable: boolean;
  page_count: number;
  page_width: number;
  page_height: number;
  paragraphs: EditorParagraphDto[];
  paragraph_pages: Record<string, number>;
  preparing: boolean;
  /** 표 칸 글(칸 id → 글 · 그림 `_`). 모델 준비 전이면 비어 있다(칸마다 `editorCellText`). */
  cells?: Record<string, string>;
}

/** 열기: 이 탭에 문서가 있으면 새 탭에서 열고 `null`(이 탭은 그대로). */
export const editorOpen = () => invoke<EditorViewDto | null>('editor_open');
/** 불러와 HLT 만들기: 워드 · XML · HML → 새 `.HLT` 를 굽고 연다(이 탭에 문서가 있으면 새 탭 · `null`). */
export const editorImport = () => invoke<EditorViewDto | null>('editor_import');
/** 새 의견서: 빈 문서 + 【의견내용】 을 만들고 연다(이 탭에 문서가 있으면 새 탭 · `null`). */
export const editorNewOpinion = () => invoke<EditorViewDto | null>('editor_new_opinion');
export const editorNewTab = () => invoke<void>('editor_new_tab');
/** 닫기: 고친 것이 있으면 저장을 묻고 창을 닫는다. `false` = 취소. */
export const editorClose = () => invoke<boolean>('editor_close');
/** Finder 가 넘긴 파일(더블클릭)을 연다 — 없으면 null. */
export const editorOpenPending = () => invoke<EditorViewDto | null>('editor_open_pending');
export const editorPage = (index: number) => invoke<PlannedPageDto>('editor_page', { index });
export const editorImage = (objectCode: number) =>
  invoke<ArrayBuffer>('editor_image', { objectCode });
export const editorReplaceText = (paragraphId: string, text: string) =>
  invoke<EditorViewDto>('editor_replace_text', { paragraphId, text });
/** 강제 쪽나눔(⌘/Ctrl+Enter): 글에 `\n` 하나 — 그 뒤 조각이 새 쪽에서 시작한다. */
export const editorReplaceTextPageBreak = (paragraphId: string, text: string) =>
  invoke<EditorViewDto>('editor_replace_text_page_break', { paragraphId, text });
export type Alignment = 'justified' | 'center' | 'left';
export const editorSetAlignment = (paragraphId: string, align: Alignment) =>
  invoke<EditorViewDto>('editor_set_alignment', { paragraphId, align });
/** 본문 첫 줄 들여쓰기 값(실물 본문 문단 = 800 · 통특기 단추는 799 를 쓴다 — 화면 차이 없음). */
export const BODY_INDENT = 800;
export const editorSetIndent = (paragraphId: string, indent: number) =>
  invoke<EditorViewDto>('editor_set_indent', { paragraphId, indent });
/** 편집기 기록 파일(앱 기록 폴더의 editor.log)에 한 줄. */
export const editorLog = (line: string) => invoke<void>('editor_log', { line });
export const editorMergeParagraph = (paragraphId: string) =>
  invoke<EditorViewDto>('editor_merge_paragraph', { paragraphId });
export const editorUndo = () => invoke<EditorViewDto>('editor_undo');
export const editorRedo = () => invoke<EditorViewDto>('editor_redo');
export const editorNumberParagraphs = () => invoke<EditorViewDto>('editor_number_paragraphs');
export const editorSaveAs = () => invoke<string | null>('editor_save_as');
/** 저장: 연 HLT 에 덮어쓴다(제출파일 · 새 문서면 다른 이름으로 묻는다) — 저장한 경로 또는 취소면 null. */
export const editorSave = () => invoke<string | null>('editor_save');
/** 문서 닫기: 고친 것이 있으면 저장을 묻는다. `false` = 취소. */
export const editorCloseDocument = () => invoke<boolean>('editor_close_document');
export const editorPrintPdf = () => invoke<string | null>('editor_print_pdf');
export const editorInsertFigure = (
  after: string,
  kind: 'math' | 'chemistry',
  pngBase64: string,
  widthPt: number,
  heightPt: number,
) => invoke<EditorViewDto>('editor_insert_figure', { after, kind, pngBase64, widthPt, heightPt });
export type CharacterFormatDto = 'bold' | 'italic' | 'underline' | 'superscript' | 'subscript';
export const editorToggleFormat = (paragraphId: string, start: number, end: number, format: CharacterFormatDto) =>
  invoke<EditorViewDto>('editor_toggle_format', { paragraphId, start, end, format });
export const editorToggleFormatSpans = (spans: [string, number, number][], format: CharacterFormatDto) =>
  invoke<EditorViewDto>('editor_toggle_format_spans', { spans, format });
export const editorDeleteRange = (fromId: string, fromOffset: number, toId: string, toOffset: number) =>
  invoke<EditorViewDto>('editor_delete_range', { fromId, fromOffset, toId, toOffset });
export const editorInsertItem = (paragraphId: string, before: boolean, kind: string, number: string) =>
  invoke<{ view: EditorViewDto; body: string }>('editor_insert_item', { paragraphId, before, kind, number });
export const editorRemoveItems = (kind: string, numbers: string[]) =>
  invoke<EditorViewDto>('editor_remove_items', { kind, numbers });
export const editorCellText = (cellId: string) => invoke<string>('editor_cell_text', { cellId });
export const editorReplaceCellText = (cellId: string, text: string) =>
  invoke<EditorViewDto>('editor_replace_cell_text', { cellId, text });
/** 표 편집 연산(hlt-core `TableOperation` · 줄 · 칸 번호는 0 부터 · 합치기 끝은 포함). */
export type TableOperationDto =
  | { kind: 'insert_row' | 'delete_row' | 'insert_column' | 'delete_column'; table_id: string; index: number }
  | { kind: 'merge'; table_id: string; row: number; column: number; row_end: number; column_end: number }
  | { kind: 'split'; table_id: string; row: number; column: number };
export interface CellPositionDto {
  table_id: string;
  row: number;
  column: number;
  rowspan: number;
  colspan: number;
  rows: number;
  columns: number;
}
export const editorInsertTable = (after: string, rows: number, columns: number) =>
  invoke<{ view: EditorViewDto; cell: string }>('editor_insert_table', { after, rows, columns });
export const editorTableEdit = (operation: TableOperationDto) =>
  invoke<EditorViewDto>('editor_table_edit', { operation });
export const editorDeleteTable = (tableId: string) => invoke<EditorViewDto>('editor_delete_table', { tableId });
export const editorCellPosition = (cellId: string) =>
  invoke<CellPositionDto | null>('editor_cell_position', { cellId });
export const editorExportFin = () => invoke<string | null>('editor_export_fin');
export interface AmendItemDto {
  target: string;
  method: string;
  content: string;
}
export interface AmendmentDoneDto {
  dta: string;
  fin: string;
  order: number;
  items: AmendItemDto[];
}
export const editorExportAmendment = () => invoke<AmendmentDoneDto | null>('editor_export_amendment');
export const editorSetSize = (paragraphId: string, start: number, end: number, size: number) =>
  invoke<EditorViewDto>('editor_set_size', { paragraphId, start, end, size });

export type FindFormatDto = 'bold' | 'italic' | 'underline' | 'superscript' | 'subscript' | 'plain';
export type ReplaceFormatDto =
  | 'keep' | 'superscript' | 'subscript' | 'normal' | 'bold' | 'italic' | 'underline' | 'clear';
export interface FindMatchDto {
  paragraph_id: string;
  start: number;
  end: number;
  context: string;
}
export const editorFind = (text: string, format: FindFormatDto | null) =>
  invoke<FindMatchDto[]>('editor_find', { query: { text, format } });
export const editorReplace = (matches: FindMatchDto[], replacement: string, format: ReplaceFormatDto) =>
  invoke<EditorViewDto>('editor_replace', { matches, replacement, format });
export const editorView = () => invoke<EditorViewDto>('editor_view');
