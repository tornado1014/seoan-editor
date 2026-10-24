// 기능 투어: 실제 화면 위에서 그 기능의 단추 · 칸을 차례로 비추고(나머지는 어둡게) 말풍선으로 설명한다.
// 투어는 데이터(단계 목록) · 엔진은 하나. 투어는 문서를 고치지 않는다 — 단계 준비로 연 것(찾기 막대 등)은 끝낼 때 되돌린다.

export interface TourStep {
  /** 비출 화면 요소(선택자). 없거나 보이지 않으면 말풍선만 가운데에. */
  target: string;
  title: string;
  text: string;
  /** 단계 전 준비(예: 찾기 막대 열기) — 되돌리는 함수를 돌려주면 다음 단계 · 끝낼 때 부른다. */
  prepare?: (root: HTMLElement) => (() => void) | void;
}

export interface Tour {
  id: string;
  group: string;
  title: string;
  summary: string;
  steps: TourStep[];
}

/** 찾기 막대를 열어 두고(원래 닫혀 있었으면) 끝낼 때 닫는다. */
const openFindBar = (root: HTMLElement) => {
  const bar = root.querySelector<HTMLElement>('.find-bar');
  if (!bar || !bar.hidden) return;
  bar.hidden = false;
  return () => {
    bar.hidden = true;
  };
};

