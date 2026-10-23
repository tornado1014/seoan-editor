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
  /** 판독 그림 id — 크기 바꾸기가 가리킨다. */
  image: string;
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
  /** 방금 고침에서 CP949 에 없어 바꾼 글자들(붙여넣기 등). */
  cp949_substitutions?: { from: string; to: string; count: number }[];
}

/** 열기: 이 탭에 문서가 있으면 새 탭에서 열고 `null`(이 탭은 그대로). */
export const editorOpen = () => invoke<EditorViewDto | null>('editor_open');
/** 불러와 HLT 만들기: 워드 · XML · HML → 새 `.HLT` 를 굽고 연다(이 탭에 문서가 있으면 새 탭 · `null`). */
/** CP949(HLT)에 없는 글자 한 종류 — 확인 표 한 줄. `suggestion` 이 `null` 이면 대응표에 없다. */
export interface Cp949FindingDto {
  from: string;
  code: string;
  suggestion: string | null;
  count: number;
  context: string;
}
/** 불러오기: 연 화면, 또는 CP949 밖 글자 확인 표(`review` — 고른 뒤 `editorImportFinish`). */
export const editorImport = () =>
  invoke<{ view: EditorViewDto | null; review: Cp949FindingDto[] }>('editor_import');
/** 확인 표 뒤: 고른 대응(글자 → 바꿀 글 · 빈 글 = 지움)으로 굽고 연다. `null` = 취소. */
export const editorImportFinish = (choices: Record<string, string> | null) =>
  invoke<EditorViewDto | null>('editor_import_finish', { choices });
/** 글에서 CP949 밖 글자 찾기(바꾸지 않는다). */
export const editorCp949Scan = (text: string) => invoke<Cp949FindingDto[]>('editor_cp949_scan', { text });
/** 새 의견서: 빈 문서 + 【의견내용】 을 만들고 연다(이 탭에 문서가 있으면 새 탭 · `null`). */
/** 새 의견서(`body` = 본문 글 · 줄 하나 = 문단 하나 — 없으면 빈 의견서). */
export const editorNewOpinion = (body?: string) =>
  invoke<EditorViewDto | null>('editor_new_opinion', body === undefined ? undefined : { body });
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
export const editorExportDocx = () => invoke<string | null>('editor_export_docx');
export const editorInsertFigure = (
  after: string,
  kind: 'math' | 'chemistry',
  pngBase64: string,
  widthPt: number,
  heightPt: number,
) => invoke<EditorViewDto>('editor_insert_figure', { after, kind, pngBase64, widthPt, heightPt });
/** 그림 크기 바꾸기(비율 잠금): 판독 그림 id · 배율. */
export const editorResizeImage = (image: string, scale: number) =>
  invoke<EditorViewDto>('editor_resize_image', { image, scale });
/** 그림 파일(JPG · TIFF) 골라 `after` 문단 뒤에 넣기 · 취소하면 그대로인 화면. */
/** `offset` 이 있으면 `after` 는 표 칸 id · 칸 글의 그 자리에 넣는다. */
export const editorInsertPicture = (after: string, offset?: number) =>
  invoke<EditorViewDto>('editor_insert_picture', { after, offset: offset ?? null });
/** 그림 바꾸기(도면 보정): 판독 그림 id 자리에 고른 그림 파일 · 취소하면 그대로인 화면. */
export const editorReplacePicture = (image: string) => invoke<EditorViewDto>('editor_replace_picture', { image });
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
  /** 보정 전후 대비표(.docx · dta 옆 · 못 썼으면 `null`). */
  table: string | null;
}
export const editorExportAmendment = () => invoke<AmendmentDoneDto | null>('editor_export_amendment');
/** OA 검토보고서(docx)에서 읽은 보정안 · 상세설명 오기 후보 · 도면 할 일. */
export type ProposedClaimDto = { number: string; before: string[]; after: string[] };
export type DescriptionNoteDto = { paragraphs: string[]; wrong: string; right: string | null; sentence: string; positional: boolean };
export type ReviewReportDto = {
  proposals: { label: string; claims: ProposedClaimDto[]; tracked: boolean }[];
  notes: DescriptionNoteDto[];
  figures: { number: string; sentence: string }[];
  /** 오기 후보마다 지금 문서에서 걸릴 곳 수. */
  matches: number[];
};
export type ReviewOutcomeDto = {
  claims_changed: string[];
  claims_deleted: string[];
  claims_added: string[];
  claims_failed: [string, string][];
  notes_applied: [number, number][];
};
/** 보고서 고르기(취소면 null). */
export const editorReadReview = () => invoke<ReviewReportDto | null>('editor_read_review');
/** 고른 보정안 청구항 · 고른 오기 정정을 한 번의 고침으로 건다. */
export const editorApplyReview = (claims: ProposedClaimDto[], tracked: boolean, notes: DescriptionNoteDto[]) =>
  invoke<{ view: EditorViewDto; outcome: ReviewOutcomeDto }>('editor_apply_review', { claims, tracked, notes });
