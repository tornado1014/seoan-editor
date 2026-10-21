---
name: seoan
description: >
  서안(Seoan) 명령줄 hlt-cli 로 KIPO 전자출원 문서를 만들고 바꾸고 고친다 — .HLT 명세서 · 의견서 · 보정서 ·
  제출파일(.hlz/.fin). Use when the user wants to read or edit an .HLT, save an .HLT as Word (.docx),
  build an .HLT from Word/KIPO XML, write a new 의견서, make a 보정서 from a 최종본 .fin,
  print an .HLT to PDF, or export/validate KIPO submission XML — from an agent harness or any shell.
---

# seoan — 하네스용 서안 API

모든 명령은 `hlt-cli <명령> …` 이고 stdout 에 **JSON 한 줄**(봉투)을 낸다.
판정은 봉투로만 한다: `ok: true` 면 성공 · 아니면 `error.code`(`output_exists` · `revision_conflict` · `usage` …).
종료 코드 0 = 성공 · 2 = 실패 · 64 = 인자 잘못.

**규칙**
- 출력은 늘 **없는 새 파일**이다. 있으면 `output_exists` 로 거부한다 — 원본은 절대 덮이지 않는다. 다시 할 땐 새 이름으로.
  예외: `export-fin` · `export-amendment` 는 `--overwrite` 를 주면 있는 `.fin` · `.dta` 를 덮는다(사용자가 덮으라고 했을 때만 · 임시 파일에 다 쓴 뒤 바꿔 끼운다).
- 다루는 것은 **미공개 출원**이다. 파일 · 본문을 외부 서비스에 올리지 마라.
- 고친 `.HLT` 는 `hlt-cli verify <file>` 로 확인한다(`ok: true`).

## 설치

```bash
command -v hlt-cli || cargo install --path <seoan-editor 저장소>/crates/hlt-cli
```

## 레시피

### 1. 읽기
```bash
hlt-cli inspect doc.HLT --kind paragraphs [--limit 500 --cursor <next_cursor>]   # id = "paragraph:N", text
hlt-cli inspect-docx in.docx · inspect-kipo-xml in.xml · inspect-hml in.hml
hlt-cli verify doc.HLT
```

### 2. Word 로 저장
```bash
hlt-cli export-docx doc.HLT doc.docx
```
표제(【발명의 설명】 · 【청구항 1】 …)는 나오고 식별번호(【0001】)는 안 나온다. 표(병합 칸) · 그림 · 굵게/첨자 유지 · 줄 간격 배수 2.8.

### 3. Word · KIPO XML → .HLT 명세서
```bash
hlt-cli generate-hlt spec.docx spec.HLT --dialect pct-patent            # PCT 국내진입
hlt-cli generate-hlt spec.docx spec.HLT --dialect patent-specification-ko # 국내출원
```
HLT 는 CP949 로 저장한다. CP949 밖 글자(`−` `é` NBSP …)는 대응표로 바뀌고 결과의 `cp949_substitutions` 에 남는다.
대응표에 없는 글자(CP949 밖 첨자 `⁵` · 이모지)는 `cp949_invalid`(`details.chars` = 전부)로 멈춘다 — 사용자에게 보여 주고
고른 대응을 `--substitutions map.json`(`{"⁵": "^5"}` · 빈 글 = 지움)으로 다시 준다.

### 4. 글 고치기
1. `inspect --kind paragraphs` 로 고칠 문단 id 를 찾는다.
2. `edits.json` = `[{"paragraph_id": "paragraph:3", "replacement": "새 글"}]`
3. 해시 · 판(revision)은 손으로 쓰지 말고 도우미로 채운다:
   ```bash
   python3 <이 스킬>/ops.py doc.HLT edits.json > ops.json
   hlt-cli edit-plan-variable doc.HLT --operations ops.json     # 미리 보기: decisions[].status = ready
   hlt-cli save-variable-as-new doc.HLT doc-2.HLT --operations ops.json
   ```
   글 길이가 같으면 `edit-plan` / `save-as-new` 도 된다. `skipped` 의 `reason: source_mismatch` = 문단 글이 바뀌었다 → ops.py 를 다시 돌려라. `hidden_controls_unsupported` = 표 · 그림이 든 문단이라 글로 못 고친다(표 칸은 `table-plan`).
- 서식 · 표 · 문단모양: `format-plan`/`save-formats-as-new` · `table-plan`/`save-table-as-new` · `set-paragraph-shapes-as-new` (요청 형식 = 저장소 `contracts/*.schema.json`)
- 식별번호 매기기: `hlt-cli number-paragraphs doc.HLT numbered.HLT`

### 5. 의견서 새로 쓰기
본문을 글 파일로 쓴다 — **줄 하나 = 문단 하나**, 빈 줄 = 빈 문단. 【의견내용】 표제는 자동으로 붙는다.
```bash
hlt-cli new-opinion opinion.HLT --body body.txt
hlt-cli export-fin opinion.HLT opinion.fin --hlz-name opinion.hlz --app-date 2026-10-19 --app-time 20261019120000
```

### 6. 보정서
```bash
hlt-cli open-package final.fin current.HLT        # 최종본(.fin) · 제출파일(.hlz) → .HLT
# 4번으로 current.HLT → amended.HLT
hlt-cli export-amendment final.fin amended.HLT amended.fin --dta-name amend.dta --date 2026-10-19   # 보정서가 붙은 새 .fin
hlt-cli export-amendment before.HLT amended.HLT amend.dta                                            # 보정서 .dta 만
```

### 7. 인쇄 · 제출 XML
```bash
hlt-cli print-pdf doc.HLT doc.pdf --font <바탕 .ttc/.ttf> [--font-index 1]
hlt-cli export-kipo-xml doc.HLT doc.xml
hlt-cli validate-kipo-xml doc.fin --schema-dir <Epasl/INCLUDE/Schema>
hlt-cli check-claims doc.HLT --checker-dir <청구항 점검기 폴더>
```

## 한계
- 표 삽입 · 칸 병합 · 그림 삽입은 아직 앱(서안 편집기)에서만 된다.
- Word 내보내기는 줄 · 쪽을 Word 가 다시 흘린다(HLT 쪽과 같지 않다).
