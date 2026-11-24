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
    // Word 가 그리는 크기(wp:extent = 40×20pt)를 그대로 읽는다 — 쪽 폭으로 키우지 않는다.
    let mm = |units: Option<u32>| f64::from(units.unwrap()) * 0.282;
    let figure = &word.images[0];
    assert!(
        (mm(figure.width) - 40.0 * 25.4 / 72.0).abs() < 0.5,
        "{figure:?}"
    );
    assert!(
        (mm(figure.height) - 20.0 * 25.4 / 72.0).abs() < 0.5,
        "{figure:?}"
    );
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
    let (again, payloads, _) = load_generation_input(&out).unwrap();
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

/// 도면 쪽 나눔: 표제(【… N】)는 바로 뒤 그림과 같은 쪽(keepNext), 그림 줄은 한 줄 간격,
/// 쪽을 넘는 그림은 비율대로 줄인다 — 표제 N+1 이 그림 N 아래 홀로 남고 그림 N+1 이 다음 쪽으로 밀리지 않게.
#[test]
fn figure_captions_stay_on_the_same_page_as_their_figures() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = session(&dir);
    let mut png = Vec::new();
    image::RgbImage::from_pixel(8, 16, image::Rgb([0, 0, 0]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let after = session.paragraphs()[3].id.clone();
    // 표제와 합쳐 쪽(본문 451 × 684pt)을 넘는 그림.
    session
        .insert_figure(&after, FigureKind::Chemistry, png, 400.0, 700.0)
        .unwrap();
    let bytes = session.docx().unwrap();
    let mut xml = String::new();
    std::io::Read::read_to_string(
        &mut zip::ZipArchive::new(std::io::Cursor::new(&bytes))
            .unwrap()
            .by_name("word/document.xml")
            .unwrap(),
        &mut xml,
    )
    .unwrap();
    let paragraphs: Vec<&str> = xml
        .split("<w:p>")
        .skip(1)
        .map(|p| p.split("</w:p>").next().unwrap())
        .collect();
    for caption in ["【수학식 1】", "【화학식 1】"] {
        let at = paragraphs
            .iter()
            .position(|p| p.contains(caption))
            .unwrap_or_else(|| panic!("{caption} 표제"));
        assert!(
            paragraphs[at].contains("<w:keepNext/>"),
            "{caption} 는 다음 그림과 같은 쪽"
        );
        let figure = paragraphs[at + 1];
        assert!(figure.contains("<w:drawing>"), "{caption} 바로 뒤가 그림");
        assert!(
            figure.contains(r#"<w:spacing w:after="400" w:line="240" w:lineRule="auto"/>"#),
            "그림 줄은 한 줄 간격(배수 2.8 이 그림 높이에 곱해지지 않게)"
        );
    }
    // 글 문단 · 그림 앞이 아닌 표제는 묶지 않는다.
    assert_eq!(xml.matches("<w:keepNext/>").count(), 2);
    // 큰 그림은 630pt 높이로 줄인다(400×700 → 360×630).
    let emu = 12_700;
    assert!(
        xml.contains(&format!(
            r#"<wp:extent cx="{}" cy="{}"/>"#,
            360 * emu,
            630 * emu
        )),
        "쪽을 넘는 그림은 비율대로 줄인다"
    );
    // 작은 그림(40×20pt)은 그대로.
    assert!(xml.contains(&format!(
        r#"<wp:extent cx="{}" cy="{}"/>"#,
        40 * emu,
        20 * emu
    )));
}

/// 도면 6장 Word 원고 → HLT. 그림 크기(pt)는 통특기 조판이 쪽마다 1~2장을 놓게 고른다.
fn drawings_hlt(dir: &tempfile::TempDir) -> PathBuf {
    use std::io::Write;
    const SIZES: [(u64, u64); 6] = [
        (400, 260),
        (400, 260),
        (380, 420),
        (300, 200),
        (300, 200),
        (420, 560),
    ];
    let paragraph = |text: &str| format!("<w:p><w:r><w:t>{text}</w:t></w:r></w:p>");
    let mut body: String = [
        "【발명의 설명】",
        "【발명의 명칭】",
        "시험 장치",
        "【기술분야】",
        "본 발명은 시험 장치에 관한 것이다.",
        "【청구범위】",
        "【청구항 1】",
        "시험 장치.",
        "【요약서】",
        "【요약】",
        "시험 장치.",
        "【대표도】",
        "도 1",
        "【도면】",
    ]
    .iter()
    .map(|text| paragraph(text))
    .collect();
    let mut rels = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>"#,
    );
    let mut pngs = Vec::new();
    for (n, (w, h)) in SIZES.iter().enumerate().map(|(i, size)| (i + 1, size)) {
        body.push_str(&paragraph(&format!("【도 {n}】")));
        body.push_str(&format!(
            r#"<w:p><w:r><w:drawing><wp:inline><wp:extent cx="{}" cy="{}"/><a:graphic><a:graphicData><pic:pic><pic:blipFill><a:blip r:embed="rIdImg{n}"/></pic:blipFill></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"#,
            w * 12_700,
            h * 12_700
        ));
        rels.push_str(&format!(
            r#"<Relationship Id="rIdImg{n}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image{n}.png"/>"#
        ));
        let mut png = Vec::new();
        image::RgbImage::from_pixel(*w as u32, *h as u32, image::Rgb([255, 255, 255]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        pngs.push(png);
    }
    rels.push_str("</Relationships>");
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"><w:body>{body}</w:body></w:document>"#
    );
    let input = dir.path().join("도면.docx");
    {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&input).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        let mut part = |name: &str, bytes: &[u8]| {
            zip.start_file(name, options).unwrap();
            zip.write_all(bytes).unwrap();
        };
        part("[Content_Types].xml", b"<Types/>");
        part("word/document.xml", document.as_bytes());
        part("word/_rels/document.xml.rels", rels.as_bytes());
        for (index, png) in pngs.iter().enumerate() {
            part(&format!("word/media/image{}.png", index + 1), png);
        }
        zip.finish().unwrap();
    }
    let (document, payloads, _) = load_generation_input(&input).unwrap();
    let hlt = dir.path().join("도면.HLT");
    save_generated_hlt_as_new(
        &input,
        &document,
        GenerationDialect::PatentSpecificationKo,
        &payloads,
        &hlt,
        WriteOptions::default(),
    )
    .unwrap();
    hlt
}

/// 통특기가 새 쪽에 놓은 도면 표제 앞에서 Word 도 쪽을 나눈다 — 한 쪽에 「도 N 표제 · 그림 · 도 N+1 표제」가
/// 오고 그림 N+1 이 다음 쪽으로 밀리던 결함(쪽마다 표제 · 그림 짝이 맞게).
#[test]
fn drawing_pages_follow_the_hlt_page_layout() {
    let dir = tempfile::tempdir().unwrap();
    let hlt = drawings_hlt(&dir);
    // 통특기 조판: 【도 1】·【도 2】 | 【도 3】 | 【도 4】·【도 5】 | 【도 6】.
    let document = hlt_core::inspect_path(&hlt, hlt_core::InspectOptions::default()).unwrap();
    let (_, inflated) = hlt_core::inflate_hlt(&hlt).unwrap();
    let (heights, _, pages) = hlt_core::page_layout(&document, &inflated);
    let hlt_page = |caption: &str| {
        let paragraph = document
            .paragraphs
            .iter()
            .find(|p| p.text.trim_start_matches('\x0c') == caption)
            .unwrap();
        let line = document
            .lines
            .iter()
            .position(|l| l.paragraph_id == paragraph.id)
            .unwrap();
        let line = (line..).find(|l| heights[*l] >= 0).unwrap();
        pages[line]
    };
    let figure_pages: Vec<usize> = (1..=6).map(|n| hlt_page(&format!("【도 {n}】"))).collect();
    let first = figure_pages[0];
    assert_eq!(
        figure_pages,
        [first, first, first + 1, first + 2, first + 2, first + 3],
        "시험 문서의 통특기 조판"
    );

    let bytes = EditorSession::open(&hlt).unwrap().docx().unwrap();
    let mut xml = String::new();
    std::io::Read::read_to_string(
        &mut zip::ZipArchive::new(std::io::Cursor::new(&bytes))
            .unwrap()
            .by_name("word/document.xml")
            .unwrap(),
        &mut xml,
    )
    .unwrap();
    let paragraph_of = |caption: &str| {
        xml.split("<w:p>")
            .find(|p| p.contains(&format!(">{caption}<")))
            .unwrap()
            .split("</w:p>")
            .next()
            .unwrap()
            .to_string()
    };
    for n in 1..=6 {
        let caption = paragraph_of(&format!("【도 {n}】"));
        assert!(
            caption.contains("<w:keepNext/>"),
            "【도 {n}】 는 제 그림과 같은 쪽"
        );
        let new_page = n > 1 && figure_pages[n - 1] > figure_pages[n - 2];
        assert_eq!(
            caption.contains(r#"<w:br w:type="page"/>"#),
            new_page,
            "【도 {n}】 앞 쪽나눔은 통특기 조판대로"
        );
    }
    // 본문 · 다른 표제에는 쪽나눔을 새로 달지 않는다(【청구범위】 · 【요약서】 · 【도면】 의 원래 나눔만).
    assert_eq!(xml.matches(r#"<w:br w:type="page"/>"#).count(), 3 + 3);
}
