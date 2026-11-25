// 수식 그림 → LaTeX: pix2text-mfr(MIT · TrOCR 계열)를 onnxruntime-web(WASM)으로 앱 안에서 돌린다 — 그림은 밖으로 나가지 않는다.
// 굵은 글씨는 자주 틀린다(실측) — 결과는 늘 원본 그림과 대조해 사람이 고친다. 자동으로 넣지 않는다.

const SIZE = 384;
/** 시작 · 끝 토큰(둘 다 `</s>`). */
const EOS = 2;
const MAX_TOKENS = 512;

const cubic = (t: number) => {
  t = Math.abs(t);
  return t < 1 ? (1.5 * t - 2.5) * t * t + 1 : t < 2 ? ((-0.5 * t + 2.5) * t - 4) * t + 2 : 0;
};

/** 한 축 바꾸기: 줄마다 `n` 화소를 `m` 화소로. 화소는 `get` 으로 읽고 `put` 으로 쓴다(가로 · 세로가 같은 계산을 쓴다). */
function resizeAxis(
  n: number,
  m: number,
  lines: number,
  get: (line: number, at: number, channel: number) => number,
  put: (line: number, at: number, channel: number, value: number) => void,
) {
  const scale = n / m;
  // 줄일 땐 필터를 배율만큼 넓힌다(안 넓히면 화소를 건너뛰어 가는 획이 사라진다).
  const spread = Math.max(1, scale);
  const support = 2 * spread;
  for (let o = 0; o < m; o++) {
    const center = (o + 0.5) * scale;
    const lo = Math.max(0, Math.trunc(center - support + 0.5));
    const hi = Math.min(n, Math.trunc(center + support + 0.5));
    const weights: number[] = [];
    let sum = 0;
    for (let i = lo; i < hi; i++) {
      const weight = cubic((i - center + 0.5) / spread);
      weights.push(weight);
      sum += weight;
    }
    for (let line = 0; line < lines; line++) {
      for (let k = 0; k < 3; k++) {
        let acc = 0;
        for (let i = lo; i < hi; i++) acc += get(line, i, k) * weights[i - lo];
        put(line, o, k, Math.min(255, Math.max(0, Math.round(acc / sum))));
      }
    }
  }
}

/** RGB(줄 차례 · 화소마다 3바이트) w×h → toW×toH. Pillow 의 BICUBIC 과 같은 계산(가로 → 세로 · 단계마다 8비트 반올림) —
 *  캔버스 보간은 브라우저마다 달라 같은 그림이 다른 LaTeX 가 된다. */
export function resizeBicubic(rgb: Uint8Array, w: number, h: number, toW: number, toH: number): Uint8Array {
  const wide = new Uint8Array(h * toW * 3);
  resizeAxis(w, toW, h, (y, x, k) => rgb[(y * w + x) * 3 + k], (y, x, k, v) => (wide[(y * toW + x) * 3 + k] = v));
  const out = new Uint8Array(toH * toW * 3);
  resizeAxis(h, toH, toW, (x, y, k) => wide[(y * toW + x) * 3 + k], (x, y, k, v) => (out[(y * toW + x) * 3 + k] = v));
  return out;
}

/** RGBA 화소 → 모델 입력(1×3×384×384 · 채널 차례 · −1…1). 투명은 부르는 쪽이 흰 바탕에 합성해 둔다. */
export function toInput(rgba: Uint8ClampedArray | Uint8Array, w: number, h: number): Float32Array {
  const rgb = new Uint8Array(w * h * 3);
  for (let i = 0; i < w * h; i++) for (let k = 0; k < 3; k++) rgb[i * 3 + k] = rgba[i * 4 + k];
  const small = resizeBicubic(rgb, w, h, SIZE, SIZE);
  const input = new Float32Array(3 * SIZE * SIZE);
  for (let i = 0; i < SIZE * SIZE; i++) for (let k = 0; k < 3; k++) input[k * SIZE * SIZE + i] = small[i * 3 + k] / 127.5 - 1;
  return input;
}

/** 토큰 번호 → 글(ByteLevel BPE: `Ġ` = 띄어쓰기 · `<…>` 특수 토큰은 버린다). */
export function decodeTokens(ids: number[], words: string[]): string {
  return ids
    .map((id) => words[id] ?? '')
    .filter((word) => !/^<.*>$/.test(word))
    .join('')
    .replaceAll('Ġ', ' ')
    .trim();
}