export const TOURS: Tour[] = [
  {
    id: 'overview',
    group: '처음이라면',
    title: '전체 둘러보기',
    summary: '툴바 묶음과 화면 구성을 차례로',
    steps: [
      { target: '.tb-group[aria-label="파일"]', title: '파일', text: '열기 · 새로 만들기(새 의견서 · 워드/XML로 HLT 만들기 · 새 탭) · 저장 · 닫기. 최근 문서는 메뉴 「파일」에 있습니다.' },
      { target: '.tb-group[aria-label="편집"]', title: '편집', text: '되돌리기(⌘Z) · 다시 하기(⇧⌘Z) · 찾기 · 바꾸기(⌘F). 식별번호 · 청구항으로 가기는 ⌘G 입니다.' },
      { target: '.tb-group[aria-label="글자"]', title: '글자 · 문단', text: '굵게 · 기울임 · 밑줄 · 위/아래첨자, 그리고 문단 정렬과 들여쓰기. 커서가 있는 문단에 걸립니다.' },
      { target: '.tb-group[aria-label="넣기"]', title: '넣기', text: '수식(LaTeX) · 화학식(구조식) · 표를 커서 문단 아래에 넣습니다.' },
      { target: '.tb-group[aria-label="보기"]', title: '보기', text: '문단부호(↵) 보이기, 영문 원문을 오른쪽에 나란히 두는 「원문 나란히」' },
      { target: '.tb-group[aria-label="도구"]', title: '도구 · 일괄 처리', text: '번호 다시 매기기 · 부호 목록 · 문서 통계 · 되풀이 문장, 그리고 폴더를 통째로 처리하는 일괄 처리.' },
      { target: '.tb-group[aria-label="보정(OA 대응)"]', title: '보정(OA 대응)', text: '「검토보고서」로 보정안을 불러오고 → 「보정 표시」로 확인 → 「보정 근거」 → 「보정서」로 보정서를 만드는 차례로 씁니다.' },
      { target: '.tb-group[aria-label="내보내기"]', title: '내보내기', text: 'PDF · Word · 제출파일(.hlz · .fin). 제출파일을 만들기 전에 제출 전 점검을 보여 줍니다.' },
      { target: '.editor-outline', title: '개요', text: '왼쪽 개요에서 절 · 청구항 · 도면으로 바로 갑니다. 청구항은 독립항 → 종속항 나무로 보이고, 지금 보는 절이 강조됩니다.' },
      { target: '[data-action="help"]', title: '언제든 다시', text: '이 「도움말」에서 기능별 투어를 골라 볼 수 있습니다. 모든 명령은 ⌘K 로 이름을 쳐서 찾을 수도 있습니다.' },
    ],
  },
  {
    id: 'source',
    group: '보기',
    title: '원문 나란히 보기',
    summary: '영문 원문을 오른쪽에 두고 함께 스크롤',
    steps: [
      { target: '[data-action="source"]', title: '원문 열기', text: '「원문 나란히」를 누르고 영문 원문 워드(.docx)를 고르면 문서 오른쪽에 원문 칸이 열립니다. 다시 누르면 닫힙니다.' },
      { target: '.editor-pages', title: '함께 움직임', text: '문서를 스크롤하면 원문 칸이 같은 식별번호([0001] ↔ 【0001】) · 같은 청구항 번호로 따라 옵니다.' },
      { target: '.editor-body', title: '원문에서 가기 · 짝 없음', text: '원문 문단을 누르면 문서의 그 문단으로 갑니다. 짝이 없는 원문 문단은 흐리게 보입니다.' },
    ],
  },
  {
    id: 'amend-marks',
    group: '보정(OA 대응)',
    title: '보정 표시',
    summary: '기준(연 뒤 · 원출원 · 다른 파일)에 견준 바뀐 글',
    steps: [
      { target: '.amend-base', title: '기준 고르기', text: '「연 뒤 고친 것」(이 파일을 연 뒤), 「원출원」(최종본 안 보정서까지), 「다른 파일…」(아무 HLT · fin) 가운데 견줄 기준을 고릅니다.' },
      { target: '[data-action="amend-marks"]', title: '보정 표시 켜기', text: '넣은 글은 밑줄, 지운 자리는 표시로 쪽 위에 보입니다. 상태줄에 바뀐 · 넣은 · 지운 요소가 요약됩니다.' },
      { target: '.editor-pages', title: '고친 곳 사이 이동', text: '⌘] 다음 고친 곳, ⌘[ 앞 고친 곳으로 갑니다(「연 뒤 고친 것」 기준).' },
    ],
  },
  {
    id: 'review',
    group: '보정(OA 대응)',
    title: '검토보고서 불러오기',
    summary: 'OA 검토보고서의 보정안 · 오기 정정을 편집기에',
    steps: [
      { target: '[data-action="review"]', title: '「검토보고서」 단추', text: 'OA 검토보고서(워드)를 고르면 보정안 · 상세설명 오기 정정 후보 · 도면 할 일을 읽어 옵니다.' },
      { target: '.editor-pages', title: '골라서 걸기', text: '보정안 하나와 걸 오기 정정을 고르면 한 번에 걸립니다(⌘Z 한 번에 되돌림). 오기마다 걸릴 곳 수를 먼저 보여 줍니다.' },
      { target: '.editor-toolbar', title: '남은 정정', text: '걸지 않은 오기 정정이 있으면 툴바에 단추가 생기고, 누를 때마다 다음 정정을 찾기 막대로 보여 줍니다.' },
    ],
  },
  {
    id: 'basis',
    group: '보정(OA 대응)',
    title: '보정 근거 찾기',
    summary: '바뀐 청구항 말이 있는 원출원 단락 · 인용 초안',
    steps: [
      { target: '[data-action="basis"]', title: '근거 찾기', text: '바뀐 청구항의 새 말이 원출원 명세서의 어느 단락에 있는지 찾아 줍니다(실시예 단락 우선 · 위 3개 + 「더 보기」).' },
      { target: '.amend-base', title: '기준', text: '근거는 「원출원」 기준에서 찾는 것이 보통입니다.' },
      { target: '.editor-pages', title: '의견서 초안', text: '고른 근거로 「…는 식별번호 【0045】에 기재되어 있다」 같은 인용 문장 초안을 만들고, 새 의견서로 열 수 있습니다.' },
    ],
  },
  {
    id: 'amend',
    group: '보정(OA 대응)',
    title: '보정서 만들기 · 대비표',
    summary: '인용 점검 → 보정서(.dta) · 대비표(워드)',
    steps: [
      { target: '[data-action="amend"]', title: '「보정서」 단추', text: '최종본(.fin)으로 연 문서에서 씁니다. 먼저 청구항 인용 문제(지운 항 인용 · 다중 인용 등)를 보여 주고, 그대로 만들지 고릅니다.' },
      { target: '.editor-status', title: '만든 것', text: '보정서(.dta)와 갱신된 최종본, 그리고 「이름_대비표.docx」(항목 · 보정 전 · 보정 후)를 함께 만듭니다.' },
    ],
  },
  {
    id: 'submit',
    group: '점검',
    title: '제출 전 점검',
    summary: '제출파일을 만들기 전에 사람이 볼 것',
    steps: [
      { target: '[data-action="fin"]', title: '언제 뜨나', text: '「제출파일」을 누르면 먼저 점검 목록이 뜹니다. 걸린 것이 없으면 바로 만듭니다.' },
      { target: '.editor-pages', title: '보는 것', text: '빈 식별번호 · 도면 언급 · 부호(같은 부호 다른 이름 · 같은 이름 다른 부호 · 안 쓰인 부호) · 청구항 인용 · 마침표 · 대표도 · 빈 표제 · 요약 길이.' },
      { target: '[data-action="batch"]', title: '폴더 통째로', text: '「일괄 처리 ▸ 폴더 제출 전 점검」으로 폴더 안 문서를 한 번에 점검해 표(CSV)로 받을 수 있습니다.' },
    ],
  },
  {
    id: 'renumber',
    group: '도구',
    title: '청구항 · 도면 번호 다시 매기기',
    summary: '번호와 인용 · 조사까지 한 번에',
    steps: [
      { target: '[data-action="tools"]', title: '도구 ▸ 번호 다시 매기기', text: '항이나 도면을 넣고 지운 뒤 「청구항 번호 다시 매기기」 · 「도면 번호 다시 매기기」를 고릅니다.' },
      { target: '.editor-pages', title: '미리 보기 → 적용', text: '바뀌는 곳을 먼저 보여 주고 「적용」하면 한 번에 바뀝니다(⌘Z 한 번에 되돌림). 「제N항」 · 「도 N」 인용과 뒤 조사(과/와)도 맞춥니다.' },
      { target: '.editor-status', title: '보정 문서', text: '최종본(보정) 문서에서는 지운 항 번호를 다시 매기지 않는 것이 실무라 경고를 보입니다.' },
    ],
  },
  {
    id: 'signs',
    group: '도구',
    title: '부호 목록',
    summary: '부호마다 이름 · 쓰인 수 · 처음 자리',
    steps: [
      { target: '[data-action="tools"]', title: '도구 ▸ 부호 목록', text: '상세설명의 「이름(부호)」와 【부호의 설명】을 모아 부호 숫자 차례로 보여 줍니다.' },
      { target: '.editor-pages', title: '이름이 여럿이면', text: '한 부호에 이름이 여럿(「몸체(2) · 본체(1)」)이면 바로 보입니다. 「가기」로 처음 나온 곳으로 갑니다.' },
    ],
  },
  {
    id: 'find',
    group: '편집',
    title: '찾기 옵션(단어 · 정규식)',
    summary: '대소문자 · 낱말 단위 · 정규식 바꾸기',
    steps: [
      { target: '.find-text', title: '찾을 글', text: '찾을 글을 칩니다. 비우고 서식만 고르면 그 서식 글자를 찾습니다.', prepare: openFindBar },
      { target: '.find-option[data-option="whole_word"]', title: '단어', text: '낱말 단위로 찾습니다. 뒤에 조사는 붙어도 됩니다 — 「기판」은 「기판은」에 걸리고 「기판부」에는 안 걸립니다.', prepare: openFindBar },
      { target: '.find-option[data-option="regex"]', title: '정규식', text: '「제(\\d+)항」처럼 찾고, 바꿀 글에 「청구항 $1」처럼 찾은 묶음을 쓸 수 있습니다.', prepare: openFindBar },
      { target: '.find-range', title: '범위', text: '상세설명만 · 청구항만 · 고른 곳만으로 좁힐 수 있습니다.', prepare: openFindBar },
    ],
  },
  {
    id: 'jump-edits',
    group: '편집',
    title: '고친 곳 사이 이동',
    summary: '⌘] 다음 · ⌘[ 앞',
    steps: [
      { target: '.editor-pages', title: '고친 곳만 훑기', text: '⌘] 를 누르면 연 뒤 고친 다음 문단으로, ⌘[ 는 앞 문단으로 갑니다. 끝에서는 처음으로 돌아갑니다.' },
      { target: '[data-action="tools"]', title: '메뉴에서도', text: '「도구」 메뉴의 「다음 고친 곳」 · 「앞 고친 곳」도 같습니다.' },
    ],
  },
  {
    id: 'palette',
    group: '편집',
    title: '명령 찾기(⌘K)',
    summary: '기능 이름을 쳐서 바로 실행',
    steps: [
      { target: '.editor-toolbar', title: '⌘K', text: '⌘K 를 누르고 「대비표」 · 「일괄」 · 「원문」처럼 이름을 치면 툴바 · 도구 메뉴의 명령이 걸러집니다. ↑↓ 와 Enter 로 실행합니다.' },
    ],
  },
  {
    id: 'batch',
    group: '도구',
    title: '일괄 처리',
    summary: '폴더 통째로 HLT 만들기 · 점검',
    steps: [
      { target: '[data-action="batch"]', title: '두 가지', text: '「폴더 HLT 만들기」(같은 이름 HLT 가 있으면 건너뜀)와 「폴더 제출 전 점검」(문서 × 종류 표)을 고릅니다.' },
      { target: '.editor-toolbar', title: '진행 · 멈춤', text: '오른쪽 아래에 「n/N · 파일」 진행이 보이고 「멈춤」으로 지금 파일까지만 하고 멈춥니다. 결과 표는 CSV 로 저장됩니다.' },
    ],
  },
  {
    id: 'import',
    group: '파일',
    title: '워드/XML로 HLT 만들기',
    summary: '워드 · KIPO XML · HML → 새 HLT',
    steps: [
      { target: '[data-action="new-menu"]', title: '새로 만들기 ▸ 워드/XML로 HLT 만들기', text: '원본을 고르고 출원 종류(PCT 국내진입 · 국내 출원)를 고르면 새 HLT 를 만들어 엽니다.' },
      { target: '.editor-status', title: '쓸 수 없는 글자', text: '통특기에 없는 글자가 있으면 바꿀 글자를 먼저 고르게 합니다. 같은 이름 HLT 가 있으면 저장 창에서 「대치」로 덮을 수 있습니다.' },
    ],
  },
  {
    id: 'recovery',
    group: '파일',
    title: '자동 저장 · 복구',
    summary: '앱이 비정상으로 끝나도 고친 것을 살린다',
    steps: [
      { target: '.editor-status', title: '자동 저장', text: '고친 뒤 2분 동안 손을 멈추면 복구본을 따로 둡니다(원래 파일은 그대로). 저장하거나 닫으면 지웁니다.' },
      { target: '[data-action="save"]', title: '다음 실행에 복구', text: '앱이 비정상으로 끝났다면 다음에 켤 때 「복구할까요?」를 묻습니다. 복구한 문서는 저장할 때 이름을 묻습니다.' },
    ],
  },
  {
    id: 'history',
    group: '파일',
    title: '이전 판 · 바뀐 파일 알림',
    summary: '저장이 덮은 판을 다시 열고, 다른 곳에서 고친 파일을 말없이 덮지 않는다',
    steps: [
      { target: '[data-action="save"]', title: '덮기 전 판을 남김', text: '저장할 때마다 덮이기 직전의 파일을 따로 남깁니다(파일마다 최근 10개 · 30일 — 「설정」에서 바꿈). 저장한 뒤에는 쓴 파일을 다시 읽어 화면과 같은지 확인합니다.' },
      { target: '[data-action="open"]', title: '이전 판 열기', text: '메뉴 「파일 › 이전 판…」에서 때를 골라 「열기」를 누르면 새 탭에 이름 없는 문서로 열립니다. 원본은 그대로이고, 저장하면 이름을 묻습니다.' },
      { target: '[data-action="save"]', title: '다른 곳에서 바뀐 파일', text: '연 뒤에 통특기 같은 다른 프로그램이 같은 파일을 고쳤으면, 저장하기 전에 「덮어쓰기 · 다른 이름으로 · 취소」를 묻습니다. 같은 파일은 한 탭에서만 열립니다(이미 연 탭으로 옮겨 갑니다).' },
    ],
  },
  {
    id: 'read-only',
    group: '파일',
    title: '읽기 전용으로 열기',
    summary: '원출원 · 최종본을 실수로 고치지 않게',
    steps: [
      { target: '[data-action="open"]', title: '원본 보호', text: '메뉴 「파일 › 읽기 전용으로 열기…」로 열면 글을 고칠 수 없고 ⌘S 도 막힙니다. 탭 제목에 「(읽기 전용)」이 붙습니다.' },
      { target: '[data-action="save-as"]', title: '사본으로 고치기', text: '고치려면 「다른 이름으로 저장」으로 사본을 만드세요. 그 뒤로는 사본을 고치는 문서가 되고 원본은 그대로 남습니다.' },
    ],
  },
  {
    id: 'snippets',
    group: '편집',
    title: '상용구',
    summary: '자주 쓰는 문구를 골라 넣기',
    steps: [
      { target: '[data-action="snippets"]', title: '상용구 창', text: '넣을 자리를 누른 뒤 「상용구」에서 「넣기」. 「새 상용구」 · 「고치기」 · 「지우기」로 목록을 다듬으면 모든 탭이 같은 목록을 씁니다.' },
      { target: '[data-action="help"]', title: '이름으로 넣기', text: '⌘K 명령 찾기에 상용구 이름을 쳐도 바로 넣을 수 있습니다(「상용구: 이름」).' },
    ],
  },
  {
    id: 'settings',
    group: '보기',
    title: '문서 배율 · 설정',
    summary: '글자 크기 · 자동 저장 간격 · 이전 판 보관',
    steps: [
      { target: '.zoom-box', title: '문서 배율', text: '문서 칸 오른쪽 아래 − · 100% · + (또는 ⌘+ 확대 · ⌘− 축소 · ⌘0 실제 크기 · 50~200%). 문서 쪽만 커지고 툴바 · 개요는 그대로입니다. 모든 탭이 같은 배율이 되고 다음 실행에도 남습니다.' },
      { target: '.editor-toolbar', title: '설정', text: '⌘,(메뉴 「서안 통특기 › 설정…」)에서 자동 저장 간격 · 이전 판 보관 개수와 날수 · 문서 배율 · 문단부호 기본 보이기를 바꿉니다.' },
    ],
  },
  {
    id: 'outline',
    group: '보기',
    title: '개요 · 청구항 나무 · 식별번호 미리 보기',
    summary: '절 · 청구항 · 도면으로 바로 가기',
    steps: [
      { target: '.editor-outline', title: '개요', text: '누르면 그 문단으로 갑니다. 스크롤하면 지금 보는 절이 강조됩니다.' },
      { target: '.editor-outline', title: '청구항 나무', text: '【청구범위】 아래가 독립항 → 종속항 나무로 보입니다. 다중 인용은 「제1·3항」, 아래 종속항 수는 「종속 N」.' },
      { target: '.editor-pages', title: '식별번호 미리 보기', text: '식별번호가 없는 HLT 는 제출파일에서 매겨질 [0001] 을 쪽 왼쪽에 흐리게 보입니다. ⌘G 로 그 번호에 갈 수도 있습니다.' },
    ],
  },
];

