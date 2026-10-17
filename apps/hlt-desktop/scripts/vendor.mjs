// 수식·화학식 편집기의 제3자 브라우저 자산을 node_modules 에서 public/ 으로 복사한다(저장소에는 넣지 않는다).
// MathJax 3(Apache-2.0) tex-svg-full · RDKit.js(BSD-3-Clause) minimal + wasm · JSME 구조식 그리기(BSD-3-Clause, GWT 폴더 통째).
import { copyFileSync, cpSync, mkdirSync } from 'node:fs';

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
