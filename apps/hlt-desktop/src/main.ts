import { editorLog } from './contracts';
import { mountEditor } from './editor';
import './styles.css';

// 화면 보안 정책(CSP)이 막은 것을 오류 기록에 남긴다 — 정책 탓에 기능이 깨지면 버그 신고로 보인다.
// JSME 의 숨은 iframe 주소 `javascript:""` 는 막혀도 무해(빈 iframe 에 글을 쓴다)라 뺀다.
document.addEventListener('securitypolicyviolation', (event) => {
  if (event.sample.startsWith('javascript:')) return;
  void editorLog(`csp 차단 ${event.violatedDirective} ${event.blockedURI}`).catch(() => {});
});

const root = document.getElementById('app');
if (!root) throw new Error('missing element: app');
mountEditor(root);
