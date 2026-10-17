// 수식·화학식 편집기(P5): 통특기의 수식 편집기(Eqsvr)·화학식 편집기(ChemTest)를 대신한다.
// 수식 = LaTeX → MathJax SVG · 화학식 = SMILES/Molfile → RDKit SVG. 둘 다 300dpi PNG 로 구워 그림 문단으로 넣는다.
export interface Figure {
  svg: string;
  /** 본문 12pt 기준 자연 크기(pt). */
  widthPt: number;
  heightPt: number;
}

/** 본문 글자 12pt = 16px · 1ex = 8px(MathJax 기본 비율). */
const EM_PX = 16;
const EX_PX = 8;
const PX_TO_PT = 0.75;

interface MathJaxGlobal {
  tex2svg(latex: string, options: { display: boolean; em: number; ex: number; containerWidth: number }): HTMLElement;
  startup: { promise: Promise<void> };
}
let mathjax: Promise<MathJaxGlobal> | null = null;

/** MathJax 브라우저 구성본(tex-svg-full)을 처음 쓸 때 한 번 싣는다 — npm 모듈판은 브라우저에서 `require` 를 부른다. */
export function loadMathJax(): Promise<MathJaxGlobal> {
  mathjax ??= new Promise((resolve, reject) => {
    const host = window as unknown as { MathJax?: unknown };
    host.MathJax = { svg: { fontCache: 'none' }, startup: { typeset: false } };
    const script = document.createElement('script');
    script.src = '/mathjax/tex-svg-full.js';
    script.onload = () => {
      const loaded = host.MathJax as MathJaxGlobal;
      loaded.startup.promise.then(() => resolve(loaded), reject);
    };
    script.onerror = () => reject(new Error('MathJax 를 싣지 못했습니다'));
    document.head.append(script);
  });
  return mathjax;
}

export async function renderLatex(latex: string): Promise<Figure> {
  return latexFigure(await loadMathJax(), latex);
}

/** MathJax 결과(ex 단위 SVG) → px 크기를 박은 SVG · pt 크기. */
export function latexFigure(engine: Pick<MathJaxGlobal, 'tex2svg'>, latex: string): Figure {
  const node = engine.tex2svg(latex, { display: true, em: EM_PX, ex: EX_PX, containerWidth: 1200 });
  const svg = node.querySelector('svg');
  if (!svg) throw new Error('수식을 그리지 못했습니다');
  if (node.querySelector('[data-mjx-error], merror')) throw new Error('LaTeX 오류');
  const ex = (value: string | null) => Number.parseFloat(value ?? '0') * EX_PX;
  const widthPx = ex(svg.getAttribute('width'));
  const heightPx = ex(svg.getAttribute('height'));
  svg.setAttribute('width', `${widthPx}`);
  svg.setAttribute('height', `${heightPx}`);
  svg.setAttribute('xmlns', 'http://www.w3.org/2000/svg');
  return { svg: svg.outerHTML, widthPt: widthPx * PX_TO_PT, heightPt: heightPx * PX_TO_PT };
}

interface RdkitModule {
  get_mol(input: string): { get_svg_with_highlights(details: string): string; delete(): void } | null;
}
let rdkit: Promise<RdkitModule> | null = null;

/** RDKit(WASM)은 처음 쓸 때 한 번 싣는다. */
function loadRdkit(): Promise<RdkitModule> {
  rdkit ??= new Promise((resolve, reject) => {
    const script = document.createElement('script');
    script.src = '/rdkit/RDKit_minimal.js';
    script.onload = () => {
      const init = (window as unknown as { initRDKitModule: (options: object) => Promise<RdkitModule> })
        .initRDKitModule;
      init({ locateFile: () => '/rdkit/RDKit_minimal.wasm' }).then(resolve, reject);
    };
    script.onerror = () => reject(new Error('RDKit 을 싣지 못했습니다'));
    document.head.append(script);
  });
  return rdkit;
}

