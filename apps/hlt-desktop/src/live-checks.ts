import type { LiveIssueDto } from './contracts';

/** 건수(문단 없는 항목은 건수에만) · 문단별 묶음(오류가 앞 — 그 문단 표시는 오류). */
export function summarize(issues: LiveIssueDto[]) {
  const byParagraph = new Map<string, LiveIssueDto[]>();
  for (const issue of issues) {
    if (!issue.paragraph_id) continue;
    const list = byParagraph.get(issue.paragraph_id) ?? [];
    if (issue.severity === 'error') list.unshift(issue);
    else list.push(issue);
    byParagraph.set(issue.paragraph_id, list);
  }
  const errors = issues.filter((issue) => issue.severity === 'error').length;
  return { errors, warnings: issues.length - errors, byParagraph };
}

/**
 * 손을 멈춘 뒤 한 번 가져오는 틀 — 고칠 때마다 `touch()`, `delayMs` 뒤 가져온다.
 * 낡은 판(`revision` 이 화면 판과 다름)의 결과는 버리고 다시 예약한다. `stop()` 뒤에 도착한 결과도 버린다.
 */
export function createRevisionedFetch<T extends { revision: number }>(o: {
  fetch(): Promise<T | null>;
  revision(): number;
  apply(result: T): void;
  delayMs?: number;
}) {
  let timer = 0;
  let generation = 0;
  const schedule = () => {
    window.clearTimeout(timer);
    timer = window.setTimeout(() => void now(), o.delayMs ?? 1000);
  };
  async function now() {
    window.clearTimeout(timer);
    const mine = generation;
    const result = await o.fetch().catch(() => null);
    if (mine !== generation || !result) return; // 닫았다 · 가져오지 못했다
    if (result.revision !== o.revision()) return schedule(); // 그 사이 화면이 바뀌었다
    o.apply(result);
  }
  return {
    touch: schedule,
    now,
    stop() {
      generation += 1;
      window.clearTimeout(timer);
    },
  };
}

/** 상시 점검 — 고칠 때마다 `touch()`, 손을 멈추고 `delayMs` 뒤 한 번 가져온다. */
export function createLiveChecks(o: {
  fetch(): Promise<{ revision: number; issues: LiveIssueDto[] }>;
  revision(): number;
  draw(byParagraph: Map<string, LiveIssueDto[]>): void;
  strip(errors: number, warnings: number): void;
  delayMs?: number;
}) {
  return createRevisionedFetch({
    fetch: o.fetch,
    revision: o.revision,
    delayMs: o.delayMs,
    apply(result) {
      const sum = summarize(result.issues);
      o.draw(sum.byParagraph);
      o.strip(sum.errors, sum.warnings);
    },
  });
}
