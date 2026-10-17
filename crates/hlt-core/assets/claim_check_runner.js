// 특허로 청구항 점검(`PetitionClaimCheck.do`)의 판정 JS 를 jsdom 에서 그대로 돌린다.
// 현재 디렉터리 = 점검기 폴더(KIPO 자산이라 저장소 밖): web/page.html · web/kipo-common.js · web/claim_common.js ·
// web/claim_check_kr.js · node_modules/jsdom. 표준입력 = [{claimNum, claimText}](특허로 서버가 XML 에서 만드는 목록과 같은 꼴).
// 표준출력 = {error, alerts, logs, claims:[{no, patterns, parents, above:{낱말:안내}}]}. KIPO 코드는 고치지 않는다 — 환경만 맞춘다.
const fs = require('fs');
const path = require('path');
const { JSDOM, VirtualConsole } = require(path.join(process.cwd(), 'node_modules', 'jsdom'));

const web = (name) => fs.readFileSync(path.join(process.cwd(), 'web', name), 'utf8');
const claimList = JSON.parse(fs.readFileSync(0, 'utf8'));
// 페이지 DOM(#applno 등)은 그대로 두고 스크립트만 뺀다 — 인라인 스크립트는 RAONK 로 로컬 파일을 읽는 경로다.
const page = web('page.html')
  .replace(/<script[^>]*src=[^>]*><\/script>/g, '')
  .replace(/<script(?![^>]*src)[^>]*>[\s\S]*?<\/script>/g, '');
const logs = [];
const virtualConsole = new VirtualConsole();
virtualConsole.on('jsdomError', (e) => logs.push(String(e.message || e)));
const dom = new JSDOM(page, {
  runScripts: 'outside-only',
  virtualConsole,
  url: 'https://www.patent.go.kr/smart/kiponet3/apl/sg/ap/as/PetitionClaimCheck.do',
});
const w = dom.window;
const alerts = [];
w.alert = (message) => alerts.push(String(message));
// jsdom 의 XPath 는 Prototype 과 맞지 않는다 — 없으면 Prototype 이 DOM 순회로 간다.
w.document.evaluate = undefined;
// 페이지 인라인 스크립트의 전역(실측 `rootPath = "/smart/kiponet3"`).
w.rootPath = '/smart/kiponet3';
for (const name of ['kipo-common.js', 'claim_common.js', 'claim_check_kr.js']) {
  try {
    w.eval(web(name));
  } catch (e) {
    logs.push(`${name}: ${e}`);
  }
}
const out = { error: null, alerts, logs, claims: [] };
w.__claimList = claimList;
try {
  w.eval('ptclsChk(__claimList); onSuccess();');
} catch (e) {
  out.error = String(e);
}
try {
  let dumped = w.eval(`JSON.stringify($.map(hangMap.slice(1), function (h) {
    var patterns = [];
    $.each(h.hasPattern, function (k, v) { if (v != null && v != '') patterns.push(chkPattern[v - 1]); });
    var above = {};
    for (var word in h.above) above[word] = String(h.above[word].addInfo || '');
    return { no: String(h.no), patterns: patterns, parents: $.map(h.parents, function (p) { return String(p.no); }), above: above };
  }))`);
  // Prototype 의 Array#toJSON 이 JSON.stringify 를 두 번 싼다.
  while (typeof dumped === 'string') dumped = JSON.parse(dumped);
  out.claims = dumped;
} catch (e) {
  out.error = `${out.error || ''} dump: ${e}`;
}
process.stdout.write(JSON.stringify(out));
