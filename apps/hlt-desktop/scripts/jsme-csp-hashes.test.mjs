import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { expect, test } from 'vitest';

// JSME 를 바꾸고 해시를 안 갈면 배포판에서만 구조식 판이 조용히 안 뜬다 — 여기서 붉게.
test('CSP script-src 에 JSME 가 꽂는 스크립트 해시가 다 있다', () => {
  const hashes = execFileSync('node', ['scripts/jsme-csp-hashes.mjs'], { encoding: 'utf8' }).trim().split(' ');
  const conf = JSON.parse(readFileSync('src-tauri/tauri.conf.json', 'utf8'));
  const scriptSrc = conf.app.security.csp['script-src'];
  expect(hashes.length).toBeGreaterThan(10);
  for (const hash of hashes) expect(scriptSrc).toContain(hash);
  expect(conf.app.security.dangerousDisableAssetCspModification).toContain('style-src');
});
