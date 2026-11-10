//! 원문 PDF → 문단(P9). 입력 PDF 는 krilla(`pdf` feature)로 테스트 안에서 굽는다 — 실물 문서 없음.
#![cfg(all(feature = "pdf", feature = "pdf-read"))]

use hlt_core::{CoreError, ErrorCode, pdf_paragraphs};
use krilla::{
    Document,
    geom::{PathBuilder, Point, Rect},
    page::PageSettings,
    paint::Fill,
    text::{Font, TextDirection},
};

const FONT_CANDIDATES: &[&str] = &[
    "/System/Library/Fonts/Supplemental/Arial.ttf",
    "/Library/Fonts/Arial.ttf",
    "C:\\Windows\\Fonts\\arial.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
];

fn font() -> Font {
    let bytes = FONT_CANDIDATES
        .iter()
        .find_map(|path| std::fs::read(path).ok())
        .expect("테스트용 글꼴(Arial 또는 DejaVuSans)을 찾지 못했습니다");
    Font::new(bytes.into(), 0).expect("글꼴을 열지 못했습니다")
}

/// 쪽마다 (x, 기준선 y, 글) 목록. 612x792, 글자 크기 10.
fn build_pdf(pages: &[Vec<(f32, f32, &str)>]) -> Vec<u8> {
    let font = font();
    let mut document = Document::new();
    for lines in pages {
        let mut page = document.start_page_with(PageSettings::from_wh(612.0, 792.0).unwrap());
        let mut surface = page.surface();
        for (x, y, text) in lines {
            surface.draw_text(
                Point::from_xy(*x, *y),
                font.clone(),
                10.0,
                text,
                false,
                TextDirection::Auto,
            );
        }
        surface.finish();
        page.finish();
    }
    document.finish().expect("PDF 굽기")
}

fn write_temp(bytes: &[u8]) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.pdf");
    std::fs::write(&path, bytes).unwrap();
    (dir, path)
}

const HEADER: &str = "WO 2025/123456";
const P1: [&str; 3] = [
    "[0001] The first paragraph describes a device with several parts that cooperate",
    "to hold a sample while a second part measures the sample in a quiet room with care",
    "and then stops.",
];
const P2: [&str; 6] = [
    "[0002] The second paragraph is longer and begins near the end of the first page so that",
    "it must continue after the page break without being split into two separate items for",
    "the reader and it mentions a semi-",
    "conductor layer that is thin and flat and wide enough to fill this particular line fully",
    "up to the edge of the text column where the body text normally ends on every line of",
    "the page and finally ends here.",
];
const P3: [&str; 2] = [
    "[0003] The third paragraph starts on the second page after a visible gap and is also",
    "rather short.",
];

fn synthetic() -> Vec<u8> {
    let page1 = vec![
        (72.0, 30.0, HEADER),
        (300.0, 760.0, "1"),
        (30.0, 164.0, "5"),
        (72.0, 100.0, P1[0]),
        (72.0, 114.0, P1[1]),
        (72.0, 128.0, P1[2]),
        (72.0, 150.0, P2[0]),
        (72.0, 164.0, P2[1]),
        (72.0, 178.0, P2[2]),
        (72.0, 192.0, P2[3]),
        (72.0, 206.0, P2[4]),
    ];
    let page2 = vec![
        (72.0, 30.0, HEADER),
        (300.0, 760.0, "2"),
        (30.0, 114.0, "10"),
        (72.0, 100.0, P2[5]),
        (72.0, 128.0, P3[0]),
        (72.0, 142.0, P3[1]),
    ];
    build_pdf(&[page1, page2])
}

#[test]
fn paragraphs_without_header_page_number_or_line_numbers() {
    let (_dir, path) = write_temp(&synthetic());
    let paragraphs = pdf_paragraphs(&path).expect("합성 PDF 를 읽어야 합니다");
    let expected = vec![P1.join(" "), P2.join(" "), P3.join(" ")];
    assert_eq!(paragraphs, expected);
}

