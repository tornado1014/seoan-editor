// 수식·화학식 편집기의 제3자 브라우저 자산을 node_modules 에서 public/ 으로 복사한다(저장소에는 넣지 않는다).
// MathJax 3(Apache-2.0) tex-svg-full · RDKit.js(BSD-3-Clause) minimal + wasm · JSME 구조식 그리기(BSD-3-Clause, GWT 폴더 통째)
// · 수식 인식 모델 pix2text-mfr(MIT · npm 에 없어 릴리스 자산에서 받는다 · 런타임 onnxruntime-web 은 Vite 가 묶는다).
import { createHash } from 'node:crypto';
import { copyFileSync, cpSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';

const files = [
  ['node_modules/mathjax-full/es5/tex-svg-full.js', 'public/mathjax/tex-svg-full.js'],
  ['node_modules/@rdkit/rdkit/dist/RDKit_minimal.js', 'public/rdkit/RDKit_minimal.js'],
  ['node_modules/@rdkit/rdkit/dist/RDKit_minimal.wasm', 'public/rdkit/RDKit_minimal.wasm'],
];
for (const [from, to] of files) {
  mkdirSync(to.slice(0, to.lastIndexOf('/')), { recursive: true });
  copyFileSync(from, to);
}
cpSync('node_modules/jsme-editor', 'public/jsme', { recursive: true });

// 수식 인식 모델(8비트 양자화본 · 만드는 법은 public/mfr/README.txt). 있고 해시가 맞으면 받지 않는다 · 해시가 다르면 멈춘다.
const MODEL_URL = 'https://github.com/tornado1014/seoan-editor/releases/download/mfr-model-1/';
const model = {
  'encoder_model.onnx': '1124fd88ccfa7a8660dd94422c988b253e193d1c8dba346e17fcb90dcc5a8a6a',
  'decoder_model.onnx': 'b8fcaac25f6741162e49f70b8c24eb44e08e840921ac181481fdafeddf98f894',
  'tokenizer.json': '3e2ab757277d22639bec28c9d7972e352d3d1dba223051fa674002dc5ab64df3',
};
const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');
for (const [name, want] of Object.entries(model)) {
  const to = `public/mfr/${name}`;
  if (existsSync(to) && sha256(readFileSync(to)) === want) continue;
  const response = await fetch(MODEL_URL + name);
  if (!response.ok) throw new Error(`수식 인식 모델을 받지 못했습니다: ${name} (${response.status})`);
  const bytes = Buffer.from(await response.arrayBuffer());
  if (sha256(bytes) !== want) throw new Error(`수식 인식 모델의 해시가 다릅니다: ${name}`);
  writeFileSync(to, bytes);
}
