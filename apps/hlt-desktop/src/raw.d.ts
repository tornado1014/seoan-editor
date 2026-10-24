// 테스트가 Rust 메뉴 원본을 글로 읽는다(Vite `?raw`).
declare module '*?raw' {
  const text: string;
  export default text;
}