fn expect_error(bytes: &[u8], code: ErrorCode, reason: &str) -> CoreError {
    let (_dir, path) = write_temp(bytes);
    let error = pdf_paragraphs(&path).expect_err("오류여야 합니다");
    assert_eq!(error.code, code, "{error:?}");
    assert!(error.message.contains(reason), "{}", error.message);
    error
}

#[test]
fn page_without_text_layer_is_refused() {
    let mut document = Document::new();
    let mut page = document.start_page_with(PageSettings::from_wh(612.0, 792.0).unwrap());
    let mut surface = page.surface();
    let mut path = PathBuilder::new();
    path.push_rect(Rect::from_xywh(50.0, 50.0, 200.0, 100.0).unwrap());
    surface.set_fill(Some(Fill::default()));
    surface.draw_path(&path.finish().unwrap());
    surface.finish();
    page.finish();
    let bytes = document.finish().unwrap();
    expect_error(
        &bytes,
        ErrorCode::UnsupportedFormat,
        "글을 읽을 수 없는 PDF 입니다(스캔본은 지원하지 않습니다).",
    );
}

#[test]
fn empty_and_non_pdf_files_are_errors_not_panics() {
    expect_error(b"", ErrorCode::UnsupportedFormat, "PDF");
    expect_error(
        b"this is plainly not a pdf file at all",
        ErrorCode::UnsupportedFormat,
        "PDF",
    );
    // 머리만 PDF 인 잘린 파일.
    let bytes = synthetic();
    expect_error(
        &bytes[..bytes.len() / 3],
        ErrorCode::UnsupportedFormat,
        "PDF",
    );
}

#[test]
fn password_protected_pdf_is_refused() {
    use lopdf::{Dictionary, Document as Lopdf, Object, StringFormat};
    let mut document = Lopdf::load_mem(&synthetic()).unwrap();
    let mut encrypt = Dictionary::new();
    encrypt.set("Filter", Object::Name(b"Standard".to_vec()));
    encrypt.set("V", 1);
    encrypt.set("R", 2);
    encrypt.set("P", -4);
    encrypt.set("O", Object::String(vec![7; 32], StringFormat::Literal));
    encrypt.set("U", Object::String(vec![9; 32], StringFormat::Literal));
    let id = document.add_object(Object::Dictionary(encrypt));
    document.trailer.set("Encrypt", Object::Reference(id));
    let mut bytes = Vec::new();
    document.save_to(&mut bytes).unwrap();
    expect_error(
        &bytes,
        ErrorCode::UnsupportedFormat,
        "암호가 걸린 PDF 입니다.",
    );
}

#[test]
fn extra_line_gap_alone_splits_paragraphs_and_lone_page_number_is_dropped() {
    let full = [
        "aaaa bbbb cccc dddd eeee ffff gggg hhhh iiii jjjj kkkk llll mmmm nnnn oooo pppp qqqq rrrr",
        "ssss tttt uuuu vvvv wwww xxxx yyyy zzzz aaaa bbbb cccc dddd eeee ffff gggg hhhh iiii jjjj",
        "kkkk llll mmmm nnnn oooo pppp qqqq rrrr ssss tttt uuuu vvvv wwww xxxx yyyy zzzz aaaa bbbb",
        "cccc dddd eeee ffff gggg hhhh iiii jjjj kkkk llll mmmm nnnn oooo pppp qqqq rrrr ssss tttt",
    ];
    let page = vec![
        (300.0, 760.0, "- 7 -"),
        (72.0, 100.0, full[0]),
        (72.0, 114.0, full[1]),
        (72.0, 138.0, full[2]),
        (72.0, 152.0, full[3]),
    ];
    let (_dir, path) = write_temp(&build_pdf(&[page]));
    let paragraphs = pdf_paragraphs(&path).unwrap();
    assert_eq!(
        paragraphs,
        vec![
            format!("{} {}", full[0], full[1]),
            format!("{} {}", full[2], full[3])
        ]
    );
}
