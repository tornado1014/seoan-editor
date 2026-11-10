// 테스트가 Rust 메뉴 원본을 글로 읽는다(Vite `?raw`).
declare module '*?raw' {
  const text: string;
  export default text;
}

// styles.test.ts 가 CSS 원본을 읽는다(vitest 는 `?raw` CSS 를 비워 준다 · node 타입 패키지는 두지 않는다).
declare module 'node:fs' {
  export function readFileSync(path: string, encoding: 'utf8'): string;
}
