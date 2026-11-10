//! 비교 기준으로 워드를 쓴다: 굽지 않고 문단으로 읽고(`docx_baseline`), 식별번호 쪽이 다른 판과도 짝이 맞는다.

use std::io::{Cursor, Write};

use hlt_core::{EditorSession, SpecKind, SpecOptions, docx_baseline, save_new_specification};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;")
}

/// 문단 글들을 그대로 담은 워드 파일.
fn word_with(dir: &std::path::Path, texts: &[String]) -> std::path::PathBuf {
    let body: String = texts
        .iter()
        .map(|text| {
            format!(
                "<w:p><w:r><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>",
                xml_escape(text)
            )
        })
        .collect();
    let document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:v=\"urn:schemas-microsoft-com:vml\"><w:body>{body}</w:body></w:document>"
    );
    let mut bytes = Cursor::new(Vec::new());
    {
        let mut writer = ZipWriter::new(&mut bytes);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for (name, content) in [
            ("[Content_Types].xml", "<Types/>"),
            ("word/document.xml", document.as_str()),
            ("word/styles.xml", "<w:styles xmlns:w=\"x\"/>"),
            (
                "word/_rels/document.xml.rels",
                "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"/>",
            ),
        ] {
            writer.start_file(name, options).unwrap();
            writer.write_all(content.as_bytes()).unwrap();
        }
        writer.finish().unwrap();
    }
    let path = dir.join("기준.docx");
    std::fs::write(&path, bytes.get_ref()).unwrap();
    path
}

#[test]
fn docx_baseline_reads_identification_numbers_off_the_text() {
    let dir = tempfile::tempdir().unwrap();
    let path = word_with(
        dir.path(),
        &[
            "【발명의 설명】".into(),
            "[0001] 첫째 문단이다.".into(),
            "【0002】 둘째 문단이다.".into(),
            "[0003] 셋째 문단이다.".into(),
            "【청구범위】".into(),
            "【청구항 1】".into(),
            "제1항 내용이다.".into(),
        ],
    );
    let paragraphs = docx_baseline(&path).unwrap();
    let find = |text: &str| paragraphs.iter().find(|p| p.text == text).unwrap();
    assert_eq!(find("셋째 문단이다.").number.as_deref(), Some("0003"));
    assert_eq!(find("둘째 문단이다.").number.as_deref(), Some("0002"));
    // 청구항 번호는 글 그대로(표제 문단 · 번호 없음).
    assert_eq!(find("【청구항 1】").number, None);
    assert_eq!(find("제1항 내용이다.").number, None);
}

/// 식별번호 없는 HLT(그냥 HLT) 한 판과, 같은 글에 `[0001]…` 을 붙인 워드 기준.
fn unnumbered_hlt_and_numbered_word(
    dir: &std::path::Path,
) -> (EditorSession, std::path::PathBuf, String) {
    let path = dir.join("명세서.HLT");
    save_new_specification(&path, SpecKind::Patent, SpecOptions::default()).unwrap();
    let mut session = EditorSession::open(&path).unwrap();
    let paragraphs = session.paragraphs();
    let start = paragraphs
        .iter()
        .position(|p| p.text.trim() == "【발명의 설명】")
        .unwrap();
    let end = paragraphs
        .iter()
        .position(|p| matches!(p.text.trim(), "【청구범위】" | "【도면】"))
        .unwrap();
    let body: Vec<_> = (start + 1..end)
        .filter(|&at| {
            let heading = |k: usize| paragraphs[k].text.trim().starts_with('【');
            !heading(at) && !(at > 0 && paragraphs[at - 1].text.trim() == "【발명의 명칭】")
        })
        .collect();
    assert!(body.len() >= 2, "시험 틀에 본문 문단이 둘은 있어야 한다");
    let edits: Vec<(String, String)> = body
        .iter()
        .enumerate()
        .map(|(k, &at)| {
            (
                paragraphs[at].id.clone(),
                format!("{}번째 본문 문장이다.", k + 1),
            )
        })
        .collect();
    session.replace_texts(&edits).unwrap();
    let paragraphs = session.paragraphs();
    let mut number = 0;
    let texts: Vec<String> = paragraphs
        .iter()
        .enumerate()
        .map(|(at, p)| {
            if body.contains(&at) {
                number += 1;
                format!("[{number:04}] {}", p.text)
            } else {
                p.text.clone()
            }
        })
        .collect();
    let word = word_with(dir, &texts);
    let target = body[1];
    (session, word, paragraphs[target].id.clone())
}

#[test]
fn comparing_numbered_word_with_unnumbered_hlt_marks_only_the_edit() {
    let dir = tempfile::tempdir().unwrap();
    let (mut session, word, target) = unnumbered_hlt_and_numbered_word(dir.path());
    session.compare_with(&word).unwrap();
    let same = session.amendment_marks_against_file().unwrap();
    assert!(
        same.changed.is_empty() && same.added.is_empty() && same.removed.is_empty(),
        "{same:?}"
    );
    let text = session
        .paragraphs()
        .into_iter()
        .find(|p| p.id == target)
        .unwrap()
        .text;
    session
        .replace_text(&target, &format!("{text} 덧붙인 말이다."))
        .unwrap();
    let marks = session.amendment_marks_against_file().unwrap();
    assert_eq!(marks.changed.len(), 1, "{marks:?}");
    assert!(
        marks.added.is_empty() && marks.removed.is_empty(),
        "{marks:?}"
    );
}

#[test]
fn a_word_baseline_does_not_report_figures_as_changed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("도면있는.HLT");
    save_new_specification(
        &path,
        SpecKind::Patent,
        SpecOptions {
            drawings: true,
            ..Default::default()
        },
    )
    .unwrap();
    let mut session = EditorSession::open(&path).unwrap();
    let texts: Vec<String> = session
        .paragraphs()
        .iter()
        .map(|p| p.text.clone())
        .collect();
    let word = word_with(dir.path(), &texts);
    session.compare_with(&word).unwrap();
    let marks = session.amendment_marks_against_file().unwrap();
    assert!(
        !marks.changed.iter().any(|label| label.starts_with("도 ")),
        "{marks:?}"
    );
}
