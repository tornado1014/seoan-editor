// 편집 화면(P5): 도구 막대 · 구조 창 · 쪽 화면 · 문단 고치기 창.
// 쪽은 보일 때 한 쪽씩 받아 그린다. 고치면 문서 전체가 다시 구워지고 보이는 쪽을 다시 받는다.
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { Menu } from '@tauri-apps/api/menu';
import {
  editorImage,
  editorHistoryList,
  editorHistoryOpen,
  editorOpenReadOnly,
  settingsGet,
  settingsSet,
  type SettingsDto,
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
  editorReadReview,
  editorAmendmentBasis,
  editorAmendmentMarks,
  editorComparePick,
  editorAutosave,
  editorBatchCheck,
  editorBatchGenerate,
  editorBatchStop,
  editorSaveCsv,
  editorRenumberPlan,
  editorSignTable,
  editorFixDoubleSpaces,
  editorReplaceTexts,
  editorSourceOpen,
  editorParagraphKeys,
  editorApplyReview,
  editorClaimIssues,
  editorClaimParents,
  editorSubmissionChecks,
  editorExportFin,
  editorFind,
  editorReplace,
  type FindFormatDto,
  type FindOptionsDto,
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
  editorImportFinish,
  editorCp949Scan,
  type Cp949FindingDto,
  editorNewOpinion,
  editorNewTab,
  editorInsertItem,
  editorInsertPicture,
  editorResizeImage,
  editorReplacePicture,
  editorRemoveItems,
  editorClose,
  editorOpenPending,
  editorPage,
  editorPrintPdf,
  editorExportDocx,
  editorRedo,
  editorReplaceText,
  editorReplaceTextPageBreak,
  editorSetAlignment,
  editorSetIndent,
  BODY_INDENT,
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
  type AmendmentMarksDto,
  type CitationIssueDto,
  type SubmitIssueDto,
  type SourceParagraphDto,
  type ClaimBasisDto,
  type ParagraphMarkDto,
  type DescriptionNoteDto,
  type ProposedClaimDto,
  type ReviewOutcomeDto,
  type ReviewReportDto,
} from './contracts';
import { documentStats, nestClaims, outlineTree, repeatedSentences, renderPage, richParagraph, sectionOf, selectOffsets, selectionOffsets } from './editor-view';
import type { OutlineNode } from './editor-view';
import { openHelp as openTourList, runTour, TOURS } from './tour';
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
  isCell,
} from './page-caret';
import {
  type Figure,
  openStructureDrawer,
  rasterize,
  renderChemistry,
  renderLatex,
} from './figure-editor';

const SCALE = 1.3;
/** 제출 전 점검 종류 → 묶음 이름(점검 창 · 일괄 점검 표의 열). */
const SUBMIT_GROUPS: [SubmitIssueDto['kind'], string][] = [
  ['unnumbered', '빈 식별번호'],
  ['figure_missing', '도면이 없는 「도 N」'],
  ['figure_unmentioned', '본문이 말하지 않는 도면'],
  ['sign_unused', '본문에 안 나오는 부호'],
  ['sign_names', '같은 부호 · 다른 이름(괄호 안 = 쓰인 수)'],
  ['sign_shared', '같은 이름 · 다른 부호(한두 번만 다른 부호)'],
  ['citation', '청구항 인용'],
  ['claim_ending', '마침표로 끝나지 않는 청구항'],
  ['duplicate_heading', '두 번 나온 청구항 · 도면 번호'],
  ['empty_heading', '본문이 없는 표제'],
  ['representative', '대표도'],
  ['abstract_long', '요약 길이(권장 400자)'],
  ['spacing', '띄어쓰기(공백 두 칸 · 문장부호 앞 · 문단 끝 · 전각 공백)'],
];

/** 표 → CSV(칸마다 따옴표 · 안의 따옴표는 둘로). */
export function toCsv(rows: string[][]): string {
  return rows.map((row) => row.map((cell) => `"${cell.replace(/"/g, '""')}"`).join(',')).join('\r\n');
}

/** 단축키 모음(도움말). */
const SHORTCUTS: [string, string][] = [
  ['⌘O', '열기'],
  ['⌘T · ⌘N', '새 탭'],
  ['⌘S · ⇧⌘S', '저장 · 다른 이름으로 저장'],
  ['⌘W', '탭 닫기'],
  ['⌘Z · ⇧⌘Z', '되돌리기 · 다시 하기'],
  ['⌘F', '찾기 · 바꾸기'],
  ['⌘G', '식별번호 · 청구항 · 도면 · 표제로 가기'],
  ['⌘K', '명령 찾기(이름을 쳐서 실행)'],
  ['⌘] · ⌘[', '다음 · 앞 고친 곳'],
  ['⌘+ · ⌘− · ⌘0', '문서 확대 · 축소 · 실제 크기(툴바는 그대로)'],
  ['⌘,', '설정'],
  ['⌘B · ⌘I · ⌘U', '굵게 · 기울임 · 밑줄'],
  ['⌘Enter', '강제 쪽나눔'],
  ['Tab · ⇧Tab', '표 칸 이동 · 본문은 탭 넣기 · 들여쓰기 취소'],
  ['⌘A · ⌘C · ⌘X · ⌘V', '모두 선택 · 복사 · 잘라내기 · 붙여넣기'],
  ['Esc', '창 · 찾기 막대 · 투어 닫기'],
];

/** 처음 실행 안내를 본 적이 있는지(브라우저 저장 — 못 쓰면 매번 묻지 않고 넘어간다). */
const TOUR_SEEN = 'seoan.tour.seen';

/** 자동 저장: 마지막 고침 뒤 이만큼 손을 멈추면 복구본을 둔다(설정 「자동 저장 간격」의 기본값). */
export const AUTOSAVE_MS = 120_000;

/** 문서 배율 한 걸음 · 범위(50~200%). */
export function nextZoom(zoom: number, step: -1 | 0 | 1): number {
  if (step === 0) return 1;
  return Math.min(2, Math.max(0.5, Math.round((zoom + step * 0.1) * 10) / 10));
}

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
  indent: ['M21 6H11', 'M21 12H11', 'M21 18H3', 'm3 8 4 4-4 4'],
  outdent: ['M21 6H11', 'M21 12H11', 'M21 18H3', 'm7 8-4 4 4 4'],
  math: ['M18 7V4H6l6 8-6 8h12v-3'],
  chem: ['M10 2v7.527a2 2 0 0 1-.211.896L4.72 20.55a1 1 0 0 0 .9 1.45h12.76a1 1 0 0 0 .9-1.45l-5.069-10.127A2 2 0 0 1 14 9.527V2', 'M8.5 2h7', 'M7 16h10'],
  table: ['M3 3h18v18H3z', 'M3 9h18', 'M3 15h18', 'M9 3v18', 'M15 3v18'],
  marks: ['M13 4v16', 'M17 4v16', 'M19 4H9.5a4.5 4.5 0 0 0 0 9H13'],
  print: ['M6 18H4a2 2 0 0 1-2-2v-5a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2v5a2 2 0 0 1-2 2h-2', 'M6 9V3a1 1 0 0 1 1-1h10a1 1 0 0 1 1 1v6', 'M6 14h12v8H6z'],
  send: ['m22 2-7 20-4-9-9-4Z', 'M22 2 11 13'],
  amend: ['M12.5 22H18a2 2 0 0 0 2-2V7l-5-5H6a2 2 0 0 0-2 2v9.5', 'M14 2v4a2 2 0 0 0 2 2h4', 'M13.378 15.626a1 1 0 1 0-3.004-3.004l-5.01 5.012a2 2 0 0 0-.506.854l-.837 2.87a.5.5 0 0 0 .62.62l2.87-.837a2 2 0 0 0 .854-.506z'],
  fileNew: ['M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z', 'M14 2v4a2 2 0 0 0 2 2h4', 'M9 15h6', 'M12 18v-6'],
  columns: ['M5 3h14a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z', 'M12 3v18'],
  wrench: ['M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z'],
  layers: ['m12.83 2.18a2 2 0 0 0-1.66 0L2.6 6.08a1 1 0 0 0 0 1.83l8.58 3.91a2 2 0 0 0 1.66 0l8.58-3.9a1 1 0 0 0 0-1.83Z', 'm22 17.65-9.17 4.16a2 2 0 0 1-1.66 0L2 17.65', 'm22 12.65-9.17 4.16a2 2 0 0 1-1.66 0L2 12.65'],
  diff: ['M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z', 'M9 10h6', 'M12 13V7', 'M9 17h6'],
  evidence: ['M14 2v4a2 2 0 0 0 2 2h4', 'M4.27 21a2 2 0 0 0 1.73 1H18a2 2 0 0 0 2-2V7l-5-5H6a2 2 0 0 0-2 2v3', 'm9 18-1.5-1.5', 'M5 17a3 3 0 1 0 0-6 3 3 0 0 0 0 6z'],
  word: ['M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z', 'M14 2v4a2 2 0 0 0 2 2h4', 'M16 13H8', 'M16 17H8', 'M10 9H8'],
  help: ['M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20z', 'M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3', 'M12 17h.01'],
  snippet: ['M17 6H3', 'M21 12H8', 'M21 18H8', 'M3 12v6'],
  zoomIn: ['M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16z', 'm21 21-4.3-4.3', 'M11 8v6', 'M8 11h6'],
  zoomOut: ['M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16z', 'm21 21-4.3-4.3', 'M8 11h6'],
};
const icon = (name: string) =>
  `<svg class="tb-icon" viewBox="0 0 24 24" aria-hidden="true">${ICONS[name].map((d) => `<path d="${d}"/>`).join('')}</svg>`;