/** 모델은 토큰마다 띄어 쓴다(`3 . 1 0` · `\mathrm { m i n }`) — 띄어쓰기를 걷는다(그대로 두면 MathJax 가 `3. 10` 으로 벌린다).
 *  제어어 뒤에 글자가 오는 자리(`\alpha x`)만 남긴다. */
export function tidyLatex(latex: string): string {
  return latex
    .replace(/(\\[a-zA-Z]+) +(?=[a-zA-Z])/g, '$1\u0000')
    .replace(/ +/g, '')
    .replaceAll('\u0000', ' ');
}

/** 디코더 한 걸음: 지금까지의 토큰 → 다음 토큰의 점수(낱말 수만큼). */
export type NextScores = (ids: number[]) => Promise<ArrayLike<number>>;

/** 가장 점수 높은 토큰을 끝 토큰이 나올 때까지 잇는다(상한 512). 시작 토큰은 빼고 돌려준다. */
export async function greedy(next: NextScores, limit = MAX_TOKENS): Promise<number[]> {
  const ids = [EOS];
  while (ids.length <= limit) {
    const scores = await next(ids);
    let best = 0;
    for (let i = 1; i < scores.length; i++) if (scores[i] > scores[best]) best = i;
    if (best === EOS) break;
    ids.push(best);
  }
  return ids.slice(1);
}

interface Engine {
  encode(input: Float32Array): Promise<(ids: number[]) => Promise<ArrayLike<number>>>;
  words: string[];
}
let engine: Promise<Engine> | null = null;

/** 런타임(WASM 14MB · Vite 가 assets 에 싣는다) · 모델(30MB · public/mfr)은 처음 쓸 때 한 번 싣는다. 실패하면 다음에 다시 싣는다. */
function loadEngine(): Promise<Engine> {
  engine ??= (async () => {
    const ort = await import('onnxruntime-web/wasm');
    // 스레드는 SharedArrayBuffer(교차 출처 격리)가 있어야 한다 — 웹뷰엔 없다.
    ort.env.wasm.numThreads = 1;
    const [encoder, decoder, tokenizer] = await Promise.all([
      ort.InferenceSession.create('/mfr/encoder_model.onnx'),
      ort.InferenceSession.create('/mfr/decoder_model.onnx'),
      fetch('/mfr/tokenizer.json').then((response) => {
        if (!response.ok) throw new Error('수식 인식 모델을 싣지 못했습니다');
        return response.json() as Promise<{ model: { vocab: Record<string, number> } }>;
      }),
    ]);
    const words: string[] = [];
    for (const [word, id] of Object.entries(tokenizer.model.vocab)) words[id] = word;
    return {
      words,
      async encode(input) {
        const hidden = (await encoder.run({ pixel_values: new ort.Tensor('float32', input, [1, 3, SIZE, SIZE]) }))
          .last_hidden_state;
        return async (ids) => {
          const logits = (
            await decoder.run({
              input_ids: new ort.Tensor('int64', BigInt64Array.from(ids, BigInt), [1, ids.length]),
              encoder_hidden_states: hidden,
            })
          ).logits;
          const vocab = logits.dims[2];
          return (logits.data as Float32Array).subarray((ids.length - 1) * vocab, ids.length * vocab);
        };
      },
    };
  })();
  engine.catch(() => (engine = null));
  return engine;
}

/** 수식 그림(클립보드 · 파일) → LaTeX. 수 초 걸린다(첫 번은 모델을 싣느라 더). */
export async function recognizeFormula(picture: Blob): Promise<string> {
  const bitmap = await createImageBitmap(picture);
  const canvas = document.createElement('canvas');
  canvas.width = bitmap.width;
  canvas.height = bitmap.height;
  const context = canvas.getContext('2d');
  if (!context) throw new Error('캔버스를 못 씁니다');
  context.fillStyle = '#fff';
  context.fillRect(0, 0, canvas.width, canvas.height);
  context.drawImage(bitmap, 0, 0);
  const input = toInput(context.getImageData(0, 0, canvas.width, canvas.height).data, canvas.width, canvas.height);
  const { encode, words } = await loadEngine();
  return tidyLatex(decodeTokens(await greedy(await encode(input)), words));
}