/** 투어 하나를 돌린다. 끝나면 `onEnd`. */
export function runTour(root: HTMLElement, tour: Tour, onEnd: () => void = () => {}): { end: () => void } {
  const layer = document.createElement('div');
  layer.className = 'tour';
  layer.innerHTML =
    '<div class="tour-hole"></div><div class="tour-bubble" role="dialog" aria-live="polite"><p class="tour-count"></p><strong class="tour-title"></strong><p class="tour-text"></p><p class="tour-note" hidden></p><div class="tour-buttons"><button type="button" class="tour-prev">이전</button><button type="button" class="tour-next">다음</button><button type="button" class="tour-end">끝내기</button></div></div>';
  const hole = layer.querySelector<HTMLElement>('.tour-hole')!;
  const bubble = layer.querySelector<HTMLElement>('.tour-bubble')!;
  let at = 0;
  let undo: (() => void) | void;
  const place = () => {
    const step = tour.steps[at];
    const target = root.querySelector<HTMLElement>(step.target);
    const box = target?.getBoundingClientRect();
    const visible = Boolean(box && (box.width > 0 || box.height > 0));
    hole.hidden = !visible;
    layer.classList.toggle('dim', !visible);
    const note = layer.querySelector<HTMLElement>('.tour-note')!;
    const off = target instanceof HTMLButtonElement && target.disabled;
    note.hidden = !off && visible;
    note.textContent = off ? '지금은 꺼져 있습니다 — 문서를 연 뒤에 쓸 수 있습니다.' : visible ? '' : '이 자리는 지금 화면에 보이지 않습니다(문서를 열거나 그 기능을 켜면 나타납니다).';
    if (visible && box) {
      const pad = 6;
      Object.assign(hole.style, {
        left: `${box.left - pad}px`,
        top: `${box.top - pad}px`,
        width: `${box.width + pad * 2}px`,
        height: `${box.height + pad * 2}px`,
      });
      const width = 320;
      if (box.height > window.innerHeight * 0.4) {
        // 키 큰 자리(개요 · 쪽): 옆에(오른쪽 · 자리가 없으면 왼쪽) 위쪽에 맞춘다 — 아래 · 위에 두면 그 자리를 가린다.
        const right = box.right + 12;
        bubble.style.left = `${right + width + 12 < window.innerWidth ? right : Math.max(12, box.left - width - 12)}px`;
        bubble.style.top = `${Math.max(12, box.top + 12)}px`;
      } else {
        const below = box.bottom + 12;
        bubble.style.left = `${Math.max(12, Math.min(box.left, window.innerWidth - width - 12))}px`;
        bubble.style.top = below + 180 < window.innerHeight ? `${below}px` : `${Math.max(12, box.top - 12 - 180)}px`;
      }
    } else {
      bubble.style.left = `${Math.max(12, window.innerWidth / 2 - 160)}px`;
      bubble.style.top = `${Math.max(12, window.innerHeight / 3)}px`;
    }
  };
  const show = (index: number) => {
    if (typeof undo === 'function') undo();
    at = index;
    const step = tour.steps[at];
    undo = step.prepare?.(root);
    layer.querySelector('.tour-count')!.textContent = `${tour.title} · ${at + 1}/${tour.steps.length}`;
    layer.querySelector('.tour-title')!.textContent = step.title;
    layer.querySelector('.tour-text')!.textContent = step.text;
    layer.querySelector<HTMLButtonElement>('.tour-prev')!.disabled = at === 0;
    layer.querySelector('.tour-next')!.textContent = at === tour.steps.length - 1 ? '마치기' : '다음';
    place();
  };
  const keys = (event: KeyboardEvent) => {
    if (event.key === 'Escape') end();
    else if (event.key === 'ArrowRight') next();
    else if (event.key === 'ArrowLeft' && at > 0) show(at - 1);
    else return;
    event.preventDefault();
    event.stopPropagation();
  };
  const next = () => (at < tour.steps.length - 1 ? show(at + 1) : end());
  function end() {
    if (typeof undo === 'function') undo();
    undo = undefined;
    document.removeEventListener('keydown', keys, true);
    window.removeEventListener('resize', place);
    layer.remove();
    onEnd();
  }
  layer.querySelector('.tour-next')!.addEventListener('click', next);
  layer.querySelector('.tour-prev')!.addEventListener('click', () => show(at - 1));
  layer.querySelector('.tour-end')!.addEventListener('click', end);
  document.addEventListener('keydown', keys, true);
  window.addEventListener('resize', place);
  root.append(layer);
  show(0);
  return { end };
}

