// JSME(GWT xsiframe) 가 숨은 iframe 에 글로 꽂는 스크립트들의 CSP 해시를 낸다.
// 꽂는 방식은 public/jsme/jsme.nocache.js 의 m() · __installRunAsyncCode 그대로:
//   본체 — Chrome 계열: 'eval("' + 조각들(JSON 이스케이프) + '");' 한 덩이 · 그 밖(WebKit): 조각마다 하나씩
//   지연 조각 — runAsyncCallbackN('글') 의 글 그대로
// JSME 를 바꾸면 다시 돌려 tauri.conf.json 의 script-src 를 갈아라: node scripts/jsme-csp-hashes.mjs
import { createHash } from 'node:crypto';
import { readFileSync, readdirSync } from 'node:fs';
import { runInNewContext } from 'node:vm';

const root = new URL('../public/jsme/', import.meta.url);
const hash = (text) => `'sha256-${createHash('sha256').update(text, 'utf8').digest('base64')}'`;
const hashes = new Set();
// 순열(브라우저별 본체) = compilation-mappings.txt 에 적힌 것(96E40….cache.js 는 InChI 부속 — script src 로 실려 해시 불필요)
const permutations = new Set(readFileSync(new URL('compilation-mappings.txt', root), 'utf8').match(/^[0-9A-F]{32}\.cache\.js/gm));
for (const name of permutations) {
  let chunks;
  runInNewContext(readFileSync(new URL(name, root), 'utf8'), { jsme: { onScriptDownloaded: (a) => (chunks = a) } });
  hashes.add(hash(`eval("${chunks.map((c) => JSON.stringify(c).slice(1, -1)).join('')}");`));
  chunks.forEach((c) => hashes.add(hash(c)));
  const deferred = new URL(`deferredjs/${name.replace('.cache.js', '')}/`, root);
  for (const part of readdirSync(deferred)) {
    const wnd = { jsme: new Proxy({}, { get: () => (code) => hashes.add(hash(code)) }) };
    runInNewContext(readFileSync(new URL(part, deferred), 'utf8'), { $wnd: wnd });
  }
}
console.log([...hashes].join(' '));