export function mountEditor(root: HTMLElement): void {
  /** 쪽 그리기 배율(pt → 화면 px) — 문서 배율(설정 · ⌘+ ⌘− ⌘0)이 곱해진다. 툴바 · 개요는 그대로. */
  let scale = SCALE;
  root.innerHTML = `
    <div class="editor">
      <div class="editor-toolbar" role="toolbar">
        <div class="tb-group" role="group" aria-label="파일">
          <button type="button" data-action="open" title="열기 (⌘O) — HLT · 제출파일(.fin · .hlz)을 연다 · 최근 문서는 메뉴 「파일」">${icon('open')}<span>열기</span></button>
          <button type="button" data-action="new-menu" title="새로 만들기 — 새 의견서 · 워드/XML/HML로 HLT 만들기 · 새 탭(⌘T)">${icon('fileNew')}<span>새로 만들기 ▾</span></button>
          <button type="button" data-action="save" disabled title="저장 — 연 HLT 에 덮어쓰기 (⌘S)">${icon('save')}<span>저장</span></button>
          <button type="button" data-action="save-as" disabled title="다른 이름으로 저장 (⇧⌘S)">${icon('saveAs')}</button>
          <button type="button" data-action="close-document" disabled title="문서 닫기 — 저장하지 않은 고침이 있으면 묻는다">${icon('close')}<span>닫기</span></button>
        </div>
        <div class="tb-hidden" hidden>
          <button type="button" data-action="new-opinion" title="새 의견서 — 빈 의견서(【의견내용】)를 새로 만든다">${icon('opinion')}<span>새 의견서</span></button>
          <button type="button" data-action="import" title="워드/XML로 HLT 만들기 — 워드 · KIPO XML · HML 을 불러와 새 HLT 로 만든다">${icon('import')}<span>워드/XML로 HLT 만들기</span></button>
          <button type="button" data-action="new-tab" title="새 탭 (⌘T · ⌘N)">${icon('tab')}<span>새 탭</span></button>
          <button type="button" data-action="open-read-only" title="읽기 전용으로 열기 — 원본을 실수로 고치지 않게(사본은 「다른 이름으로 저장」)"><span>읽기 전용으로 열기</span></button>
          <button type="button" data-action="history" disabled title="이전 판 — 저장할 때 덮인 판을 골라 새 탭에서 연다(최근 판 · 지난 날수는 설정)"><span>이전 판</span></button>
          <button type="button" data-action="settings" title="설정 (⌘,) — 자동 저장 간격 · 이전 판 보관 · 문서 배율 · 문단부호 기본"><span>설정</span></button>
        </div>
        <div class="tb-group" role="group" aria-label="편집">
          <button type="button" data-action="undo" disabled title="되돌리기 (⌘Z)">${icon('undo')}</button>
          <button type="button" data-action="redo" disabled title="다시 하기 (⇧⌘Z)">${icon('redo')}</button>
          <button type="button" data-action="find" disabled title="찾기 · 바꾸기 (⌘F) — 대소문자 · 낱말 · 정규식 옵션 · 식별번호 · 청구항으로 가기는 ⌘G">${icon('find')}<span>찾기</span></button>
        </div>
        <div class="tb-group panel-formats" role="group" aria-label="글자">
          <button type="button" data-format="bold" title="굵게 (⌘B)"><b>가</b></button>
          <button type="button" data-format="italic" title="기울임 (⌘I)"><i>가</i></button>
          <button type="button" data-format="underline" title="밑줄 (⌘U)"><u>가</u></button>
          <button type="button" data-format="superscript" title="위첨자">x<sup>2</sup></button>
          <button type="button" data-format="subscript" title="아래첨자">x<sub>2</sub></button>
        </div>
        <div class="tb-group panel-aligns" role="group" aria-label="문단">
          <button type="button" data-action="align-justified" disabled title="양쪽 정렬 (커서가 있는 문단)">${icon('justify')}</button>
          <button type="button" data-action="align-center" disabled title="가운데 정렬 (커서가 있는 문단)">${icon('center')}</button>
          <button type="button" data-action="align-left" disabled title="왼쪽 정렬 (커서가 있는 문단)">${icon('left')}</button>
          <button type="button" data-action="indent" disabled title="들여쓰기 — 첫 줄 들여쓰기 (커서가 있는 문단)">${icon('indent')}</button>
          <button type="button" data-action="outdent" disabled title="들여쓰기 취소 (커서가 있는 문단 · ⇧Tab)">${icon('outdent')}</button>
        </div>
        <div class="tb-group" role="group" aria-label="넣기">
          <button type="button" data-action="math" disabled title="수식 넣기(LaTeX)">${icon('math')}<span>수식</span></button>
          <button type="button" data-action="chem" disabled title="화학식 넣기(구조식)">${icon('chem')}<span>화학식</span></button>
          <button type="button" data-action="table" disabled title="표 넣기(커서 문단 아래 · 고치기는 표 칸 위 우클릭) · 쪽나눔은 ⌘Enter">${icon('table')}<span>표</span></button>
          <button type="button" data-action="snippets" disabled title="상용구 — 자주 쓰는 문구를 골라 커서 자리에 넣는다 · 만들기 · 고치기 · 지우기 · ⌘K 로 이름을 쳐서 넣기">${icon('snippet')}<span>상용구</span></button>
        </div>
        <div class="tb-group" role="group" aria-label="보기">
          <button type="button" data-action="marks" aria-pressed="true" title="문단부호 보기 — 문단 끝 ↵ 를 보이거나 숨긴다(화면에만)">${icon('marks')}<span>문단부호</span></button>
          <button type="button" data-action="source" disabled title="원문 나란히 — 영문 원문(.docx)을 오른쪽에 두고 식별번호 · 청구항 번호로 함께 움직인다(다시 누르면 닫기)">${icon('columns')}<span>원문 나란히</span></button>
        </div>
        <div class="tb-group" role="group" aria-label="도구">
          <button type="button" data-action="tools" disabled title="도구 — 번호 다시 매기기 · 부호 목록 · 문서 통계 · 되풀이 문장 · 고친 곳 이동(⌘[ ⌘]) · 모든 명령은 ⌘K">${icon('wrench')}<span>도구 ▾</span></button>
          <button type="button" data-action="batch" title="일괄 처리 — 폴더 안 워드 · XML 을 HLT 로 · 폴더 안 문서마다 제출 전 점검">${icon('layers')}<span>일괄 처리 ▾</span></button>
        </div>
        <div class="tb-group" role="group" aria-label="보정(OA 대응)">
          <button type="button" data-action="review" disabled title="검토보고서 불러오기 — OA 검토보고서(워드)의 보정안 · 오기 정정을 골라 편집기에 건다">${icon('import')}<span>검토보고서</span></button>
          <button type="button" data-action="amend-marks" aria-pressed="false" disabled title="보정 표시 — 옆에서 고른 기준에 견준 바뀐 글(넣은 글 · 지운 자리 · 바뀐 문단)을 쪽 위에 보인다">${icon('diff')}<span>보정 표시</span></button>
          <select class="amend-base" aria-label="보정 표시 · 보정 근거의 기준" title="견줄 기준 — 연 뒤 고친 것 · 원출원(최종본 안 보정서까지) · 다른 파일">
            <option value="">연 뒤 고친 것</option>
            <option value="original">원출원</option>
            <option value="file">다른 파일…</option>
          </select>
          <button type="button" data-action="basis" disabled title="보정 근거 — 바뀐 청구항의 새 말이 있는 원출원 명세서 단락 · 의견서 인용 문장 초안">${icon('evidence')}<span>보정 근거</span></button>
          <button type="button" data-action="amend" disabled title="보정서 만들기 — 청구항 인용을 먼저 점검하고 보정서(.dta) · 대비표(워드)를 만든다">${icon('amend')}<span>보정서</span></button>
        </div>
        <div class="tb-group" role="group" aria-label="내보내기">
          <button type="button" data-action="pdf" disabled title="PDF로 인쇄">${icon('print')}<span>PDF</span></button>
          <button type="button" data-action="docx" disabled title="Word로 저장(표제 포함 · 식별번호 없음)">${icon('word')}<span>Word</span></button>
          <button type="button" data-action="fin" disabled title="제출파일 만들기(.hlz · .fin) — 만들기 전에 제출 전 점검을 보여 준다">${icon('send')}<span>제출파일</span></button>
        </div>
        <div class="tb-group" role="group" aria-label="도움말">
          <button type="button" data-action="help" title="도움말 — 기능별 투어(화면 위에서 차례로 짚어 준다) · 단축키 모음">${icon('help')}<span>도움말</span></button>
        </div>
        <span class="editor-status" aria-live="polite"></span>
        <button type="button" class="review-left" hidden title="걸지 않은 오기 정정 — 누를 때마다 다음 정정을 찾기 막대로"></button>
        <span class="editor-error" role="alert" hidden title="눌러서 닫기"></span>
      </div>
      <div class="find-bar" hidden>
        <label>찾을 글 <input class="find-text" type="text" placeholder="(비우면 서식만으로)"></label>
        <button type="button" class="find-option" data-option="case_insensitive" aria-pressed="false" title="대소문자 무시">Aa</button>
        <button type="button" class="find-option" data-option="whole_word" aria-pressed="false" title="낱말 단위 — 뒤에 조사는 붙어도 된다(「기판」은 「기판은」 · 「기판의」에 걸리고 「기판부」에는 안 걸린다)">단어</button>
        <button type="button" class="find-option" data-option="regex" aria-pressed="false" title="정규식(바꿀 글에 $1 · $2 …)">.*</button>
        <select class="find-range" aria-label="찾을 범위">
          <option value="">전체</option>
          <option value="description">상세설명만</option>
          <option value="claims">청구항만</option>
          <option value="span">고른 곳만</option>
        </select>
        <span class="find-scope" hidden><span></span> <button type="button" data-action="find-unscope" title="범위 풀기">✕</button></span>
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
        <div class="zoom-box" role="group" aria-label="문서 배율">
          <button type="button" data-action="zoom-out" title="문서 축소 (⌘−) " aria-label="문서 축소">${icon('zoomOut')}</button>
          <button type="button" data-action="zoom-reset" class="zoom-level" title="문서 실제 크기 (⌘0) — 지금 문서 배율" aria-label="문서 실제 크기">100%</button>
          <button type="button" data-action="zoom-in" title="문서 확대 (⌘+) " aria-label="문서 확대">${icon('zoomIn')}</button>
        </div>
        <aside class="editor-source" hidden aria-label="원문"></aside>
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
    if (inCell(selected)) return;
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
    for (const action of ['save', 'save-as', 'close-document', 'undo', 'redo', 'find', 'pdf', 'docx', 'fin', 'review', 'amend-marks', 'basis', 'amend', 'math', 'chem', 'table', 'snippets', 'history', 'align-justified', 'align-center', 'align-left', 'indent', 'outdent', 'source', 'tools']) {
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
    host.replaceChildren(renderPage(page, scale, (code) => images.get(code) ?? null, panel.hidden ? null : selected));
    drawNumbers(host, page);
    drawMarks(host, page);
    drawAmendMarks(host, page);
    paint();
    markCurrent();
  }

  /** 식별번호(최종본을 연 문서): 통특기처럼 문단 첫 줄 왼쪽 여백에 [0001](본문 왼쪽 끝 바로 앞에 오른쪽 맞춤). */
  function drawNumbers(host: HTMLElement, page: PlannedPageDto) {
    const lines = page.lines.filter((line) => line.paragraph && !line.cell && line.stops?.length);
    if (lines.length === 0) return;
    const margin = Math.min(...lines.map((line) => line.stops![0]));
    for (const line of lines) {
      if (line.start !== 0) continue;
      const own = paragraphOf(line.paragraph!)?.number;
      const number = own ?? planned.get(line.paragraph!);
      if (!number || number === '+') continue; // + = 새 문단(번호 없음)
      const mark = document.createElement('span');
      mark.className = own ? 'nk-pnum' : 'nk-pnum planned';
      if (!own) mark.title = '제출파일을 만들 때 매겨질 식별번호';
      mark.textContent = `[${number}]`;
      mark.style.right = `${(pageWidth() - margin + 3) * scale}px`;
      mark.style.top = `${(line.top + 2) * scale}px`;
      mark.style.fontSize = `${8 * scale}px`;
      host.append(mark);
    }
  }
  const pageWidth = () => view?.page_width ?? 595;

  /** 문단부호 보기: 문단이 끝나는 줄 끝에 ↵(빈 문단 포함 · 화면에만 — 인쇄 · 저장엔 없다). */
  let showMarks = true; // 기본으로 켠다(사용자 요청) · 설정 「문단부호 기본」이 바꾼다
  function drawMarks(host: HTMLElement, page: PlannedPageDto) {
    if (!showMarks) return;
    for (const line of page.lines) {
      if (!line.paragraph || line.cell || line.start === undefined || !line.stops?.length) continue;
      if (line.start + line.stops.length - 1 !== charsOf(line.paragraph).length) continue;
      // ↵ 를 그림으로 그린다(글꼴 글자는 화살표가 글줄 가운데에 떠 있다) — 바닥이 글자 바닥선(마침표 높이)에 닿게.
      const mark = document.createElement('span');
      mark.className = 'nk-pmark';
      mark.innerHTML = PARAGRAPH_MARK;
      mark.style.left = `${(line.stops.at(-1)! + 1) * scale}px`;
      mark.style.top = `${(line.top + BASELINE - MARK_HEIGHT) * scale}px`;
      mark.style.width = `${MARK_WIDTH * scale}px`;
      mark.style.height = `${MARK_HEIGHT * scale}px`;
      host.append(mark);
    }
  }

  /** 보정 표시: 최종본에 견준 넣은 글(연두 바탕) · 지운 자리(빨간 쐐기 — 올리면 지운 글) · 바뀐 문단(왼쪽 막대). */
  let showAmend = false;
  let amendMarks = new Map<string, ParagraphMarkDto>();
  function drawAmendMarks(host: HTMLElement, page: PlannedPageDto) {
    if (!showAmend) return;
    for (const line of page.lines) {
      if (!line.paragraph || line.cell || line.start === undefined || !line.stops?.length) continue;
      const mark = amendMarks.get(line.paragraph);
      if (!mark) continue;
      const stops = line.stops;
      const from = line.start;
      const to = from + stops.length - 1;
      const last = to === charsOf(line.paragraph).length;
      overlay(host, 'nk-chg', stops[0] - 7, line.top, 2);
      for (const [a, b] of mark.inserted) {
        const start = Math.max(a, from);
        const end = Math.min(b, to);
        if (end > start) overlay(host, 'nk-ins', stops[start - from], line.top, stops[end - from] - stops[start - from]);
      }
      for (const [at, text] of mark.deleted) {
        if (at < from || at > to || (at === to && !last)) continue;
        const wedge = overlay(host, 'nk-del', stops[at - from] - 1.5, line.top, 3);
        wedge.title = `지움: ${text}`;
      }
    }
  }
  const amendBase = q<HTMLSelectElement>('.amend-base');
  /** 보정 표시 · 보정 근거의 기준이 원출원인가(기본 = 연 뒤 고친 것). */
  const fromOriginal = () => amendBase.value === 'original';
  const fromFile = () => amendBase.value === 'file';
  const fileOption = q<HTMLOptionElement>('.amend-base option[value="file"]');
  /** 상태줄에 보일 기준 이름. */
  const baseName = () => (fromOriginal() ? '원출원' : fromFile() ? fileOption.textContent! : '연 뒤 고친 것');
  let lastBase = '';
  amendBase.addEventListener('change', async () => {
    if (fromFile()) {
      // 「다른 파일…」: 고를 때마다 새로 고른다(취소하면 앞 기준으로).
      say('견줄 파일 여는 중…');
      const name = await editorComparePick().catch((error) => (fail('견줄 파일', error), null));
      if (!name) {
        amendBase.value = lastBase;
        say('');
        return;
      }
      fileOption.textContent = `파일: ${name}`;
      if (!showAmend) {
        q<HTMLButtonElement>('[data-action="amend-marks"]').click();
        lastBase = amendBase.value;
        return;
      }
    }
    lastBase = amendBase.value;
    void refreshAmend(true);
  });
  /** 보정 표시를 새로 받아 그린다(켜 둔 동안 고칠 때마다). `announce` = 상태줄에 요약. */
  async function refreshAmend(announce: boolean) {
    if (!showAmend) return;
    const marks = await editorAmendmentMarks(fromOriginal(), fromFile()).catch(() => null);
    if (!marks) {
      showAmend = false;
      amendMarks = new Map();
      q<HTMLButtonElement>('[data-action="amend-marks"]').setAttribute('aria-pressed', 'false');
      for (const index of pages.keys()) redraw(index);
      if (fromFile()) {
        // 이 문서로는 아직 고른 파일이 없다(탭에 새 문서를 열었다) — 기준을 처음으로.
        amendBase.value = lastBase = '';
        say('견줄 파일을 다시 고르세요(보정 표시 옆 「다른 파일…」).');
      } else say('보정 표시는 최종본(.fin)으로 연 문서에서만 볼 수 있습니다.');
      return;
    }
    amendMarks = new Map(marks.paragraphs.map((mark) => [mark.id, mark]));
    for (const index of pages.keys()) redraw(index);
    paint();
    if (announce) {
      const issues = await editorClaimIssues().catch(() => []);
      say(amendSummary(marks, issues));
    }
  }
  function amendSummary(marks: AmendmentMarksDto, issues: CitationIssueDto[]): string {
    const parts = [
      marks.changed.length ? `바뀐 것: ${marks.changed.join(', ')}` : '',
      marks.added.length ? `새로 넣은 것: ${marks.added.join(', ')}` : '',
      marks.removed.length ? `지운 것: ${marks.removed.join(', ')}` : '',
      issues.length ? `청구항 인용 문제 ${issues.length}건(「보정서」를 누르면 목록)` : '',
    ].filter(Boolean);
    return parts.length
      ? `보정 표시(${baseName()}) — ${parts.join(' · ')}`
      : fromOriginal()
        ? '보정 표시 — 원출원과 같습니다'
        : fromFile()
          ? `보정 표시 — ${baseName()}과(와) 같습니다`
          : '보정 표시 — 연 뒤 고친 것이 없습니다(원출원에 견주려면 옆에서 「원출원」)';
  }

  /** 표 칸 글(쪽 위 칸 편집) — 화면 판마다 바탕이 실어 준다. 모델 준비 전이면 칸을 누를 때 묻는다(`loadCell`). */
  const cellTexts = new Map<string, string>();

  function layout(next: EditorViewDto, keepScroll: boolean) {
    const scroll = pagesRoot.scrollTop;
    view = next;
    const replaced = next.cp949_substitutions ?? [];
    if (replaced.length > 0) {
      const list = replaced.map((item) => `${item.from} → ${item.to || '(지움)'} (${item.count}곳)`).join(' · ');
      warn(`CP949 에 없는 글자를 바꿨습니다: ${list}`);
    }
    cellTexts.clear();
    for (const [id, text] of Object.entries(next.cells ?? {})) cellTexts.set(id, text);
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
        page.style.width = `${next.page_width * scale}px`;
        page.style.height = `${next.page_height * scale}px`;
        pagesRoot.append(page);
        observer.observe(page);
      }
    }
    if (keepScroll) pagesRoot.scrollTop = scroll;
    renderOutline(next);
    void refreshClaims(next);
    void refreshKeys(next);
    // 자동 저장: 판이 바뀔 때마다(고침 · 되돌리기) 2분을 다시 센다 — 손을 멈추면 복구본을 둔다(고친 것이 없으면 지운다).
    window.clearTimeout(autosaveTimer);
    if (next.editable) autosaveTimer = window.setTimeout(() => void editorAutosave().catch(() => {}), autosaveMs);
    say(
      `${next.title} · ${next.page_count}쪽${
        next.read_only ? ' · 읽기 전용(원본 보호) — 고치려면 「다른 이름으로 저장」으로 사본을 만드세요' : next.editable ? '' : ' · 보기 전용(이 서식은 고칠 수 없다)'
      }`,
    );
    enable();
    watchPreparing(next);
    void refreshAmend(false);
  }

  /** 문서 닫기 뒤: 쪽 · 구조 창 · 커서 · 찾기 결과를 비우고 처음 화면으로(단추는 다시 잠근다). */
  function clearDocument() {
    view = null;
    pages = new Map();
    observer.disconnect();
    pagesRoot.replaceChildren();
    outlineRoot.replaceChildren();
    claimParents = {};
    planned = new Map();
    window.clearTimeout(autosaveTimer);
    closeSource();
    images.clear();
    caret = anchor = null;
    selected = selectedCell = null;
    panel.hidden = true;
    found = [];
    foundAt = -1;
    foundKey = '';
    foundView = null;
    q<HTMLElement>('.find-bar').hidden = true;
    setScope(null);
    leftButton.hidden = true;
    paint();
    enable();
    say('문서를 닫았습니다.');
  }

  /** 접어 둔 마디(경로 = 위 마디 이름들 — 다시 그려도 문단 id 와 달리 그대로다). */
  const collapsed = new Set<string>();

  /** 구조 창: 통특기 왼쪽 트리처럼 [−]/[+] 접기 · 책 아이콘 · 점선. 마디를 누르면 그 쪽으로. */
  function renderOutline(next: EditorViewDto) {
    const order = new Map(next.paragraphs.map((paragraph, index) => [paragraph.id, index]));
    outlineMarks = [];
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
      if (node.note) {
        const note = document.createElement('span');
        note.className = 'tree-note';
        note.textContent = node.note;
        label.append(note);
      }
      label.addEventListener('click', () => {
        if (node.id) jumpTo(node.id);
        else pageHost(0)?.scrollIntoView();
      });
      if (node.id) outlineMarks.push({ index: order.get(node.id) ?? 0, label });
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
    tree.append(build(nestClaims(outlineTree(next.title, next.paragraphs), claimParents), ''));
    outlineRoot.replaceChildren(tree);
    outlineMarks.sort((a, b) => a.index - b.index);
    markCurrent(); // 청구항 나무만 새로 받아 다시 그릴 때(쪽은 그대로)
  }

  /** 청구항 나무(항 → 인용하는 앞 항) — 판이 바뀔 때마다 받아, 달라졌으면 개요를 다시 그린다. */
  let claimParents: Record<string, number[]> = {};
  let autosaveTimer = 0;
  let autosaveMs = AUTOSAVE_MS;
  let settings: SettingsDto | null = null;
  async function refreshClaims(next: EditorViewDto) {
    const parents = await editorClaimParents().catch(() => ({}));
    if (view !== next || JSON.stringify(parents) === JSON.stringify(claimParents)) return;
    claimParents = parents;
    renderOutline(next);
  }

  /** 개요 마디(문서 차례) — 화면 맨 위 문단이 든 절을 강조한다. */
  let outlineMarks: { index: number; label: HTMLElement }[] = [];
  function markCurrent() {
    const id = topParagraph();
    const at = id === null ? -1 : view?.paragraphs.findIndex((paragraph) => paragraph.id === id) ?? -1;
    const current = at < 0 ? undefined : outlineMarks.filter((mark) => mark.index <= at).pop();
    for (const mark of outlineMarks) {
      const on = mark === current;
      if (on && !mark.label.classList.contains('current')) mark.label.scrollIntoView({ block: 'nearest' });
      mark.label.classList.toggle('current', on);
    }
  }
  let marking = 0;
  pagesRoot.addEventListener('scroll', () => {
    cancelAnimationFrame(marking);
    marking = requestAnimationFrame(() => {
      markCurrent();
      syncSource();
    });
  });

  // 원문과 나란히 보기: 원문 문단(짝 이름 → 첫 문단) · 지금 판 문단 짝 이름.
  const sourcePane = q<HTMLElement>('.editor-source');
  let sourceByKey = new Map<string, HTMLElement>();
  let docKeys: (string | null)[] = [];
  /** 식별번호 없는 문서: 문단 id → 제출파일에서 매겨질 번호(「0001」). */
  let planned = new Map<string, string>();
  function openSource(rows: SourceParagraphDto[]) {
    sourceByKey = new Map();
    sourcePane.replaceChildren(
      ...rows.map((row) => {
        const element = document.createElement('p');
        element.textContent = row.text;
        if (row.key) {
          element.dataset.key = row.key;
          if (!sourceByKey.has(row.key)) sourceByKey.set(row.key, element);
          element.addEventListener('click', () => {
            const at = docKeys.indexOf(row.key);
            if (at >= 0 && view) jumpTo(view.paragraphs[at].id);
          });
        }
        return element;
      }),
    );
    sourcePane.hidden = false;
    root.classList.add('with-source');
  }
  function closeSource() {
    sourcePane.hidden = true;
    sourcePane.replaceChildren();
    sourceByKey = new Map();
    root.classList.remove('with-source');
  }
  /** 판이 바뀌면 문단 짝 이름을 다시 받고, 짝 없는 원문 문단을 흐리게. */
  /** 판이 바뀌면 문단 짝 이름을 다시 받는다 — 식별번호 없는 문서는 매겨질 번호를 쪽 왼쪽에 흐리게(원문 칸도 같은 이름을 쓴다). */
  async function refreshKeys(next: EditorViewDto) {
    const keys = await editorParagraphKeys().catch(() => []);
    if (view !== next) return;
    docKeys = keys;
    const numbered = next.paragraphs.some((paragraph) => paragraph.number);
    const nextPlanned = new Map<string, string>();
    if (!numbered) {
      keys.forEach((key, at) => {
        const paragraph = next.paragraphs[at];
        if (paragraph && key?.startsWith('p:')) nextPlanned.set(paragraph.id, key.slice(2));
      });
    }
    const changed = JSON.stringify([...nextPlanned]) !== JSON.stringify([...planned]);
    planned = nextPlanned;
    if (changed) for (const index of pages.keys()) redraw(index);
    if (sourcePane.hidden) return;
    const present = new Set(keys);
    for (const element of sourcePane.querySelectorAll<HTMLElement>('p[data-key]')) {
      element.classList.toggle('unpaired', !present.has(element.dataset.key!));
    }
    syncSource();
  }
  /** 화면 맨 위 문단(짝 이름이 없으면 앞 문단의 것)의 원문을 원문 칸 맨 위로. */
  function syncSource() {
    if (sourcePane.hidden || !view) return;
    const id = topParagraph();
    let at = id === null ? -1 : view.paragraphs.findIndex((paragraph) => paragraph.id === id);
    while (at > 0 && !docKeys[at]) at -= 1;
    const element = at >= 0 && docKeys[at] ? sourceByKey.get(docKeys[at]!) : undefined;
    if (!element || element.classList.contains('current')) return;
    sourcePane.querySelector('.current')?.classList.remove('current');
    element.classList.add('current');
    sourcePane.scrollTop = element.offsetTop;
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
  /** 다음 커서 보이기의 자리(가기는 화면 가운데로 · 평소 입력은 가장 가까이). */
  let revealBlock: ScrollLogicalPosition = 'nearest';
  /** 다시 굽지 않은 글 고침이 있는 문단 · 임시 고침 수(다시 굽는 동안 더 쳤는가). */
  let dirtyId: string | null = null;
  let edits = 0;
  let commitTimer = 0;
  let holding = 0;
  const backlog: (() => void)[] = [];

  const paragraphOf = (id: string) => view?.paragraphs.find((item) => item.id === id);
  const charsOf = (id: string) => Array.from((isCell(id) ? cellTexts.get(id) : paragraphOf(id)?.text) ?? '');
  /** 앞뒤 문단(표 칸은 칸 밖으로 안 나간다 — 가장자리에서 멈춘다). */
  const neighbour = (id: string, step: -1 | 1) => {
    if (isCell(id)) return null;
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
    if (selection.from.paragraph === selection.to.paragraph) {
      return [[selection.from.paragraph, selection.from.offset, selection.to.offset]];
    }
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
  /** 그 문단으로 가서 커서를 둔다(쪽을 먼저 보여 그리게 한 뒤 커서 줄을 화면 가운데로). */
  function jumpTo(id: string) {
    const page = view?.paragraph_pages[id];
    if (page !== undefined) pageHost(page)?.scrollIntoView({ block: 'start' });
    revealBlock = 'center';
    place({ paragraph: id, offset: 0 }, false);
  }
  /** 화면 맨 위에 보이는 문단(그린 쪽의 줄 자리로 · 못 정하면 `null`). */
  function topParagraph(): string | null {
    const top = pagesRoot.getBoundingClientRect().top;
    for (const host of pagesRoot.querySelectorAll<HTMLElement>('.nk-page')) {
      const box = host.getBoundingClientRect();
      if (box.bottom <= top) continue;
      const lines = pages.get(Number(host.dataset.index))?.lines.filter((line) => line.paragraph) ?? [];
      const line = lines.find((each) => box.top + each.top * scale >= top) ?? lines[lines.length - 1];
      return line?.paragraph ?? null;
    }
    return null;
  }
  const act = (job: () => void) => (holding > 0 ? backlog.push(job) : job());

  function overlay(host: HTMLElement, className: string, x: number, top: number, width: number) {
    const element = document.createElement('div');
    element.className = className;
    element.style.left = `${x * scale}px`;
    element.style.top = `${(top - 1) * scale}px`;
    element.style.height = `${14 * scale}px`;
    if (width) element.style.width = `${width * scale}px`;
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
      compose.style.fontSize = `${12 * scale}px`;
    }
    const rect = mark.getBoundingClientRect();
    input.style.left = `${rect.left}px`;
    input.style.top = `${rect.top}px`;
    if (reveal) {
      reveal = false;
      mark.scrollIntoView({ block: revealBlock });
      revealBlock = 'nearest';
    }
  }

  function redraw(index: number) {
    const host = pageHost(index);
    const page = pages.get(index);
    if (host && page && host.dataset.drawn === '1') {
      host.replaceChildren(renderPage(page, scale, (code) => images.get(code) ?? null, null));
      drawNumbers(host, page);
      drawMarks(host, page);
      drawAmendMarks(host, page);
    }
  }

  function place(next: Caret, extend: boolean, keepGoal = false) {
    if (dirtyId && dirtyId !== next.paragraph) void commit();
    // 고르기는 한 칸 안에서만(칸과 다른 칸 · 본문을 잇지 않는다).
    const crosses = anchor && anchor.paragraph !== next.paragraph && (isCell(anchor.paragraph) || isCell(next.paragraph));
    if (!extend || !anchor || crosses) anchor = next;
    caret = next;
    selected = next.paragraph;
    if (!keepGoal) goalX = null;
    reveal = true;
    paint();
  }

  function canEdit(): boolean {
    if (!view?.editable) {
      say(view?.read_only ? '읽기 전용으로 연 문서입니다 — 고치려면 「다른 이름으로 저장」으로 사본을 만드세요.' : '이 서식은 보기 전용입니다.');
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
          return await (isCell(id) ? editorReplaceCellText(id, text) : editorReplaceText(id, text));
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
    if (isCell(id)) {
      cellTexts.set(id, chars.join(''));
      dirtyId = id;
      edits += 1;
      return;
    }
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

  /** 친 · 붙여넣은 글에 CP949(HLT) 밖 글자가 있으면 확인 표를 거쳐 넣는다(그동안 키는 모아 둔다) · 취소면 버린다. */
  function insertReviewed(text: string) {
    if (!CP949_SUSPECT.test(text) || !canEdit()) return insertText(text);
    holding += 1;
    void (async () => {
      let next: string | null = text;
      try {
        const findings = await editorCp949Scan(text);
        if (findings.length > 0) {
          const choices = await reviewCp949(findings, '입력한 글');
          next = choices && Array.from(text, (ch) => choices[ch] ?? ch).join('');
        }
      } catch (error) {
        fail('글자 확인', error);
        next = null;
      } finally {
        holding -= 1;
        input.focus({ preventScroll: true });
        if (next) insertText(next);
        while (holding === 0 && backlog.length > 0) backlog.shift()!();
      }
    })();
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
    if (isCell(caret.paragraph)) {
      if (!cellTexts.has(caret.paragraph)) return say('표 칸 글을 읽는 중입니다 — 잠시 뒤 다시 치세요.');
      if (text.includes('\n')) return say('표 칸 글은 한 줄입니다(다음 칸은 Tab).');
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
    if (isCell(caret.paragraph)) return say('표 칸에서는 쪽을 나눌 수 없습니다.');
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

  /** 문단에만 되는 일(정렬 · 들여쓰기 · 식별항목 · 표 · 수식 넣기)을 표 칸에서 부르면 알리고 멈춘다. */
  function inCell(id: string | null | undefined): boolean {
    if (!id || !isCell(id)) return false;
    say('표 칸에서는 안 됩니다 — 표 밖 문단을 누르세요.');
    return true;
  }

  /** 표 칸에서 Tab · Shift+Tab: 쪽 차례로 다음 · 앞 칸 처음. */
  function nextCell(step: -1 | 1) {
    if (!caret) return;
    const cells = [...new Set(caretLines(pages).map((item) => item.line.cell).filter((id): id is string => Boolean(id)))];
    const target = cells[cells.indexOf(caret.paragraph) + step];
    if (target) void enterCell({ paragraph: target, offset: 0 }, false);
  }

  /** 칸에 커서를 둔다 — 칸 글이 아직 없으면(모델 준비 전) 먼저 묻는다. */
  async function enterCell(next: Caret, extend: boolean) {
    if (!cellTexts.has(next.paragraph)) {
      try {
        cellTexts.set(next.paragraph, await editorCellText(next.paragraph));
      } catch (error) {
        fail('칸 읽기', error);
        return;
      }
    }
    place(next, extend);
  }

  const pointCaret = (event: MouseEvent): Caret | null => {
    const host = (event.target as HTMLElement).closest<HTMLElement>('.nk-page');
    if (!host) return null;
    const box = host.getBoundingClientRect();
    return hit(caretLines(pages), Number(host.dataset.index), (event.clientX - box.left) / scale, (event.clientY - box.top) / scale);
  };
  let dragging = false;
  pagesRoot.addEventListener('mousedown', (event) => {
    if (event.button !== 0 || !view) return;
    if ((event.target as HTMLElement).closest('.nk-image-handle')) return;
    clearPicture();
    const picture = pictureAt(event);
    if (picture && view.editable) {
      event.preventDefault();
      return selectPicture(picture);
    }
    const next = pointCaret(event);
    if (!next) return;
    event.preventDefault();
    const shift = event.shiftKey;
    act(() => (isCell(next.paragraph) && !cellTexts.has(next.paragraph) ? void enterCell(next, shift) : place(next, shift)));
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

  /** 누른 자리의 그림(글 줄이 그림 아래쪽을 덮으니 대상이 아니라 자리로 찾는다). */
  function pictureAt(event: MouseEvent): HTMLImageElement | null {
    const all = pagesRoot.querySelectorAll<HTMLImageElement>('img.nk-image[data-image]');
    return (
      Array.from(all).find((picture) => {
        const box = picture.getBoundingClientRect();
        return event.clientX >= box.left && event.clientX <= box.right && event.clientY >= box.top && event.clientY <= box.bottom;
      }) ?? null
    );
  }
  function clearPicture() {
    pagesRoot.querySelectorAll('.nk-image-frame').forEach((frame) => frame.remove());
  }
  /** 그림 고르기: 테두리 + 오른쪽 아래 손잡이. 손잡이를 끌면 비율을 잠근 채 크기가 따라오고, 놓으면 그 배율로 다시 굽는다. */
  function selectPicture(picture: HTMLImageElement) {
    const frame = document.createElement('div');
    frame.className = 'nk-image-frame';
    const handle = document.createElement('div');
    handle.className = 'nk-image-handle';
    frame.append(handle);
    const fit = () => {
      frame.style.left = picture.style.left;
      frame.style.top = picture.style.top;
      frame.style.width = picture.style.width;
      frame.style.height = picture.style.height;
    };
    fit();
    picture.after(frame);
    handle.addEventListener('mousedown', (event) => {
      if (event.button !== 0 || !canEdit()) return;
      event.preventDefault();
      event.stopPropagation();
      const [startX, startY] = [event.clientX, event.clientY];
      const width = parseFloat(picture.style.width);
      const height = parseFloat(picture.style.height);
      const ratio = height / width;
      let next = width;
      const move = (moved: MouseEvent) => {
        // 가로 · 세로 중 더 많이 끈 쪽을 따른다 — 세로는 늘 비율대로.
        const dx = moved.clientX - startX;
        const dy = (moved.clientY - startY) / ratio;
        next = Math.max(8, width + (Math.abs(dx) >= Math.abs(dy) ? dx : dy));
        picture.style.width = `${next}px`;
        picture.style.height = `${next * ratio}px`;
        fit();
      };
      const up = () => {
        window.removeEventListener('mousemove', move);
        window.removeEventListener('mouseup', up);
        const scale = next / width;
        if (Math.abs(scale - 1) < 0.01) return;
        const id = picture.dataset.image!;
        void run('그림 크기', () => editorResizeImage(id, scale));
      };
      window.addEventListener('mousemove', move);
      window.addEventListener('mouseup', up);
    });
  }
  window.addEventListener('keydown', (event) => {
    if (event.key === 'Escape') clearPicture();
  });
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
      if (isCell(caret.paragraph)) {
        const shift = event.shiftKey;
        act(() => nextCell(shift ? -1 : 1));
        return;
      }
      // ⇧Tab = 들여쓰기 취소(워드 · 한글과 같게). 전엔 탭 문자를 넣어 글이 도리어 오른쪽으로 밀렸다.
      if (event.shiftKey) {
        q<HTMLButtonElement>('[data-action="outdent"]').click();
        return;
      }
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
    act(() => insertReviewed(text));
    paint();
  });
  input.addEventListener('input', () => {
    if (composing) return paint();
    const text = input.value;
    input.value = '';
    if (text) act(() => insertReviewed(text));
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
    const cell = (event.target as HTMLElement).closest<HTMLElement>('[data-cell]')?.dataset.cell ?? null;
    void contextMenu(cell, pictureAt(event)).catch((error) => fail('메뉴', error));
  });
  /** 식별항목 넣기: 번호를 묻고(같은 종류 가장 큰 번호 + 1 을 채워 둔다) 커서 문단 아래(문단 맨 앞이면 위)에 표제 + 빈 문단. */
  async function insertItem(kind: string) {
    if (!caret || !canEdit() || inCell(caret.paragraph)) return;
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

  /** 그림 파일(JPG · TIFF)을 골라 커서 문단 뒤에 그림 문단으로 넣는다 · 커서는 그대로. */
  /** 표 칸이면 칸 글의 커서 자리에, 아니면 커서 문단 뒤에 그림 문단으로. */
  async function insertPicture() {
    if (!caret || !canEdit()) return;
    const { paragraph: at, offset } = caret;
    await structural('이미지 넣기', async () => ({
      next: await editorInsertPicture(at, isCell(at) ? offset : undefined),
      caret: null,
    }));
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
  /** CP949 밖 글자 확인 표: 줄 = 글자 한 종류(코드 · 몇 곳 · 처음 나온 곳 앞뒤 글 · 바꿀 글자 칸 · 지움).
   *  돌려줌 = 고른 대응(글자 → 바꿀 글 · 빈 글 = 지움) · 취소면 `null`. 바꿀 글도 CP949 인지 확인한 뒤에 닫는다. */
  function reviewCp949(findings: Cp949FindingDto[], what: string): Promise<Record<string, string> | null> {
    return new Promise((resolve) => {
      const box = document.createElement('form');
      box.className = 'cp949-review';
      const rows = findings
        .map((item, i) => {
          const from = escapeHtml(item.from);
          const context = escapeHtml(item.context).split(from).join(`<mark>${from}</mark>`);
          const none = item.suggestion === null;
          return `<tr class="${none ? 'cp949-none' : ''}"><td class="cp949-char">${from}</td><td>${item.code}</td><td>${item.count}곳</td>
            <td class="cp949-context">${context}</td>
            <td><input data-row="${i}" value="${escapeHtml(item.suggestion ?? '')}" placeholder="${none ? '대응 없음 — 입력' : ''}" size="9" autocomplete="off" spellcheck="false"></td>
            <td><input type="checkbox" data-drop="${i}"></td></tr>`;
        })
        .join('');
      box.innerHTML = `<header>${escapeHtml(what)} — HLT(CP949)에 쓸 수 없는 글자</header>
        <p class="cp949-hint">바꿀 글자를 확인하거나 고치세요. 대응이 없는 글자(붉은 줄)는 직접 넣거나 「지움」을 고르세요.</p>
        <div class="cp949-table"><table><thead><tr><th>글자</th><th>코드</th><th>횟수</th><th>앞뒤 글</th><th>바꿀 글자</th><th>지움</th></tr></thead><tbody>${rows}</tbody></table></div>
        <p class="cp949-error" aria-live="polite"></p>
        <div class="cp949-buttons"><button type="submit">바꾸기</button><button type="button" class="cp949-cancel">취소</button></div>`;
      const error = box.querySelector<HTMLParagraphElement>('.cp949-error')!;
      const done = (value: Record<string, string> | null) => {
        box.remove();
        resolve(value);
      };
      box.addEventListener('submit', async (event) => {
        event.preventDefault();
        const choices: Record<string, string> = {};
        for (const [i, item] of findings.entries()) {
          const drop = box.querySelector<HTMLInputElement>(`[data-drop="${i}"]`)!.checked;
          const value = drop ? '' : box.querySelector<HTMLInputElement>(`[data-row="${i}"]`)!.value;
          if (!value && !drop) {
            error.textContent = `${item.from} (${item.code}): 바꿀 글자를 넣거나 「지움」을 고르세요.`;
            return;
          }
          choices[item.from] = value;
        }
        const still = await editorCp949Scan(Object.values(choices).join(''));
        if (still.length > 0) {
          error.textContent = `바꿀 글자에도 쓸 수 없는 글자가 있습니다: ${still.map((item) => `${item.from} (${item.code})`).join(', ')}`;
          return;
        }
        done(choices);
      });
      box.querySelector('.cp949-cancel')!.addEventListener('click', () => done(null));
      box.addEventListener('keydown', (event) => {
        if (event.key === 'Escape') done(null);
      });
      root.append(box);
      box.querySelector<HTMLInputElement>('[data-row]')?.focus();
    });
  }

  /** ⌘G 가기: 식별번호 · 청구항 · 도 · 표제 → 그 문단 첫머리로(못 찾으면 창을 두고 다시 치게). */
  function askGo(): Promise<void> {
    return new Promise((resolve) => {
      const box = document.createElement('form');
      box.className = 'item-ask go-ask';
      box.innerHTML = `<label>가기 <input class="go-text" size="14" autocomplete="off" spellcheck="false"></label>
        <button type="submit">가기</button><span class="item-hint">0049 · 청구항 7 · 도 3 · 기술분야</span>`;
      const field = box.querySelector<HTMLInputElement>('.go-text')!;
      const done = () => {
        box.remove();
        input.focus({ preventScroll: true });
        resolve();
      };
      box.addEventListener('submit', (event) => {
        event.preventDefault();
        // 식별번호 없는 문서는 매겨질 번호로도 간다.
        const paragraphs = view?.paragraphs.map((p) => (p.number || !planned.has(p.id) ? p : { ...p, number: planned.get(p.id) }));
        const id = paragraphs ? goTarget(field.value, paragraphs) : null;
        if (!id) {
          box.querySelector('.item-hint')!.textContent = `「${field.value.trim()}」이(가) 없습니다`;
          field.select();
          return;
        }
        done();
        jumpTo(id);
        say(`가기: ${field.value.trim()}`);
      });
      field.addEventListener('keydown', (event) => {
        if (event.key === 'Escape') done();
      });
      root.append(box);
      field.focus();
    });
  }

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

  /** 그림 크기를 mm 로 넣기(비율 고정 — 한 칸을 고치면 다른 칸이 따라온다) → 그 배율로 다시 굽는다. */
  /** 보정안 불러오기 창: 보정안 고르기 · 상세설명 오기 후보(바른 말 고쳐 쓰기) · 도면 할 일. 취소면 null. */
  function askReview(
    report: ReviewReportDto,
  ): Promise<{ claims: ProposedClaimDto[]; tracked: boolean; notes: DescriptionNoteDto[] } | null> {
    const escape = (text: string) =>
      text.replace(/[&<>"]/g, (ch) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[ch]!);
    const summary = (claims: ProposedClaimDto[]) => {
      const changed = claims.filter((c) => c.before.length > 0 && c.after.length > 0 && c.before.join() !== c.after.join());
      const deleted = claims.filter((c) => c.after.length === 0);
      const added = claims.filter((c) => c.before.length === 0);
      const list = (items: ProposedClaimDto[]) => items.map((c) => c.number).join(', ');
      return [
        changed.length ? `바뀐 항 ${list(changed)}` : '',
        deleted.length ? `삭제 ${list(deleted)}` : '',
        added.length ? `새 항 ${list(added)}` : '',
      ]
        .filter(Boolean)
        .join(' · ');
    };
    return new Promise((resolve) => {
      const box = document.createElement('form');
      box.className = 'item-ask review-ask';
      const proposals = report.proposals
        .map(
          (proposal, index) => `<label class="review-proposal"><input type="radio" name="proposal" value="${index}"${
            index === 0 ? ' checked' : ''
          }> ${escape(proposal.label)} — ${escape(summary(proposal.claims) || '바뀐 청구항 없음')}${
            proposal.tracked ? '' : ' <em>(추적 변경 없음 · 보정 전 대조 생략)</em>'
          }</label>`,
        )
        .join('');
      const notes = report.notes
        .map(
          (note, index) => `<label class="review-note" title="${escape(note.sentence)}"><input type="checkbox" data-note="${index}"${
            note.right && !note.positional && report.matches[index] > 0 ? ' checked' : ''
          }> [${note.paragraphs.join('], [')}] “${escape(note.wrong)}” → <input class="review-right" data-note="${index}" size="16" value="${escape(
            note.right ?? '',
          )}" placeholder="바른 말"> <small>(${
            report.matches[index] > 0
              ? `<button type="button" class="review-goto" data-note="${index}" title="그 자리로 — 창을 접고 찾기 막대로">${report.matches[index]}곳</button>`
              : '0곳'
          }${
            note.positional ? ' · 한 자리만 고치라는 정정 — 화면에서 직접' : ''
          })</small></label>`,
        )
        .join('');
      const figures = report.figures
        .map((figure) => `<li title="${escape(figure.sentence)}">도 ${escape(figure.number)} — 그림 위 오른쪽 클릭 「그림 바꾸기…」</li>`)
        .join('');
      box.innerHTML = `<strong>보정안 불러오기</strong>
        ${report.proposals.length ? `<fieldset><legend>청구항</legend>${proposals}<label class="review-proposal"><input type="radio" name="proposal" value="-1"> 청구항은 걸지 않음</label></fieldset>` : ''}
        ${notes ? `<fieldset><legend>상세설명 오기 정정(고른 것만 · 그 번호 문단 안에서)</legend>${notes}</fieldset>` : ''}
        ${figures ? `<fieldset><legend>도면(새 그림이 있어야 한다)</legend><ul>${figures}</ul></fieldset>` : ''}
        <button type="submit">걸기</button><button type="button" class="item-cancel">취소</button>
        <span class="item-hint">걸고 나서 화면에서 확인 · 손본 뒤 「보정서」를 누르세요 · 되돌리기 한 번에 전부 돌아갑니다</span>`;
      // 「N곳」: 창을 접고(고른 것은 그대로) 찾기 막대로 그 번호 문단 안에서 차례로 · 띠를 누르면 창이 다시 보인다.
      const reopen = document.createElement('button');
      reopen.type = 'button';
      reopen.className = 'review-reopen';
      reopen.textContent = '보정안 창 다시 열기';
      reopen.addEventListener('click', () => {
        reopen.remove();
        box.hidden = false;
      });
      box.querySelectorAll<HTMLButtonElement>('.review-goto').forEach((button) =>
        button.addEventListener('click', () => {
          const note = report.notes[Number(button.dataset.note)];
          box.hidden = true;
          root.append(reopen);
          void findIn(note.wrong, note.paragraphs);
        }),
      );
      const done = (value: { claims: ProposedClaimDto[]; tracked: boolean; notes: DescriptionNoteDto[] } | null) => {
        reopen.remove();
        box.remove();
        input.focus({ preventScroll: true });
        resolve(value);
      };
      box.addEventListener('submit', (event) => {
        event.preventDefault();
        const chosen = Number(box.querySelector<HTMLInputElement>('input[name="proposal"]:checked')?.value ?? -1);
        const proposal = report.proposals[chosen];
        const picked = report.notes.flatMap((note, index) => {
          const on = box.querySelector<HTMLInputElement>(`input[type="checkbox"][data-note="${index}"]`)!.checked;
          const right = box.querySelector<HTMLInputElement>(`.review-right[data-note="${index}"]`)!.value;
          return on && right ? [{ ...note, right }] : [];
        });
        done({ claims: proposal?.claims ?? [], tracked: proposal?.tracked ?? false, notes: picked });
      });
      box.querySelector('.item-cancel')!.addEventListener('click', () => done(null));
      box.addEventListener('keydown', (event) => {
        if (event.key === 'Escape') done(null);
      });
      root.append(box);
      box.querySelector<HTMLButtonElement>('button[type="submit"]')!.focus();
    });
  }

  /** 보정안을 건 결과 한 줄. */
  function reviewSummary(outcome: ReviewOutcomeDto, figures: number): string {
    const list = (items: string[]) => items.join(', ');
    const notes = outcome.notes_applied.reduce((sum, [, count]) => sum + count, 0);
    return [
      outcome.claims_changed.length ? `바뀐 항 ${list(outcome.claims_changed)}` : '',
      outcome.claims_deleted.length ? `삭제 ${list(outcome.claims_deleted)}` : '',
      outcome.claims_added.length ? `새 항 ${list(outcome.claims_added)}` : '',
      outcome.claims_failed.length
        ? `못 건 항 ${outcome.claims_failed.map(([number, why]) => `${number}(${why})`).join(', ')}`
        : '',
      notes ? `오기 ${notes}곳 정정` : '',
      figures ? `도면 ${figures}개는 「그림 바꾸기…」로` : '',
    ]
      .filter(Boolean)
      .join(' · ') || '바뀐 것이 없습니다';
  }

  /** 보정 근거 창: 청구항 · 새 말마다 근거 후보(체크 · 누르면 그 단락으로) → 초안 글(고쳐 쓸 수 있다) → 복사 · 새 의견서. */
  function askBasis(basis: ClaimBasisDto[]): Promise<{ kind: 'copy' | 'opinion'; text: string } | null> {
    const escape = (text: string) =>
      text.replace(/[&<>"]/g, (ch) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[ch]!);
    const phraseCount = basis.reduce((sum, claim) => sum + claim.phrases.length, 0);
    return new Promise((resolve) => {
      // 화면 전체를 덮는 창(왼쪽 = 청구항별 근거 후보 · 오른쪽 = 초안) — 좁으면 위아래로.
      const shade = document.createElement('div');
      shade.className = 'basis-shade';
      const box = document.createElement('form');
      box.className = 'basis-ask';
      box.setAttribute('role', 'dialog');
      box.setAttribute('aria-label', '보정 근거');
      const hitRow = (hit: ClaimBasisDto['phrases'][number]['hits'][number], c: number, p: number, h: number) => `
        <label class="basis-hit">
          <input type="checkbox" data-c="${c}" data-p="${p}" data-h="${h}"${
            (h === 0 && (hit.exact || hit.score >= 0.8)) || (h === 1 && hit.exact) ? ' checked' : ''
          }>
          <button type="button" class="basis-go" data-number="${hit.number}" title="그 단락으로 가기(창을 접는다)">【${hit.number}】</button>
          <span class="basis-match ${hit.exact ? 'exact' : ''}">${hit.exact ? '통째로' : `낱말 ${Math.round(hit.score * 100)}%`}</span>
          <span class="basis-quote" title="${escape(hit.quote)}">${escape(hit.quote)}</span>
          <span class="basis-figures">${hit.figures.length ? `도 ${hit.figures.map(escape).join(', ')}` : ''}</span>
        </label>`;
      const cards = basis
        .map(
          (claim, c) => `<section class="basis-claim">
            <header><strong>청구항 ${claim.claim}</strong><span class="basis-badge ${claim.added ? 'added' : ''}">${claim.added ? '새 항' : '바뀐 항'}</span></header>
            ${claim.phrases
              .map(
                (phrase, p) => `<div class="basis-phrase">
                  <div class="basis-phrase-text"><span class="basis-label">새로 넣은 말</span>${escape(phrase.text)}</div>
                  ${
                    phrase.hits.length
                      ? `<div class="basis-hits">${phrase.hits.slice(0, 3).map((hit, h) => hitRow(hit, c, p, h)).join('')}${
                          phrase.hits.length > 3
                            ? `<div class="basis-more" hidden>${phrase.hits
                                .slice(3)
                                .map((hit, h) => hitRow(hit, c, p, h + 3))
                                .join('')}</div><button type="button" class="basis-more-toggle">후보 ${phrase.hits.length - 3}개 더 보기</button>`
                            : ''
                        }</div>`
                      : '<div class="basis-none">근거 단락을 찾지 못했습니다 — 초안에 직접 쓰세요.</div>'
                  }
                </div>`,
              )
              .join('')}
          </section>`,
        )
        .join('');
      box.innerHTML = `
        <header class="basis-head">
          <div><h2>보정 근거</h2><p>청구항 ${basis.length}개 · 새로 넣은 말 ${phraseCount}개 — 근거로 쓸 단락을 고르면 의견서 초안이 바로 바뀝니다.</p></div>
          <button type="button" class="item-cancel basis-close" aria-label="닫기">✕</button>
        </header>
        <div class="basis-body">
          <div class="basis-list">${cards}</div>
          <aside class="basis-side">
            <label for="basis-draft">의견서 초안 <small>고쳐 쓸 수 있습니다 · 체크를 바꾸면 다시 만듭니다</small></label>
            <textarea id="basis-draft" class="basis-draft" spellcheck="false"></textarea>
            <div class="basis-actions">
              <button type="button" class="basis-copy primary">글 복사</button>
              <button type="button" class="basis-opinion">새 의견서로 열기</button>
            </div>
          </aside>
        </div>`;
      const draft = box.querySelector<HTMLTextAreaElement>('.basis-draft')!;
      const redraft = () => {
        draft.value = basisDraft(
          basis.map((claim, c) => ({
            ...claim,
            phrases: claim.phrases.map((phrase, p) => ({
              ...phrase,
              hits: phrase.hits.filter(
                (_, h) => box.querySelector<HTMLInputElement>(`input[data-c="${c}"][data-p="${p}"][data-h="${h}"]`)!.checked,
              ),
            })),
          })),
        );
      };
      redraft();
      box.addEventListener('change', (event) => {
        if ((event.target as HTMLElement).matches('input[type="checkbox"]')) redraft();
      });
      // 【번호】: 창을 접고 그 단락으로 · 띠를 누르면 고른 것 · 고쳐 쓴 초안 그대로 다시 보인다.
      const reopen = document.createElement('button');
      reopen.type = 'button';
      reopen.className = 'review-reopen';
      reopen.textContent = '보정 근거 창 다시 열기';
      reopen.addEventListener('click', () => {
        reopen.remove();
        shade.hidden = false;
      });
      const done = (value: { kind: 'copy' | 'opinion'; text: string } | null) => {
        reopen.remove();
        shade.remove();
        input.focus({ preventScroll: true });
        resolve(value);
      };
      box.querySelector('.basis-copy')!.addEventListener('click', () => done({ kind: 'copy', text: draft.value }));
      box.querySelector('.basis-opinion')!.addEventListener('click', () => done({ kind: 'opinion', text: draft.value }));
      box.querySelector('.item-cancel')!.addEventListener('click', () => done(null));
      shade.addEventListener('mousedown', (event) => {
        if (event.target === shade) done(null);
      });
      box.addEventListener('keydown', (event) => {
        if (event.key === 'Escape') done(null);
      });
      for (const button of box.querySelectorAll<HTMLButtonElement>('.basis-go')) {
        button.addEventListener('click', () => {
          const target = view?.paragraphs.find((paragraph) => paragraph.number === button.dataset.number);
          if (!target) return;
          shade.hidden = true;
          root.append(reopen);
          jumpTo(target.id);
        });
      }
      for (const toggle of box.querySelectorAll<HTMLButtonElement>('.basis-more-toggle')) {
        toggle.addEventListener('click', () => {
          const more = toggle.previousElementSibling as HTMLElement;
          more.hidden = !more.hidden;
          toggle.textContent = more.hidden ? `후보 ${more.children.length}개 더 보기` : '후보 접기';
        });
      }
      box.addEventListener('submit', (event) => event.preventDefault());
      shade.append(box);
      root.append(shade);
      box.querySelector<HTMLButtonElement>('.basis-copy')!.focus();
    });
  }

  /** 보정서 앞 청구항 인용 점검: 문제 목록(누르면 그 항으로) · 「그래도 만들기」면 true. */
  /** 제출 전 점검 창: 종류별 목록 · 「가기」는 창을 접고 그 문단으로(띠로 다시 열기) · 그래도 만들기 / 멈추기. */
  function askSubmit(issues: SubmitIssueDto[]): Promise<boolean> {
    const escape = (text: string) =>
      text.replace(/[&<>"]/g, (ch) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[ch]!);
    const groups = SUBMIT_GROUPS;
    return new Promise((resolve) => {
      const box = document.createElement('form');
      box.className = 'item-ask review-ask submit-ask';
      box.innerHTML = `<strong>제출 전 점검 — ${issues.length}건</strong>${groups
        .map(([kind, title]) => {
          const rows = issues
            .map((issue, index) => ({ issue, index }))
            .filter(({ issue }) => issue.kind === kind)
            .map(
              ({ issue, index }) =>
                `<li>${issue.paragraph_id ? `<button type="button" class="submit-go" data-index="${index}">가기</button> ` : ''}${escape(issue.detail)}</li>`,
            )
            .join('');
          const fix =
            kind === 'spacing' && issues.some((issue) => issue.kind === kind && issue.detail.startsWith('공백 두 칸'))
              ? '<button type="button" class="submit-fix-spaces">공백 두 칸 → 한 칸으로 모두 고치기</button>'
              : '';
          return rows ? `<fieldset><legend>${title}</legend>${fix}<ul>${rows}</ul></fieldset>` : '';
        })
        .join('')}
        <button type="submit">그래도 만들기</button><button type="button" class="item-cancel">멈추기</button>`;
      const reopen = document.createElement('button');
      reopen.type = 'button';
      reopen.className = 'review-reopen';
      reopen.textContent = '점검 창 다시 열기';
      reopen.addEventListener('click', () => {
        reopen.remove();
        box.hidden = false;
      });
      const done = (value: boolean) => {
        reopen.remove();
        box.remove();
        input.focus({ preventScroll: true });
        resolve(value);
      };
      box.addEventListener('submit', (event) => {
        event.preventDefault();
        done(true);
      });
      box.querySelector('.item-cancel')!.addEventListener('click', () => done(false));
      box.addEventListener('keydown', (event) => {
        if (event.key === 'Escape') done(false);
      });
      box.querySelector<HTMLButtonElement>('.submit-fix-spaces')?.addEventListener('click', async (event) => {
        const button = event.currentTarget as HTMLButtonElement;
        if (!canEdit()) return;
        button.disabled = true;
        await run('공백 두 칸 고치기', editorFixDoubleSpaces);
        // 표 칸 글은 고치지 않는다 — 다시 점검해 남은 곳만 둔다.
        const double = (issue: SubmitIssueDto) => issue.kind === 'spacing' && issue.detail.startsWith('공백 두 칸');
        const left = new Set((await editorSubmissionChecks().catch(() => issues)).filter(double).map((issue) => issue.paragraph_id));
        for (const go of button.parentElement!.querySelectorAll<HTMLButtonElement>('.submit-go')) {
          const issue = issues[Number(go.dataset.index)];
          if (double(issue) && !left.has(issue.paragraph_id)) go.closest('li')!.remove();
        }
        button.replaceWith(
          Object.assign(document.createElement('p'), {
            textContent: `공백 두 칸을 한 칸으로 고쳤습니다(⌘Z로 되돌리기).${left.size ? ` 표 칸 ${left.size}곳은 직접 고치세요.` : ''}`,
          }),
        );
      });
      for (const button of box.querySelectorAll<HTMLButtonElement>('.submit-go')) {
        button.addEventListener('click', () => {
          const id = issues[Number(button.dataset.index)].paragraph_id!;
          box.hidden = true;
          root.append(reopen);
          jumpTo(id);
        });
      }
      root.append(box);
      box.querySelector<HTMLButtonElement>('button[type="submit"]')!.focus();
    });
  }

  function askCitations(issues: CitationIssueDto[]): Promise<boolean> {
    const what: Record<CitationIssueDto['kind'], (cited: string) => string> = {
      mis_reference: (cited) => `자기 항 · 뒤 항 · 없는 항(${cited})을 인용`,
      removed: (cited) => `지운 청구항 ${cited}을(를) 인용`,
      duplicate_multiple: () => '다중 인용 항을 다시 다중 인용(중복 다중참조)',
      no_alternative: () => '「내지」 범위 뒤에 「어느」가 없다(택일어 미사용)',
      range_mis_reference: (cited) => `범위 안에 자기 항 · 뒤 항 · 없는 항(${cited})`,
      and_citation: () => '「및 · 과 · 와」로 묶어 인용(택일적이 아니다)',
      range_no_alternative: () => '범위 인용 뒤에 「어느」가 없다',
    };
    const app = new Set<CitationIssueDto['kind']>(['removed', 'range_mis_reference', 'and_citation', 'range_no_alternative']);
    return new Promise((resolve) => {
      const box = document.createElement('form');
      box.className = 'item-ask citation-ask';
      box.innerHTML = `<strong>청구항 인용 점검 — ${issues.length}건</strong><ul>${issues
        .map(
          (issue, index) => `<li><button type="button" class="citation-go" data-index="${index}">청구항 ${issue.claim}</button> ${what[
            issue.kind
          ](issue.cited.join(', '))}${app.has(issue.kind) ? ' <small>(특허로 점검 페이지는 못 잡는 것)</small>' : ''}</li>`,
        )
        .join('')}</ul>
        <button type="submit">그래도 만들기</button><button type="button" class="item-cancel">취소</button>`;
      const done = (value: boolean) => {
        box.remove();
        input.focus({ preventScroll: true });
        resolve(value);
      };
      box.addEventListener('submit', (event) => {
        event.preventDefault();
        done(true);
      });
      box.querySelector('.item-cancel')!.addEventListener('click', () => done(false));
      box.addEventListener('keydown', (event) => {
        if (event.key === 'Escape') done(false);
      });
      for (const button of box.querySelectorAll<HTMLButtonElement>('.citation-go')) {
        button.addEventListener('click', () => {
          const heading = `【청구항 ${issues[Number(button.dataset.index)].claim}】`;
          const target = view?.paragraphs.find((paragraph) => paragraph.text.trim() === heading);
          if (target) place({ paragraph: target.id, offset: 0 }, false);
        });
      }
      root.append(box);
      box.querySelector<HTMLButtonElement>('button[type="submit"]')!.focus();
    });
  }

  function pictureSize(picture: HTMLImageElement): Promise<void> {
    // 인쇄는 그림을 표시 크기보다 0.76pt 작게 그린다(print.rs DRAW_INSET_WIDTH/HEIGHT) — 되돌려 표시 크기로 보인다.
    const mm = (pt: number) => ((pt + 0.76) * 25.4) / 72;
    const width = mm(parseFloat(picture.dataset.width ?? '0'));
    const height = mm(parseFloat(picture.dataset.height ?? '0'));
    if (!(width > 0 && height > 0)) return Promise.resolve();
    return new Promise((resolve) => {
      const box = document.createElement('form');
      box.className = 'item-ask';
      box.innerHTML = `<label>가로 <input class="picture-width" size="6" autocomplete="off"> mm</label>
        <label>× 세로 <input class="picture-height" size="6" autocomplete="off"> mm</label>
        <button type="submit">바꾸기</button><button type="button" class="item-cancel">취소</button>
        <span class="item-hint">비율 고정 · 본문 174 × 240mm, 표 칸은 칸 폭보다 커지지 않습니다</span>`;
      const across = box.querySelector<HTMLInputElement>('.picture-width')!;
      const down = box.querySelector<HTMLInputElement>('.picture-height')!;
      across.value = width.toFixed(1);
      down.value = height.toFixed(1);
      const follow = (from: HTMLInputElement, to: HTMLInputElement, ratio: number) =>
        from.addEventListener('input', () => {
          const value = parseFloat(from.value);
          if (value > 0) to.value = (value * ratio).toFixed(1);
        });
      follow(across, down, height / width);
      follow(down, across, width / height);
      const done = () => {
        box.remove();
        input.focus({ preventScroll: true });
        resolve();
      };
      box.addEventListener('submit', (event) => {
        event.preventDefault();
        const value = parseFloat(across.value);
        if (!(value > 0)) {
          box.querySelector('.item-hint')!.textContent = '0 보다 큰 숫자를 넣어 주세요';
          return;
        }
        done();
        const scale = value / width;
        if (Math.abs(scale - 1) >= 0.001) void run('그림 크기', () => editorResizeImage(picture.dataset.image!, scale));
      });
      box.querySelector('.item-cancel')!.addEventListener('click', done);
      box.addEventListener('keydown', (event) => {
        if (event.key === 'Escape') done();
      });
      root.append(box);
      across.focus();
      across.select();
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
    if (inCell(after)) return;
    if (!after || !canEdit()) {
      say('표: 넣을 자리(문단)를 먼저 누르세요.');
      return;
    }
    const size = await askTableSize();
    input.focus({ preventScroll: true });
    if (!size) return;
    await structural('표 넣기', async () => ({
      next: (await editorInsertTable(after, size.rows, size.columns)).view,
      caret: null,
    }));
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

  async function contextMenu(cell: string | null, picture: HTMLImageElement | null = null) {
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
        { text: '들여쓰기', enabled: caret !== null && editable, action: button('indent') },
        { text: '들여쓰기 취소', enabled: caret !== null && editable, action: button('outdent') },
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
        {
          text: '이미지 파일 선택하기',
          enabled: caret !== null && editable,
          action: () => void insertPicture(),
        },
        { text: '그림 크기…', enabled: picture !== null && editable, action: () => picture && void pictureSize(picture) },
        {
          text: '그림 바꾸기…',
          enabled: picture !== null && editable,
          action: () => picture && void run('그림 바꾸기', () => editorReplacePicture(picture.dataset.image!)),
        },
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
    act(() => insertReviewed(text));
  });

  /** 쪽에서 고른 글자에 서식을 건다(고른 것은 그대로 남는다). */
  async function pageFormat(format: CharacterFormatDto, size?: number) {
    if (caret && isCell(caret.paragraph)) return say('표 칸 글자모양은 아직 바꿀 수 없습니다.');
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
  const findArgs = (): [string, FindFormatDto | null, FindOptionsDto] => [
    q<HTMLInputElement>('.find-text').value,
    (q<HTMLSelectElement>('.find-format').value || null) as FindFormatDto | null,
    Object.fromEntries(
      [...root.querySelectorAll<HTMLButtonElement>('.find-option')].map((button) => [
        button.dataset.option!,
        button.getAttribute('aria-pressed') === 'true',
      ]),
    ) as FindOptionsDto,
  ];
  for (const button of root.querySelectorAll<HTMLButtonElement>('.find-option')) {
    button.addEventListener('click', () =>
      button.setAttribute('aria-pressed', String(button.getAttribute('aria-pressed') !== 'true')),
    );
  }
  /** 정규식으로 찾는 중이면 그 조건(바꾸기에 넘겨 `$1` 을 채운다). */
  const regexPattern = () => {
    const [text, format, options] = findArgs();
    return options.regex ? { text, format, ...options } : null;
  };
  /** 찾기 범위(식별번호 · 보정안 오기 정정에서) — 없으면 문서 전체. */
  let findScope: string[] | null = null;
  /** 사용자가 고른 범위(절 · 고른 곳) — 보정안 번호 범위와 따로 · 새로 쳐도 남는다. 고른 곳은 고를 때 얼려 둔다. */
  let findRange: { kind: 'description' | 'claims' | 'span'; from?: Caret; to?: Caret } | null = null;
  const RANGE_LABEL = { description: '상세설명', claims: '청구항', span: '고른 곳' } as const;
  const rangeSelect = q<HTMLSelectElement>('.find-range');
  rangeSelect.addEventListener('change', () => {
    const kind = rangeSelect.value as '' | 'description' | 'claims' | 'span';
    if (kind === 'span') {
      const picked = span();
      if (!picked) {
        countLabel.textContent = '고른 곳만: 쪽에서 범위를 먼저 고르세요';
        rangeSelect.value = '';
        findRange = null;
        return;
      }
      findRange = { kind, ...picked };
    } else findRange = kind ? { kind } : null;
    countLabel.textContent = '';
  });
  const findKey = () => JSON.stringify([...findArgs(), findScope, findRange]);
  const setScope = (numbers: string[] | null) => {
    findScope = numbers;
    const label = q<HTMLElement>('.find-scope');
    label.hidden = !numbers;
    label.querySelector('span')!.textContent = numbers ? `범위 【${numbers.join('】, 【')}】` : '';
  };
  /** 보정안 번호 범위 안 결과만(번호 문단). */
  const byNumbers = (matches: FindMatchDto[]) => {
    if (!findScope) return matches;
    const inside = new Set(view?.paragraphs.filter((p) => p.number && findScope!.includes(p.number)).map((p) => p.id));
    return matches.filter((match) => inside.has(match.paragraph_id));
  };
  /** 고른 범위 안 결과만(절 · 고른 곳 [from, to) 안에 통째로). */
  const byRange = (matches: FindMatchDto[]) => {
    const range = findRange;
    if (!range) return matches;
    if (range.kind === 'span') {
      const before = (a: Caret, b: Caret) => order(a.paragraph) < order(b.paragraph) || (a.paragraph === b.paragraph && a.offset <= b.offset);
      return matches.filter(
        (m) => before(range.from!, { paragraph: m.paragraph_id, offset: m.start }) && before({ paragraph: m.paragraph_id, offset: m.end }, range.to!),
      );
    }
    const sections = sectionOf(view?.paragraphs ?? []);
    return matches.filter((match) => sections.get(match.paragraph_id) === range.kind);
  };
  /** 찾기 · 모두 바꾸기가 함께 쓰는 거르기. */
  const scoped = (matches: FindMatchDto[]) => byRange(byNumbers(matches));
  /** 그 번호 문단 안에서 그 말을 찾기 막대로(「다음 찾기」로 차례로). */
  async function findIn(text: string, numbers: string[]) {
    q<HTMLInputElement>('.find-text').value = text;
    q<HTMLSelectElement>('.find-format').value = '';
    rangeSelect.value = '';
    findRange = null;
    setScope(numbers);
    q<HTMLElement>('.find-bar').hidden = false;
    await findNext();
  }
  q<HTMLInputElement>('.find-text').addEventListener('input', () => setScope(null));
  /** 걸기 뒤 남은 오기 정정(누를 때마다 다음 것). */
  let leftNotes: DescriptionNoteDto[] = [];
  let leftAt = 0;
  const leftButton = q<HTMLButtonElement>('.review-left');
  leftButton.addEventListener('click', () => {
    const note = leftNotes[leftAt % leftNotes.length];
    leftAt += 1;
    if (note) void findIn(note.wrong, note.paragraphs);
  });
  const replaceText = () => q<HTMLInputElement>('.replace-text').value;
  const replaceFormat = () => q<HTMLSelectElement>('.replace-format').value as ReplaceFormatDto;

  const FORMAT_LABEL: Record<string, string> = {
    superscript: '위첨자', subscript: '아래첨자', bold: '굵게', italic: '기울임', underline: '밑줄', plain: '서식 없는',
  };

  /** 지금 조건으로 다시 찾는다(판이 바뀌었거나 조건이 바뀌었을 때). `false` = 찾을 글이 잘못되었다(까닭은 보였다). */
  async function research(): Promise<boolean> {
    const [text, format, options] = findArgs();
    try {
      found = scoped(await editorFind(text, format, options));
    } catch (error) {
      found = [];
      foundKey = '';
      countLabel.textContent = `찾을 글 오류: ${(error as { details?: { message?: string } })?.details?.message ?? describe(error)}`;
      return false;
    }
    foundKey = findKey();
    foundView = view;
    foundAt = -1;
    return true;
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
    if ((foundKey !== findKey() || foundView !== view) && !(await research())) return;
    if (found.length === 0) {
      const what = [text && `「${text}」`, format && `${FORMAT_LABEL[format]} 글자`].filter(Boolean).join(' · ');
      const kind = findRange?.kind;
      const missing =
        (kind === 'description' || kind === 'claims') && ![...sectionOf(view.paragraphs).values()].includes(kind);
      countLabel.textContent = missing ? `이 문서에는 ${RANGE_LABEL[kind]} 절이 없습니다` : `${rangeName()}${what}: 찾는 것이 없습니다`;
      return;
    }
    foundAt = nextFrom(caret);
    show(found[foundAt]);
  }

  const rangeName = () => (findRange ? `${RANGE_LABEL[findRange.kind]} · ` : '');

  /** 결과를 쪽 위에서 고른다 — 찾기 칸에 포커스를 둔 채(문단 창을 열면 다음 Enter 가 그 창에 줄바꿈으로 들어갔다). */
  function show(match: FindMatchDto) {
    countLabel.textContent = `${rangeName()}${foundAt + 1} / ${found.length} · …${match.context}…`;
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
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'g' && view && !root.querySelector('.go-ask')) {
      event.preventDefault();
      void askGo();
      return;
    }
    if ((event.metaKey || event.ctrlKey) && event.key === 'f' && view) {
      event.preventDefault();
      q<HTMLElement>('.find-bar').hidden = false;
      q<HTMLInputElement>('.find-text').focus();
    }
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k' && !root.querySelector('.palette-shade')) {
      event.preventDefault();
      openPalette();
      return;
    }
    if ((event.metaKey || event.ctrlKey) && (event.key === ']' || event.key === '[') && view) {
      event.preventDefault();
      void jumpEdit(event.key === ']');
    }
    // 문서 배율: ⌘+(⌘=) · ⌘− · ⌘0.
    if ((event.metaKey || event.ctrlKey) && ['=', '+', '-', '0'].includes(event.key)) {
      event.preventDefault();
      void zoomBy(event.key === '0' ? 0 : event.key === '-' ? -1 : 1);
      return;
    }
    if ((event.metaKey || event.ctrlKey) && event.key === ',' && !root.querySelector('.settings-shade')) {
      event.preventDefault();
      openSettings();
    }
  });

  /** 설정: 시작할 때 읽고 · 어느 탭에서든 바꾸면(`settings-changed`) 모든 탭이 같은 값을 쓴다. */
  function applySettings(next: SettingsDto | null | undefined) {
    if (!next) return;
    const marksChanged = settings !== null && settings.marks_default !== next.marks_default;
    const first = settings === null;
    settings = next;
    autosaveMs = Math.max(1, next.autosave_minutes) * 60_000;
    setPageZoom(next.zoom);
    if (first || marksChanged) {
      showMarks = next.marks_default;
      q<HTMLButtonElement>('[data-action="marks"]').setAttribute('aria-pressed', String(showMarks));
      for (const index of pages.keys()) redraw(index);
    }
  }
  void settingsGet()
    .then(applySettings)
    .catch(() => {});
  void listen<SettingsDto>('settings-changed', (event) => applySettings(event.payload)).catch(() => {});
  // 바탕이 알리는 한 줄(이미 연 파일이라 이 탭으로 옮겼다 등).
  void listen<string>('notice', (event) => say(event.payload)).catch(() => {});

  async function saveSettings(change: Partial<SettingsDto>) {
    if (!settings) settings = await settingsGet().catch(() => null);
    if (!settings) return say('설정을 읽지 못했습니다.');
    // 바로 걸고 나서 저장한다 — 저장이 끝나기 전에 또 누른 키(⌘− 연타)가 옛 값에서 세지 않게.
    const next = { ...settings, ...change };
    applySettings(next);
    try {
      applySettings(await settingsSet(next));
    } catch (error) {
      fail('설정 저장', error);
    }
  }

  /** 문서 배율: 쪽만 다시 그린다(같은 자리를 보도록 스크롤도 같은 비율로). */
  function setPageZoom(zoom: number) {
    q<HTMLButtonElement>('.zoom-level').textContent = `${Math.round(zoom * 100)}%`;
    const next = SCALE * zoom;
    if (next === scale) return;
    const ratio = next / scale;
    scale = next;
    if (!view) return;
    const top = pagesRoot.scrollTop;
    layout(view, false);
    pagesRoot.scrollTop = top * ratio;
    paint();
  }

  async function zoomBy(step: -1 | 0 | 1) {
    const zoom = nextZoom(settings?.zoom ?? 1, step);
    await saveSettings({ zoom });
    say(`문서 배율 ${Math.round(zoom * 100)}%`);
  }

  /** 설정 창: 숫자 칸 · 켬/끔 → 저장(모든 탭에 바로). */
  function openSettings() {
    root.querySelector('.settings-shade')?.remove();
    const current = settings;
    if (!current) return say('설정을 읽는 중입니다 — 잠시 뒤 다시 여세요.');
    const shade = document.createElement('div');
    shade.className = 'basis-shade settings-shade';
    shade.innerHTML = `<form class="basis-ask settings-ask"><header><strong>설정</strong></header>
      <label>자동 저장 간격 <input name="autosave" type="number" min="1" max="60" value="${current.autosave_minutes}"> 분 — 고친 뒤 손을 멈추면 복구본을 둔다</label>
      <label>이전 판 보관 <input name="keep" type="number" min="1" max="100" value="${current.history_keep}"> 개 — 파일마다 최근 판만</label>
      <label>이전 판 보관 기간 <input name="days" type="number" min="1" max="365" value="${current.history_days}"> 일 — 지난 판은 지운다</label>
      <label>문서 배율 <input name="zoom" type="number" min="50" max="200" step="10" value="${Math.round(current.zoom * 100)}"> % — ⌘+ · ⌘− · ⌘0</label>
      <label><input name="marks" type="checkbox"${current.marks_default ? ' checked' : ''}> 문단부호(↵)를 기본으로 보이기</label>
      <div class="settings-buttons"><button type="submit">저장</button><button type="button" class="settings-cancel">취소</button></div></form>`;
    const form = shade.querySelector<HTMLFormElement>('form')!;
    const close = () => {
      shade.remove();
      input.focus({ preventScroll: true });
    };
    const number = (name: string, fallback: number) => {
      const value = Number(form.querySelector<HTMLInputElement>(`[name="${name}"]`)!.value);
      return Number.isFinite(value) && value > 0 ? value : fallback;
    };
    form.addEventListener('submit', (event) => {
      event.preventDefault();
      void saveSettings({
        autosave_minutes: Math.round(number('autosave', current.autosave_minutes)),
        history_keep: Math.round(number('keep', current.history_keep)),
        history_days: Math.round(number('days', current.history_days)),
        zoom: number('zoom', current.zoom * 100) / 100,
        marks_default: form.querySelector<HTMLInputElement>('[name="marks"]')!.checked,
      }).then(() => say('설정을 저장했습니다.'));
      close();
    });
    shade.querySelector('.settings-cancel')!.addEventListener('click', close);
    form.addEventListener('keydown', (event) => {
      if (event.key === 'Escape') close();
    });
    root.append(shade);
    form.querySelector<HTMLInputElement>('input')!.focus();
  }

  /** 상용구를 커서 자리에 — 붙여넣기와 같은 길(CP949 밖 글자 확인 · 여러 줄 = 문단 나누기). */
  function insertSnippet(text: string) {
    if (!view) return say('「상용구」: 문서를 먼저 여세요.');
    if (!caret) return say('상용구를 넣을 자리를 먼저 누르세요.');
    if (!canEdit()) return;
    act(() => insertReviewed(text));
  }

  /** 상용구 창: 넣기 · 고치기 · 지우기 · 새로(설정에 저장 — 모든 탭이 같은 목록). */
  function openSnippets() {
    root.querySelector('.snippet-shade')?.remove();
    const list = settings?.snippets ?? [];
    const escape = (text: string) => escapeHtml(text);
    const shade = document.createElement('div');
    shade.className = 'basis-shade snippet-shade';
    shade.innerHTML = `<div class="basis-ask batch-ask"><header><strong>상용구</strong><button type="button" class="snippet-new">새 상용구</button><button type="button" class="batch-close">닫기</button></header><div class="batch-body"><table><tbody>${list
      .map(
        (snippet, index) =>
          `<tr><td><strong>${escape(snippet.name)}</strong></td><td class="snippet-text">${escape(snippet.text)}</td><td><button type="button" class="snippet-insert" data-index="${index}">넣기</button><button type="button" class="snippet-edit" data-index="${index}">고치기</button><button type="button" class="snippet-delete" data-index="${index}">지우기</button></td></tr>`,
      )
      .join('')}</tbody></table>${list.length ? '' : '<p>상용구가 없습니다 — 「새 상용구」로 만드세요.</p>'}</div></div>`;
    const close = () => {
      shade.remove();
      input.focus({ preventScroll: true });
    };
    const at = (button: Element) => Number((button as HTMLElement).dataset.index);
    shade.querySelector('.batch-close')!.addEventListener('click', close);
    for (const button of shade.querySelectorAll('.snippet-insert')) {
      button.addEventListener('click', () => {
        close();
        insertSnippet(list[at(button)].text);
      });
    }
    for (const button of shade.querySelectorAll('.snippet-delete')) {
      button.addEventListener('click', () => {
        void saveSettings({ snippets: list.filter((_, index) => index !== at(button)) }).then(openSnippets);
      });
    }
    const edit = (index: number | null) => {
      const old = index === null ? { name: '', text: '' } : list[index];
      const form = document.createElement('form');
      form.className = 'snippet-form';
      form.innerHTML = `<label>이름 <input name="name" value="${escape(old.name)}" required></label><label>문구 <textarea name="text" rows="5" required>${escape(old.text)}</textarea></label><div class="settings-buttons"><button type="submit">저장</button><button type="button" class="snippet-cancel">취소</button></div>`;
      form.addEventListener('submit', (event) => {
        event.preventDefault();
        const name = form.querySelector<HTMLInputElement>('[name="name"]')!.value.trim();
        const text = form.querySelector<HTMLTextAreaElement>('[name="text"]')!.value;
        if (!name || !text) return;
        const next = index === null ? [...list, { name, text }] : list.map((snippet, k) => (k === index ? { name, text } : snippet));
        void saveSettings({ snippets: next }).then(openSnippets);
      });
      form.querySelector('.snippet-cancel')!.addEventListener('click', () => form.remove());
      shade.querySelector('.batch-body')!.prepend(form);
      form.querySelector<HTMLInputElement>('input')!.focus();
    };
    shade.querySelector('.snippet-new')!.addEventListener('click', () => edit(null));
    for (const button of shade.querySelectorAll('.snippet-edit')) button.addEventListener('click', () => edit(at(button)));
    shade.addEventListener('keydown', (event) => {
      if (event.key === 'Escape') close();
    });
    root.append(shade);
  }

  /** 이전 판 창: 저장이 덮기 전에 남긴 판(새것부터) → 고른 판을 새 탭에서 이름 없는 문서로. */
  async function openHistory() {
    let kept;
    try {
      kept = await editorHistoryList();
    } catch (error) {
      return fail('이전 판', error);
    }
    if (!kept.length) {
      return say(`이전 판이 없습니다 — 저장할 때마다 덮기 전 판을 남깁니다(파일마다 최근 ${settings?.history_keep ?? 10}개 · ${settings?.history_days ?? 30}일).`);
    }
    root.querySelector('.history-shade')?.remove();
    const shade = document.createElement('div');
    shade.className = 'basis-shade history-shade';
    shade.innerHTML = `<div class="basis-ask batch-ask"><header><strong>이전 판</strong><button type="button" class="batch-close">닫기</button></header><div class="batch-body"><p>저장할 때 덮인 판입니다. 「열기」는 새 탭에 이름 없는 문서로 엽니다 — 원본은 그대로이고, 저장하면 이름을 묻습니다.</p><table><thead><tr><th>저장한 때</th><th>크기</th><th></th></tr></thead><tbody>${kept
      .map(
        (item, index) =>
          `<tr><td>${escapeHtml(item.saved)}</td><td>${Math.max(1, Math.round(item.size / 1024)).toLocaleString()} KB</td><td><button type="button" class="history-open" data-index="${index}">열기</button></td></tr>`,
      )
      .join('')}</tbody></table></div></div>`;
    shade.querySelector('.batch-close')!.addEventListener('click', () => shade.remove());
    for (const button of shade.querySelectorAll<HTMLButtonElement>('.history-open')) {
      button.addEventListener('click', async () => {
        shade.remove();
        try {
          const opened = await editorHistoryOpen(Number(button.dataset.index));
          if (opened) {
            images.clear();
            panel.hidden = true;
            layout(opened, false);
          }
        } catch (error) {
          fail('이전 판 열기', error);
        }
      });
    }
    root.append(shade);
  }

  /** 명령 찾기(⌘K): 툴바 단추 · 「도구」 메뉴를 이름 · 설명으로 거르고 ↑↓ · Enter 로 실행(꺼진 것은 흐리게 · 실행 안 함). */
  function openPalette() {
    root.querySelector('.palette-shade')?.remove();
    const commands = [
      ...[...root.querySelectorAll<HTMLButtonElement>('.editor-toolbar [data-action], .zoom-box [data-action]')].map((button) => ({
        // 메뉴가 열리는 단추의 「▾」는 이름이 아니다 · 아이콘뿐인 단추(문서 배율)는 aria-label.
        label: (button.querySelector('span')?.textContent ?? button.getAttribute('aria-label') ?? button.textContent ?? '').replace(/\s*▾$/, '').trim(),
        hint: button.title,
        enabled: !button.disabled,
        run: () => button.click(),
      })),
      ...TOOLS.map((tool) => ({ label: tool.label, hint: '도구', enabled: Boolean(view), run: tool.run })),
      ...(settings?.snippets ?? []).map((snippet) => ({
        label: `상용구: ${snippet.name}`,
        hint: snippet.text,
        enabled: Boolean(view),
        run: () => insertSnippet(snippet.text),
      })),
    ].filter((command) => command.label);
    const shade = document.createElement('div');
    shade.className = 'basis-shade palette-shade';
    shade.innerHTML = '<div class="palette"><input class="palette-text" type="text" placeholder="명령 이름 · 설명(예: 대비표, 일괄, 원문)" aria-label="명령 찾기"><ul class="palette-list"></ul></div>';
    const field = shade.querySelector<HTMLInputElement>('.palette-text')!;
    const list = shade.querySelector<HTMLUListElement>('.palette-list')!;
    let shown: typeof commands = [];
    let active = 0;
    const close = () => {
      shade.remove();
      input.focus({ preventScroll: true });
    };
    const runAt = (index: number) => {
      const command = shown[index];
      if (!command) return;
      if (!command.enabled) {
        say(`「${command.label}」: 지금은 쓸 수 없습니다(문서를 먼저 여세요).`);
        return;
      }
      close();
      command.run();
    };
    const render = () => {
      const query = field.value.trim().toLowerCase();
      // 이름에 든 것을 먼저, 설명에만 든 것은 뒤에(「통계」 → 「문서 통계」가 「도구」보다 앞).
      const named = commands.filter((command) => command.label.toLowerCase().includes(query));
      shown = [...named, ...commands.filter((command) => !named.includes(command) && command.hint.toLowerCase().includes(query))];
      active = Math.min(active, Math.max(0, shown.length - 1));
      list.replaceChildren(
        ...shown.map((command, index) => {
          const item = document.createElement('li');
          item.className = `${index === active ? 'active' : ''}${command.enabled ? '' : ' off'}`.trim();
          item.textContent = command.label;
          item.title = command.hint;
          item.addEventListener('click', () => runAt(index));
          return item;
        }),
      );
    };
    field.addEventListener('input', () => {
      active = 0;
      render();
    });
    field.addEventListener('keydown', (event) => {
      if (event.key === 'Escape') close();
      else if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
        event.preventDefault();
        active = (active + (event.key === 'ArrowDown' ? 1 : shown.length - 1)) % Math.max(1, shown.length);
        render();
      } else if (event.key === 'Enter') {
        event.preventDefault();
        runAt(active);
      }
    });
    shade.addEventListener('click', (event) => {
      if (event.target === shade) close();
    });
    root.append(shade);
    render();
    field.focus();
  }

  /** 고친 곳 사이 이동(⌘] 다음 · ⌘[ 앞): 연 뒤 고친 문단(보정 표시 「연 뒤 고친 것」)을 문서 차례로 · 끝에서는 처음으로. */
  async function jumpEdit(forward: boolean) {
    await commit();
    const marks = await editorAmendmentMarks(false, false).catch(() => null);
    const ids = [...new Set((marks?.paragraphs ?? []).map((mark) => mark.id))]
      .filter((id) => order(id) >= 0)
      .sort((a, b) => order(a) - order(b));
    if (ids.length === 0) {
      say('연 뒤 고친 곳이 없습니다.');
      return;
    }
    const here = caret ? order(caret.paragraph) : forward ? -1 : Number.MAX_SAFE_INTEGER;
    const at = forward
      ? ids.findIndex((id) => order(id) > here)
      : ids.map((id) => order(id) < here).lastIndexOf(true);
    const index = at >= 0 ? at : forward ? 0 : ids.length - 1;
    jumpTo(ids[index]);
    say(`고친 곳 ${index + 1}/${ids.length}${at < 0 ? (forward ? ' (처음으로 돌아왔습니다)' : ' (끝으로 갔습니다)') : ''}`);
  }

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
  // 일괄 처리 진행: 「n/N · 파일」 + 멈춤.
  const batchBar = document.createElement('div');
  batchBar.className = 'batch-progress';
  batchBar.hidden = true;
  batchBar.innerHTML = '<span></span><button type="button">멈춤</button>';
  batchBar.querySelector('button')!.addEventListener('click', () => {
    void editorBatchStop();
    batchBar.querySelector('span')!.textContent = '멈추는 중(지금 파일을 끝내고)…';
  });
  root.append(batchBar);
  void listen<{ at: number; total: number; file: string }>('batch-progress', (event) => {
    batchBar.hidden = false;
    batchBar.querySelector('span')!.textContent = `일괄 ${event.payload.at + 1}/${event.payload.total} · ${event.payload.file}`;
  });

  /** 일괄 처리 결과 창: 표 · CSV 로 저장 · 닫기. */
  function showBatch(title: string, header: string[], rows: string[][], csvName: string) {
    const escape = (text: string) =>
      text.replace(/[&<>"]/g, (ch) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[ch]!);
    const shade = document.createElement('div');
    shade.className = 'basis-shade batch-shade';
    shade.innerHTML = `<div class="basis-ask batch-ask"><header><strong>${escape(title)}</strong><button type="button" class="batch-csv">CSV 로 저장</button><button type="button" class="batch-close">닫기</button></header><div class="batch-body"><table><thead><tr>${header
      .map((cell) => `<th>${escape(cell)}</th>`)
      .join('')}</tr></thead><tbody>${rows
      .map((row) => `<tr>${row.map((cell) => `<td>${escape(cell)}</td>`).join('')}</tr>`)
      .join('')}</tbody></table></div></div>`;
    shade.querySelector('.batch-close')!.addEventListener('click', () => shade.remove());
    shade.querySelector('.batch-csv')!.addEventListener('click', async () => {
      const path = await editorSaveCsv(csvName, toCsv([header, ...rows])).catch((error) => (fail('CSV 저장', error), null));
      if (path) say(`저장함: ${path}`);
    });
    root.append(shade);
  }

  /** 바뀐 곳만 앞뒤 조금(번호 다시 매기기 미리 보기). */
  const fragment = (before: string, after: string) => {
    const a = Array.from(before);
    const b = Array.from(after);
    let head = 0;
    while (head < a.length && head < b.length && a[head] === b[head]) head += 1;
    let tail = 0;
    while (tail < a.length - head && tail < b.length - head && a[a.length - 1 - tail] === b[b.length - 1 - tail]) tail += 1;
    const from = Math.max(0, head - 8);
    const cut = (chars: string[]) => `${from > 0 ? '…' : ''}${chars.slice(from, chars.length - tail + 8).join('')}${tail > 8 ? '…' : ''}`;
    return [cut(a), cut(b)];
  };

  /** 번호 다시 매기기: 미리 보기 → 적용(되돌리기 한 번). */
  async function renumber(figures: boolean) {
    if (!view) return;
    await commit();
    const what = figures ? '도면' : '청구항';
    const edits = await editorRenumberPlan(figures);
    if (edits.length === 0) {
      say(`${what} 번호가 이미 차례대로입니다.`);
      return;
    }
    const escape = (text: string) =>
      text.replace(/[&<>"]/g, (ch) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[ch]!);
    // 최종본 · 보정 명세서: 보정에서는 지운 항 · 도면 번호를 다시 매기지 않는 것이 실무다.
    const amendment = view.paragraphs.some((paragraph) => paragraph.number);
    const shade = document.createElement('div');
    shade.className = 'basis-shade renumber-shade';
    shade.innerHTML = `<div class="basis-ask batch-ask"><header><strong>${what} 번호 다시 매기기 — ${edits.length}곳</strong><button type="button" class="renumber-apply">적용</button><button type="button" class="batch-close">취소</button></header>${
      amendment
        ? `<p class="renumber-warn">최종본(보정) 문서입니다. 보정에서는 지운 ${what}의 번호를 다시 매기지 않고 「삭제」로 남기는 것이 실무입니다. 새로 쓰는 명세서에만 쓰세요.</p>`
        : ''
    }<div class="batch-body"><table><thead><tr><th>바꾸기 전</th><th>바꾼 뒤</th></tr></thead><tbody>${edits
      .map((edit) => {
        const [before, after] = fragment(edit.before, edit.after);
        return `<tr><td>${escape(before)}</td><td>${escape(after)}</td></tr>`;
      })
      .join('')}</tbody></table></div></div>`;
    shade.querySelector('.batch-close')!.addEventListener('click', () => shade.remove());
    shade.querySelector('.renumber-apply')!.addEventListener('click', () => {
      shade.remove();
      void run(`${what} 번호 다시 매기기`, () => editorReplaceTexts(edits.map((edit) => [edit.id, edit.after])));
    });
    root.append(shade);
  }

  /** 표 창(부호 목록 · 되풀이 문장 · 문서 통계): 제목 · 머리 · 줄(마지막 칸이 문단 id 면 「가기」). */
  function showTable(title: string, header: string[], rows: { cells: string[]; go?: string | null }[], className: string) {
    const escape = (text: string) =>
      text.replace(/[&<>"]/g, (ch) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[ch]!);
    root.querySelector(`.${className}`)?.remove();
    const shade = document.createElement('div');
    shade.className = `basis-shade ${className}`;
    const withGo = rows.some((row) => row.go);
    shade.innerHTML = `<div class="basis-ask batch-ask"><header><strong>${escape(title)}</strong><button type="button" class="batch-close">닫기</button></header><div class="batch-body"><table><thead><tr>${header
      .map((cell) => `<th>${escape(cell)}</th>`)
      .join('')}${withGo ? '<th></th>' : ''}</tr></thead><tbody>${rows
      .map(
        (row, index) =>
          `<tr>${row.cells.map((cell) => `<td>${escape(cell)}</td>`).join('')}${
            withGo ? `<td>${row.go ? `<button type="button" class="table-go" data-index="${index}">가기</button>` : ''}</td>` : ''
          }</tr>`,
      )
      .join('')}</tbody></table></div></div>`;
    shade.querySelector('.batch-close')!.addEventListener('click', () => shade.remove());
    for (const button of shade.querySelectorAll<HTMLButtonElement>('.table-go')) {
      button.addEventListener('click', () => {
        shade.remove();
        jumpTo(rows[Number(button.dataset.index)].go!);
      });
    }
    root.append(shade);
  }

  async function signList() {
    if (!view) return;
    await commit();
    const rows = await editorSignTable();
    if (rows.length === 0) {
      say('부호가 없습니다(상세설명의 「이름(부호)」 · 【부호의 설명】).');
      return;
    }
    showTable(
      `부호 목록 — ${rows.length}개`,
      ['부호', '본문 이름(쓰인 수)', '【부호의 설명】', '쓰인 수'],
      rows.map((row) => ({
        cells: [
          row.sign,
          row.names.map(([name, count]) => `${name}(${count})`).join(' · '),
          row.listed ?? '',
          String(row.count),
        ],
        go: row.first,
      })),
      'sign-shade',
    );
  }

  /** 되풀이 문장: 상세설명에서 두 번 넘게 나온 문장 · 본문이 같은 청구항. */
  async function repeatedList() {
    if (!view) return;
    await commit();
    const found = repeatedSentences(view.paragraphs);
    if (found.length === 0) {
      say('되풀이된 문장이 없습니다(상세설명 20자 넘는 문장 · 청구항 본문).');
      return;
    }
    // 자리 이름: 식별번호(제 것 · 매겨질 것) · 청구항 표제 · 문단 차례.
    const place = (id: string) => {
      const paragraph = paragraphOf(id);
      const number = paragraph?.number ?? planned.get(id);
      if (number) return `【${number}】`;
      if (paragraph && /^\s*【청구항 /.test(paragraph.text)) return paragraph.text.trim();
      return `문단 ${order(id) + 1}`;
    };
    const short = (text: string) => (Array.from(text).length > 70 ? `${Array.from(text).slice(0, 70).join('')}…` : text);
    showTable(
      `되풀이 문장 — ${found.length}개`,
      ['문장', '횟수', '나온 곳'],
      found.map((item) => ({
        cells: [item.claim ? `(청구항 본문) ${short(item.text)}` : short(item.text), String(item.at.length), item.at.map(place).join(', ')],
        go: item.at[0],
      })),
      'repeated-shade',
    );
  }

  /** 도움말: 기능 투어 목록 · 단축키 모음. */
  function openHelp() {
    // 도움말을 열었으면 처음 실행 권유는 할 일을 다 했다.
    root.querySelector<HTMLButtonElement>('.tour-invite-close')?.click();
    openTourList(root, () =>
      showTable('단축키 모음', ['단축키', '하는 일'], SHORTCUTS.map((cells) => ({ cells })), 'shortcut-shade'),
    );
  }

  /** 「도구」 메뉴 · ⌘K 명령 찾기가 함께 쓰는 목록. */
  const TOOLS: { label: string; run: () => void }[] = [
    { label: '청구항 번호 다시 매기기…', run: () => void renumber(false) },
    { label: '도면 번호 다시 매기기…', run: () => void renumber(true) },
    { label: '부호 목록', run: () => void signList() },
    { label: '되풀이 문장 찾기', run: () => void repeatedList() },
    {
      label: '문서 통계',
      run: () => {
        if (!view) return;
        const rows = documentStats(view.paragraphs, view.page_count, claimParents);
        showTable('문서 통계', ['항목', '값'], rows.map((cells) => ({ cells })), 'stats-shade');
      },
    },
    { label: '다음 고친 곳 (⌘])', run: () => void jumpEdit(true) },
    { label: '앞 고친 곳 (⌘[)', run: () => void jumpEdit(false) },
  ];

  async function runBatch(kind: 'generate' | 'check') {
    try {
      if (kind === 'generate') {
        const rows = await editorBatchGenerate();
        if (!rows) return;
        const count = (status: string) => rows.filter((row) => row.status === status).length;
        say(`폴더 HLT 만들기 — 만듦 ${count('만듦')} · 있음 ${count('있음')} · 실패 ${count('실패')}${count('멈춤') ? ` · 멈춤 ${count('멈춤')}` : ''}`);
        showBatch('폴더 HLT 만들기', ['파일', '결과', '내용'], rows.map((row) => [row.file, row.status, row.detail]), 'HLT 만들기.csv');
      } else {
        const rows = await editorBatchCheck();
        if (!rows) return;
        const total = (row: (typeof rows)[number]) => Object.values(row.counts).reduce((sum, n) => sum + (n ?? 0), 0);
        say(`폴더 제출 전 점검 — ${rows.length}개 파일 · 걸린 파일 ${rows.filter((row) => total(row) > 0).length}`);
        showBatch(
          '폴더 제출 전 점검',
          ['파일', ...SUBMIT_GROUPS.map(([, name]) => name.replace(/\(.*\)$/, '')), '합계', '열지 못함'],
          rows.map((row) => [row.file, ...SUBMIT_GROUPS.map(([kind]) => String(row.counts[kind] ?? 0)), String(total(row)), row.error ?? '']),
          '제출 전 점검.csv',
        );
      }
    } catch (error) {
      fail('일괄', error);
    } finally {
      batchBar.hidden = true;
    }
  }

  // 메뉴 막대(한국어): 항목 id 가 할 일 — 툴바 단추 · 도구 · 일괄 · 도움말 · 가기 · 명령 찾기.
  void listen<string>('menu-action', (event) => {
    const id = event.payload;
    const needDocument = (name: string) => say(`「${name}」: 문서를 먼저 여세요.`);
    if (id.startsWith('action:')) {
      const button = root.querySelector<HTMLButtonElement>(`[data-action="${id.slice(7)}"]`);
      if (!button) return;
      if (button.disabled) needDocument(button.title.split(' — ')[0]);
      else button.click();
    } else if (id.startsWith('tool:')) {
      const tool = TOOLS.find((each) => each.label === id.slice(5));
      if (!view) needDocument(id.slice(5));
      else tool?.run();
    } else if (id === 'batch:generate' || id === 'batch:check') void runBatch(id === 'batch:generate' ? 'generate' : 'check');
    else if (id === 'help:tours') openHelp();
    else if (id === 'help:shortcuts') showTable('단축키 모음', ['단축키', '하는 일'], SHORTCUTS.map((cells) => ({ cells })), 'shortcut-shade');
    else if (id === 'palette') openPalette();
    else if (id === 'go') {
      if (view) void askGo();
      else needDocument('가기');
    }
  });

  // 처음 실행: 기능 투어를 한 번 권한다(보거나 닫으면 다시 안 묻는다).
  const toured = (() => {
    try {
      return localStorage.getItem(TOUR_SEEN) === '1';
    } catch {
      return true;
    }
  })();
  if (!toured) {
    const invite = document.createElement('div');
    invite.className = 'tour-invite';
    invite.innerHTML = '<span>처음이세요? 화면을 차례로 짚어 주는 기능 투어를 볼 수 있습니다.</span><button type="button" class="tour-invite-go">투어 보기</button><button type="button" class="tour-invite-close">닫기</button>';
    const done = () => {
      invite.remove();
      try {
        localStorage.setItem(TOUR_SEEN, '1');
      } catch {
        // 저장 못 해도 이번엔 닫는다.
      }
    };
    invite.querySelector('.tour-invite-go')!.addEventListener('click', () => {
      done();
      runTour(root, TOURS[0]);
    });
    invite.querySelector('.tour-invite-close')!.addEventListener('click', done);
    root.append(invite);
  }

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
      case 'amend-marks': {
        showAmend = !showAmend;
        q<HTMLButtonElement>('[data-action="amend-marks"]').setAttribute('aria-pressed', String(showAmend));
        if (showAmend) await refreshAmend(true);
        else {
          amendMarks = new Map();
          for (const index of pages.keys()) redraw(index);
          paint();
          say('');
        }
        break;
      }
      case 'basis': {
        try {
          const basis = await editorAmendmentBasis(fromOriginal());
          if (!basis) {
            say('보정 근거는 최종본(.fin)으로 연 문서에서만 찾을 수 있습니다.');
            break;
          }
          if (basis.length === 0) {
            say('청구항에 새로 넣은 말이 없습니다.');
            break;
          }
          const action = await askBasis(basis);
          if (action?.kind === 'copy') {
            await navigator.clipboard.writeText(action.text);
            say('보정 근거 초안을 복사했습니다 — 의견서에 붙여 넣으세요.');
          } else if (action?.kind === 'opinion') {
            // 이 탭에 문서가 있으면 새 탭으로 연다(null) — 빈 탭이면 여기서.
            const opened = await editorNewOpinion(action.text);
            if (opened) {
              images.clear();
              panel.hidden = true;
              layout(opened, false);
            }
          }
        } catch (error) {
          fail('보정 근거', error);
        }
        break;
      }
      case 'new-tab':
        await editorNewTab().catch((error) => fail('새 탭', error));
        break;
      case 'open-read-only': {
        try {
          const opened = await editorOpenReadOnly();
          if (opened) {
            images.clear();
            panel.hidden = true;
            layout(opened, false);
          }
        } catch (error) {
          fail('읽기 전용으로 열기', error);
        }
        break;
      }
      case 'history':
        await openHistory();
        break;
      case 'settings':
        openSettings();
        break;
      case 'snippets':
        openSnippets();
        break;
      case 'zoom-in':
      case 'zoom-out':
      case 'zoom-reset':
        await zoomBy(action === 'zoom-reset' ? 0 : action === 'zoom-out' ? -1 : 1);
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
      case 'source': {
        if (!sourcePane.hidden) {
          closeSource();
          break;
        }
        try {
          const rows = await editorSourceOpen();
          if (!rows) break;
          openSource(rows);
          if (view) await refreshKeys(view);
          const paired = rows.filter((row) => row.key && docKeys.includes(row.key)).length;
          say(`원문 ${rows.filter((row) => row.key).length}곳 중 ${paired}곳 짝지음(식별번호 · 청구항 번호)`);
        } catch (error) {
          fail('원문 열기', error);
        }
        break;
      }
      case 'new-menu': {
        const menu = await Menu.new({
          items: [
            { text: '새 의견서', action: () => q<HTMLButtonElement>('[data-action="new-opinion"]').click() },
            { text: '워드/XML로 HLT 만들기…', action: () => q<HTMLButtonElement>('[data-action="import"]').click() },
            { text: '새 탭', accelerator: 'CmdOrCtrl+T', action: () => q<HTMLButtonElement>('[data-action="new-tab"]').click() },
          ],
        });
        await menu.popup();
        break;
      }
      case 'help': {
        openHelp();
        break;
      }
      case 'tools': {
        const menu = await Menu.new({ items: TOOLS.map((tool) => ({ text: tool.label, action: tool.run })) });
        await menu.popup();
        break;
      }
      case 'batch': {
        const menu = await Menu.new({
          items: [
            { text: '폴더 HLT 만들기…', action: () => void runBatch('generate') },
            { text: '폴더 제출 전 점검…', action: () => void runBatch('check') },
          ],
        });
        await menu.popup();
        break;
      }
      case 'import': {
        say('HLT 만드는 중…');
        try {
          const result = await editorImport();
          let opened = result.view;
          if (result.review.length > 0) {
            say('쓸 수 없는 글자 확인 중…');
            const choices = await reviewCp949(result.review, 'Word · XML 불러오기');
            say(choices ? 'HLT 만드는 중…' : '');
            opened = await editorImportFinish(choices);
          }
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
          // 읽기 전용 문서의 사본을 만들었으면 그 사본을 고치는 문서가 됐다 — 고칠 수 있게 화면을 다시 받는다.
          if (path && action === 'save-as' && view?.read_only) layout(await editorView(), true);
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
      case 'docx': {
        try {
          const path = await editorExportDocx();
          if (path) say(`Word: ${path}`);
        } catch (error) {
          fail(`Word`, error);
        }
        break;
      }
      case 'fin': {
        try {
          const issues = await editorSubmissionChecks();
          if (issues.length > 0 && !(await askSubmit(issues))) {
            say('제출파일 만들기를 멈췄습니다 — 고친 뒤 다시 누르세요.');
            break;
          }
        } catch (error) {
          fail('제출 전 점검', error);
          break;
        }
        say('제출파일 만드는 중…');
        try {
          const path = await editorExportFin();
          say(path ? `제출파일: ${path}` : '');
        } catch (error) {
          fail('제출파일', error);
        }
        break;
      }
      case 'review': {
        if (!canEdit()) break;
        try {
          const report = await editorReadReview();
          if (!report) break;
          if (report.proposals.length === 0 && report.notes.length === 0 && report.figures.length === 0) {
            say('보고서에서 보정안(「보정안」 표의 【청구항】)과 오기 정정 문장을 찾지 못했습니다.');
            break;
          }
          leftButton.hidden = true;
          const picked = await askReview(report);
          if (!picked) break;
          let outcome: ReviewOutcomeDto | null = null;
          await structural('보정안 불러오기', async () => {
            const done = await editorApplyReview(picked.claims, picked.tracked, picked.notes);
            outcome = done.outcome;
            return { next: done.view, caret: null };
          });
          if (outcome) say(reviewSummary(outcome, report.figures.length));
          // 걸지 않은 정정 가운데 아직 그 자리에 남은 것 → 「남은 오기 N곳」.
          const skipped = report.notes.filter((note) => !picked.notes.some((p) => p.wrong === note.wrong && p.paragraphs.join() === note.paragraphs.join()));
          leftNotes = [];
          let places = 0;
          for (const note of skipped) {
            setScope(note.paragraphs);
            const count = byNumbers(await editorFind(note.wrong, null)).length;
            if (count > 0) leftNotes.push(note);
            places += count;
          }
          setScope(null);
          leftAt = 0;
          leftButton.textContent = `남은 오기 ${places}곳`;
          leftButton.hidden = leftNotes.length === 0;
        } catch (error) {
          fail('보정안', error);
        }
        break;
      }
      case 'amend': {
        // 통특기와 같게: 최종본에서만 · 고친 것이 없으면 「변경된 내용이 없습니다.」 · 끝나면 읽기 전용 보정서.
        try {
          const issues = await editorClaimIssues();
          if (issues.length > 0 && !(await askCitations(issues))) {
            say('보정서 만들기를 멈췄습니다 — 청구항 인용을 고친 뒤 다시 누르세요.');
            break;
          }
          say('보정서 만드는 중…');
          const done = await editorExportAmendment();
          if (done) {
            say(`보정서(${done.order}차) 생성작업을 완료하였습니다. ${done.dta} · 최종본 갱신 ${done.fin}${done.table ? ` · 대비표 ${done.table}` : ''}`);
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
        if (inCell(at.paragraph)) break;
        const align = action.slice('align-'.length) as Alignment;
        await structural('정렬', async () => ({ next: await editorSetAlignment(at.paragraph, align), caret: at }));
        break;
      }
      case 'indent':
      case 'outdent': {
        if (!caret || !canEdit()) {
          say('들여쓰기: 쪽에서 문단을 먼저 누르세요.');
          break;
        }
        const at = caret;
        if (inCell(at.paragraph)) break;
        const indent = action === 'indent' ? BODY_INDENT : 0;
        await structural(action === 'indent' ? '들여쓰기' : '들여쓰기 취소', async () => ({
          next: await editorSetIndent(at.paragraph, indent),
          caret: at,
        }));
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
        setScope(null);
        break;
      case 'find-unscope':
        setScope(null);
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
        const pattern = regexPattern();
        await run('바꾸기', () => editorReplace([current!], replacement, format, pattern));
        // 서식만 바꾸면(바꿀 글 비움) 글 길이 그대로 · 정규식은 바꾼 글 길이를 모르니 그 자리에서 다시 찾는다.
        const length = pattern ? 0 : replacement === '' && format !== 'keep' ? current!.end - current!.start : Array.from(replacement).length;
        caret = anchor = { paragraph: current!.paragraph_id, offset: current!.start + length };
        await findNext();
        break;
      }
      case 'replace-all': {
        const all = scoped(await editorFind(...findArgs()));
        if (all.length === 0) {
          countLabel.textContent = '찾는 것이 없습니다';
          break;
        }
        await run(`모두 바꾸기(${all.length}곳)`, () => editorReplace(all, replaceText(), replaceFormat(), regexPattern()));
        foundKey = '';
        foundAt = -1;
        anchor = caret;
        paint();
        countLabel.textContent = `${rangeName()}${all.length}곳 바꿈`;
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
/** 이어진 번호를 범위로(【0070】~【0076】 · 도 9~13) — 숫자가 아닌 번호(3a)는 따로. */
function ranges(items: string[], wrap: (item: string) => string): string {
  const sorted = [...new Set(items)].sort((a, b) => (parseInt(a, 10) || 0) - (parseInt(b, 10) || 0) || a.localeCompare(b));
  const out: string[] = [];
  for (let i = 0; i < sorted.length; ) {
    let j = i;
    while (j + 1 < sorted.length && /^\d+$/.test(sorted[j]) && /^\d+$/.test(sorted[j + 1]) && Number(sorted[j + 1]) === Number(sorted[j]) + 1) j += 1;
    out.push(j > i ? `${wrap(sorted[i])}~${wrap(sorted[j])}` : wrap(sorted[i]));
    i = j + 1;
  }
  return out.join(', ');
}

/** 숫자 끝 읽기의 받침으로 고른 조사(1 일 → 은 · 2 이 → 는 · 10 십 → 은). */
const topic = (number: number) => ('2459'.includes(String(number).slice(-1)) ? '는' : '은');

/** 보정 근거 초안(실제 의견서 「보정 근거」 꼴): 청구항마다 근거 단락 · 도면 한 문장 + 단락 인용 줄 · 끝 문장. */
/** ⌘G 가기 대상 문단 id: 청구항(「청구항 7」 · 「제7항」 · 「7항」) · 도(「도 3a」) · 식별번호(「49」 · 「【0049】」) · 표제 이름. */
export function goTarget(
  text: string,
  paragraphs: { id: string; text: string; wipo_id: number | null; number?: string | null }[],
): string | null {
  const query = text.trim().replace(/\s+/g, ' ');
  const heading = (label: string) => paragraphs.find((p) => p.text.trim().replace(/\s+/g, ' ') === `【${label}】`)?.id ?? null;
  const claim = /^(?:청구항 ?|제 ?)?(\d+)(?: ?항)?$/.exec(query);
  if (claim && (/청구항|항/.test(query))) return heading(`청구항 ${claim[1]}`);
  const figure = /^도 ?(\d+[a-zA-Z]?)$/.exec(query);
  if (figure) return heading(`도 ${figure[1]}`);
  const number = /^[【[]?(\d{1,4})[】\]]?$/.exec(query);
  if (number) {
    const four = number[1].padStart(4, '0');
    const isHeading = (p: { text: string }) => /^\s*【[^】]+】\s*$/.test(p.text);
    return (
      paragraphs.find((p) => p.number === four)?.id ??
      paragraphs.find((p) => p.wipo_id === Number(four) && !isHeading(p))?.id ??
      null
    );
  }
  return query ? heading(query.replace(/^【|】$/g, '')) : null;
}

export function basisDraft(basis: ClaimBasisDto[]): string {
  const lines: string[] = [];
  for (const claim of basis) {
    const hits = claim.phrases.flatMap((phrase) => phrase.hits);
    const subject = claim.added
      ? `새로 추가한 청구항 ${claim.claim}${topic(claim.claim)}`
      : `청구항 ${claim.claim}의 보정 사항(${claim.phrases.map((phrase) => `“${phrase.text}”`).join(', ')})은`;
    if (hits.length === 0) {
      lines.push(`${subject} (근거 단락을 고르지 않았습니다 — 직접 쓰세요.)`);
      continue;
    }
    const figures = hits.flatMap((hit) => hit.figures);
    const where = `단락 ${ranges(hits.map((hit) => hit.number), (n) => `【${n}】`)}${figures.length ? ` 및 도 ${ranges(figures, (n) => n)}` : ''}`;
    lines.push(`${subject} 원출원 명세서의 ${where}에 기재된 내용에 근거한 것입니다.`);
    const quoted = new Set<string>();
    for (const hit of hits) {
      if (quoted.has(hit.number)) continue;
      quoted.add(hit.number);
      lines.push(`- 【${hit.number}】: "${hit.quote}"`);
    }
    lines.push('');
  }
  lines.push('따라서 본 보정은 신규사항 추가에 해당하지 않습니다.');
  return lines.join('\n');
}

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

/** CP949 에 없을 수도 있는 글자(ASCII · 한글 음절 · 호환 자모 밖) — 있을 때만 바탕에 물어본다. */
const CP949_SUSPECT = /[^\x00-\x7F\uAC00-\uD7A3\u3131-\u318E]/;

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
      heading_indent: '식별항목 표제는 들여쓸 수 없습니다(저장할 때 들여쓰기 0 으로 맞춰집니다)',
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
      open_elsewhere: '그 파일은 다른 탭에 열려 있습니다 — 그 탭에서 저장하거나 닫은 뒤 하세요',
      read_only_save: '읽기 전용으로 연 문서입니다 — 「다른 이름으로 저장」으로 사본을 만드세요',
      history_none: '이전 판이 없습니다',
      'save_verify:bytes': '저장한 파일을 다시 읽어 보니 쓴 것과 다릅니다(동기화 폴더 · 백신이 바꿨을 수 있습니다). 고친 것은 그대로 있으니 다른 자리에 다시 저장하세요 — 덮인 판은 「이전 판」에 있습니다',
      'save_verify:structure': '저장한 파일이 검증을 통과하지 못했습니다. 고친 것은 그대로 있으니 다시 저장하세요 — 덮인 판은 「이전 판」에 있습니다',
      'save_verify:read': '저장한 파일을 다시 읽지 못했습니다. 고친 것은 그대로 있으니 다시 저장하세요 — 덮인 판은 「이전 판」에 있습니다',
      'save_verify:text': '저장한 파일의 글이 화면과 다릅니다. 고친 것은 그대로 있으니 다시 저장하세요 — 덮인 판은 「이전 판」에 있습니다',
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