/** 도움말: 기능 투어 목록(묶음별) · 단축키 모음. */
export function openHelp(root: HTMLElement, showShortcuts: () => void): void {
  root.querySelector('.help-shade')?.remove();
  const shade = document.createElement('div');
  shade.className = 'basis-shade help-shade';
  // 묶음 차례 = 툴바 차례(처음이라면 → 파일 → 편집 → 보기 → 도구 → 점검 → 보정).
  const ORDER = ['처음이라면', '파일', '편집', '보기', '도구', '점검', '보정(OA 대응)'];
  const groups = [...new Set(TOURS.map((tour) => tour.group))].sort((a, b) => ORDER.indexOf(a) - ORDER.indexOf(b));
  const escape = (text: string) =>
    text.replace(/[&<>"]/g, (ch) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[ch]!);
  shade.innerHTML = `<div class="basis-ask batch-ask help-ask"><header><strong>도움말 — 기능 투어</strong><button type="button" class="help-shortcuts">단축키 모음</button><button type="button" class="batch-close">닫기</button></header><div class="batch-body"><p class="help-lead">기능을 고르면 실제 화면에서 그 기능의 단추 · 칸을 차례로 짚어 줍니다(← → 로 이동 · Esc 로 끝내기).</p>${groups
    .map(
      (group) =>
        `<section><h3>${escape(group)}</h3><ul>${TOURS.filter((tour) => tour.group === group)
          .map(
            (tour) =>
              `<li><button type="button" class="help-tour" data-tour="${tour.id}"><strong>${escape(tour.title)}</strong><span>${escape(tour.summary)}</span></button></li>`,
          )
          .join('')}</ul></section>`,
    )
    .join('')}</div></div>`;
  shade.querySelector('.batch-close')!.addEventListener('click', () => shade.remove());
  shade.querySelector('.help-shortcuts')!.addEventListener('click', () => {
    shade.remove();
    showShortcuts();
  });
  for (const button of shade.querySelectorAll<HTMLButtonElement>('.help-tour')) {
    button.addEventListener('click', () => {
      shade.remove();
      runTour(root, TOURS.find((tour) => tour.id === button.dataset.tour)!);
    });
  }
  root.append(shade);
}
