//! 연 HLT → Word(.docx). 내보낸 Word 를 서안의 Word 읽기기로 되읽어 원본 의미 모델과 맞춘다.

use std::path::PathBuf;

use hlt_core::{
    CharacterFormat, EditorSession, FigureKind, GenerationDialect, HmlDocument, HmlInline,
    TableOperation, WriteOptions, hml_document_from_hlt, inspect_docx_path, inspect_hml_path,
    load_generation_input, save_generated_hlt_as_new,
};

/// 합성 PCT 문서 + 세로 병합 표 + 수식 그림 + 굵게.
fn session(dir: &tempfile::TempDir) -> EditorSession {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/generate_pct.hml");
    let document = inspect_hml_path(&fixture).unwrap();
    let path = dir.path().join("원본.HLT");
    save_generated_hlt_as_new(
        &fixture,
        &document,
        GenerationDialect::PctPatent,
        &[],
        &path,
        WriteOptions::default(),
    )
    .unwrap();
    let mut session = EditorSession::open(&path).unwrap();
    let body = session
        .paragraphs()
        .into_iter()
        .find(|p| p.text.chars().count() > 10 && !p.text.starts_with('【'))
        .unwrap()
        .id;
    session
        .toggle_format(&body, 0, 3, CharacterFormat::Bold)
        .unwrap();
    let first = session.insert_table(&body, 3, 2).unwrap();
    session.replace_cell_text(&first, "병합 칸").unwrap();
    let table = session.cell_position(&first).unwrap().table_id;
    session
        .table_edit(TableOperation::Merge {
            table_id: table,
            row: 0,
            column: 0,
            row_end: 1,
            column_end: 0,
        })
        .unwrap();
    let mut png = Vec::new();
    image::RgbImage::from_pixel(8, 4, image::Rgb([0, 0, 0]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let body = session.paragraphs()[3].id.clone();
    session
        .insert_figure(&body, FigureKind::Math, png, 40.0, 20.0)
        .unwrap();
    session
}

type Cells = Vec<(String, u32, u32, bool)>;

fn tables(document: &HmlDocument) -> Vec<Cells> {
    document
        .tables
        .iter()
        .map(|table| {
            table
                .cell_ids
                .iter()
                .map(|id| {
                    let cell = document.cells.iter().find(|c| &c.id == id).unwrap();
                    (
                        cell.text.clone(),
                        cell.rowspan,
                        cell.colspan,
                        cell.merge_continuation,
                    )
                })
                .collect()
        })
        .collect()
}

#[test]
fn exported_word_reads_back_as_the_same_document() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = session(&dir);
    let bytes = session.docx().unwrap();
    let (model, _) = hml_document_from_hlt(&session.materialize().unwrap()).unwrap();
    let out = dir.path().join("내보냄.docx");
    std::fs::write(&out, &bytes).unwrap();
    let word = inspect_docx_path(&out).unwrap().document;
    // 줄 간격 = 배수 2.8(240 × 2.8).
    let mut styles = String::new();
    std::io::Read::read_to_string(
        &mut zip::ZipArchive::new(std::io::Cursor::new(&bytes))
            .unwrap()
            .by_name("word/styles.xml")
            .unwrap(),
        &mut styles,
    )
    .unwrap();
    assert!(styles.contains(r#"<w:spacing w:after="0" w:line="672" w:lineRule="auto"/>"#));

    let texts = |d: &HmlDocument| {
        d.paragraphs
            .iter()
            .map(|p| p.text.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        texts(&word),
        texts(&model),
        "문단 글(표 꼬리 포함) 순서대로"
    );
    let markers = |d: &HmlDocument| {
        d.paragraphs
            .iter()
            .filter_map(|p| p.marker.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(markers(&word), markers(&model), "표제");
    assert_eq!(
        markers(&word),
        ["【표 1】", "【청구범위】", "【수학식 1】"],
        "표제가 나온다"
    );
    assert_eq!(tables(&word), tables(&model), "표 칸 글 · 병합");
    assert!(
        tables(&word)
            .iter()
            .flatten()
            .any(|(_, rowspan, _, _)| *rowspan == 2),
        "세로 병합 표가 들어 있다"
    );
    assert_eq!(word.images.len(), model.images.len(), "그림 수");
    assert!(!word.images.is_empty());
    let bold = |d: &HmlDocument| {
        d.paragraphs.iter().flat_map(|p| &p.inlines).any(|inline| {
            matches!(inline, HmlInline::Text { formats, .. } if formats.contains(&CharacterFormat::Bold))
        })
    };
    assert!(bold(&word) && bold(&model), "굵게가 남는다");
    // 식별번호 【0001】 은 나오지 않는다.
    let numbered = word.paragraphs.iter().any(|p| {
        let t = p.text.trim_start();
        t.starts_with('【') && t.chars().skip(1).take(4).all(|c| c.is_ascii_digit())
    });
    assert!(!numbered, "식별번호가 없다");
    // 다시 HLT 로 굽힌다.
    let (again, payloads) = load_generation_input(&out).unwrap();
    save_generated_hlt_as_new(
        &out,
        &again,
        GenerationDialect::PctPatent,
        &payloads,
        &dir.path().join("다시.HLT"),
        WriteOptions::default(),
    )
    .unwrap();
}