/** SMILES 또는 Molfile → 구조식. 크기는 그림 px(기본 300×200 = 225×150pt). */
export async function renderChemistry(input: string, widthPx = 300, heightPx = 200): Promise<Figure> {
  const module = await loadRdkit();
  // Molfile 은 머리 세 줄(빈 줄일 수 있다)이 형식이라 다듬으면 깨진다 — 한 줄(SMILES)만 다듬는다.
  const mol = module.get_mol(input.includes('\n') ? input : input.trim());
  if (!mol) throw new Error('구조를 읽지 못했습니다(SMILES 또는 Molfile)');
  try {
    // 특허 도면은 흑백 — 원자 색(산소 빨강 등)을 검정으로. 흰 바탕만 둔다(RDKit 최소판은 흑백 팔레트 옵션을 무시한다).
    const svg = blackAndWhite(
      mol.get_svg_with_highlights(JSON.stringify({ width: widthPx, height: heightPx })),
    );
    return { svg, widthPt: widthPx * PX_TO_PT, heightPt: heightPx * PX_TO_PT };
  } finally {
    mol.delete();
  }
}

export function blackAndWhite(svg: string): string {
  return svg.replace(/#[0-9a-f]{6}\b/gi, (color) => (color.toUpperCase() === '#FFFFFF' ? color : '#000000'));
}

/** SVG → PNG(base64). 300dpi = pt × 300/72 화소 · 흰 바탕(통특기 그림은 투명을 안 쓴다). */
export async function rasterize(svg: string, widthPt: number, heightPt: number): Promise<string> {
  const scale = 300 / 72;
  const canvas = document.createElement('canvas');
  canvas.width = Math.max(1, Math.round(widthPt * scale));
  canvas.height = Math.max(1, Math.round(heightPt * scale));
  const context = canvas.getContext('2d');
  if (!context) throw new Error('캔버스를 못 씁니다');
  const image = new Image();
  const url = URL.createObjectURL(new Blob([svg], { type: 'image/svg+xml' }));
  try {
    await new Promise<void>((resolve, reject) => {
      image.onload = () => resolve();
      image.onerror = () => reject(new Error('SVG 를 그리지 못했습니다'));
      image.src = url;
    });
    context.fillStyle = '#fff';
    context.fillRect(0, 0, canvas.width, canvas.height);
    context.drawImage(image, 0, 0, canvas.width, canvas.height);
  } finally {
    URL.revokeObjectURL(url);
  }
  return canvas.toDataURL('image/png').split(',')[1];
}

interface JsmeApplet {
  molFile(): string;
  setCallBack(event: string, callback: () => void): void;
  readMolFile(molfile: string): void;
}
let jsme: Promise<void> | null = null;

/** JSME(구조식 그리기 · GWT)를 처음 쓸 때 싣는다. 준비되면 `jsmeOnLoad` 를 부른다. */
function loadJsme(): Promise<void> {
  jsme ??= new Promise((resolve, reject) => {
    (window as unknown as { jsmeOnLoad: () => void }).jsmeOnLoad = () => resolve();
    const script = document.createElement('script');
    script.src = '/jsme/jsme.nocache.js';
    script.onerror = () => reject(new Error('JSME 를 싣지 못했습니다'));
    document.head.append(script);
  });
  return jsme;
}

/** `host` 에 구조식 그리기 판을 연다. 그림이 바뀔 때마다 Molfile 을 `changed` 로 넘긴다. */
export async function openStructureDrawer(
  host: HTMLElement,
  changed: (molfile: string) => void,
): Promise<JsmeApplet> {
  await loadJsme();
  host.id ||= `jsme-${Math.random().toString(36).slice(2)}`;
  const JSApplet = (window as unknown as { JSApplet: { JSME: new (id: string, w: string, h: string, options: object) => JsmeApplet } }).JSApplet;
  const applet = new JSApplet.JSME(host.id, '100%', '320px', { options: 'oldlook,star,nocanonize' });
  applet.setCallBack('AfterStructureModified', () => changed(applet.molFile()));
  return applet;
}