/** 보정 표시: 바뀐 문단(지금 글 좌표 — 넣은 [start, end) · 지운 (자리, 글)) · 바뀐/새/지운 요소 이름. */
export type ParagraphMarkDto = { id: string; inserted: [number, number][]; deleted: [number, string][] };
export type AmendmentMarksDto = {
  paragraphs: ParagraphMarkDto[];
  changed: string[];
  added: string[];
  removed: string[];
};
/** `original` = 원출원(fin 안 보정서 적용 전)에 견준다 · `file` = 「다른 파일…」로 고른 파일에 · 아니면 연 상태(연 뒤 고친 것). */
export const editorAmendmentMarks = (original = false, file = false) =>
  invoke<AmendmentMarksDto | null>('editor_amendment_marks', { original, file });
/** 보정 표시 기준 「다른 파일…」: 견줄 파일을 고른다(고른 파일 이름 · 취소 = `null`). */
export const editorComparePick = () => invoke<string | null>('editor_compare_pick');
/** 자동 저장(고친 것이 있으면 복구본을 두고 없으면 지운다). */
export const editorAutosave = () => invoke<void>('editor_autosave');
/** 일괄 처리(폴더를 고르는 것부터 · 취소 = `null`). 진행은 `batch-progress` 사건. */
export type BatchRowDto = { file: string; status: string; detail: string };
export type BatchCheckRowDto = { file: string; counts: Partial<Record<SubmitKindDto, number>>; error: string | null };
export const editorBatchGenerate = () => invoke<BatchRowDto[] | null>('editor_batch_generate');
export const editorBatchCheck = () => invoke<BatchCheckRowDto[] | null>('editor_batch_check');
export const editorBatchStop = () => invoke<void>('editor_batch_stop');
/** 표를 CSV 로 저장(이름을 묻는다 · 취소 = `null`). */
/** 원문과 나란히 보기: 원문 문단 + 짝 이름(`p:0001` · `claim:3`). */
export type SourceParagraphDto = { key: string | null; text: string };
export const editorSourceOpen = () => invoke<SourceParagraphDto[] | null>('editor_source_open');
/** 지금 판 문단마다 원문 짝 이름(화면 문단 차례). */
export const editorParagraphKeys = () => invoke<(string | null)[]>('editor_paragraph_keys');
export const editorSaveCsv = (name: string, text: string) => invoke<string | null>('editor_save_csv', { name, text });
/** 청구항 인용 문제. 앞 넷은 특허로 점검 페이지와 같은 판정 · 뒤 셋은 앱만 보는 규칙. */
export type CitationKindDto =
  | 'mis_reference'
  | 'removed'
  | 'duplicate_multiple'
  | 'no_alternative'
  | 'range_mis_reference'
  | 'and_citation'
  | 'range_no_alternative';
export type CitationIssueDto = { claim: number; kind: CitationKindDto; cited: number[] };
export const editorClaimIssues = () => invoke<CitationIssueDto[]>('editor_claim_issues');
/** 청구항 나무: 항 번호 → 인용하는 앞 항(빈 = 독립항). */
export const editorClaimParents = () => invoke<Record<string, number[]>>('editor_claim_parents');
export type SubmitKindDto = 'unnumbered' | 'figure_missing' | 'figure_unmentioned' | 'sign_unused' | 'sign_names' | 'sign_shared' | 'citation';
/** 제출 전 점검 한 줄(가 볼 문단 · 없으면 문서 전체의 일). */
export type SubmitIssueDto = { kind: SubmitKindDto; paragraph_id: string | null; detail: string };
export const editorSubmissionChecks = () => invoke<SubmitIssueDto[]>('editor_submission_checks');
/** 보정 근거 후보: 보정 전 명세서 식별번호 문단 · 점수(통째로면 1) · 인용 문장 · 그 문단의 도면. */
export type BasisHitDto = { number: string; score: number; exact: boolean; quote: string; figures: string[] };
export type ClaimBasisDto = { claim: number; added: boolean; phrases: { text: string; hits: BasisHitDto[] }[] };
export const editorAmendmentBasis = (original = false) =>
  invoke<ClaimBasisDto[] | null>('editor_amendment_basis', { original });
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
