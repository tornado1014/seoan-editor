//! 보정서 보기 HLT: dta XML → 항목 → 의견서 틀로 구운 HLT 를 다시 열어 문단 순서를 본다.

use hlt_core::{EditorSession, amendment_hlt_from_items, amendment_items};

const DTA: &str = r#"<KIPO><AmendDOC><Amendment>
<AmendBody elementName="p" attributeValue="0010" status="A">
<p num="0010">제어부는 센서의 출력에 따라 모터를 구동한다.</p>
</AmendBody>
<AmendBody elementName="claim" attributeValue="3" status="D"></AmendBody>
<AmendBody elementName="claim" attributeValue="4" status="I"><claim num="4"><claim-text>제1항에 있어서,<br/>스위치를 더 포함하는 장치.</claim-text></claim></AmendBody>
</Amendment></AmendDOC></KIPO>"#;

#[test]
fn items_follow_the_document_order() {
    let items = amendment_items(DTA);
    let summary: Vec<(&str, &str)> = items
        .iter()
        .map(|item| (item.target.as_str(), item.method.as_str()))
        .collect();
    assert_eq!(
        summary,
        [
            ("식별번호 [0010]", "정정"),
            ("청구항 3", "삭제"),
            ("청구항 4", "추가")
        ]
    );
    assert_eq!(
        items[2].content,
        "제1항에 있어서,\n스위치를 더 포함하는 장치."
    );
}

#[test]
fn view_hlt_lists_target_method_and_content() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("보기.HLT");
    amendment_hlt_from_items(&amendment_items(DTA), &output).unwrap();
    let session = EditorSession::open(&output).unwrap();
    let texts: Vec<String> = session
        .paragraphs()
        .iter()
        .map(|paragraph| paragraph.text.clone())
        .filter(|text| !text.is_empty())
        .collect();
    assert_eq!(
        texts,
        [
            "【보정대상항목】 식별번호 [0010]",
            "【보정방법】 정정",
            "【보정내용】",
            "제어부는 센서의 출력에 따라 모터를 구동한다.",
            "【보정대상항목】 청구항 3",
            "【보정방법】 삭제",
            "【보정대상항목】 청구항 4",
            "【보정방법】 추가",
            "【보정내용】",
            "제1항에 있어서,",
            "스위치를 더 포함하는 장치.",
        ]
    );
}

#[test]
fn no_items_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    assert!(amendment_hlt_from_items(&[], &dir.path().join("x.HLT")).is_err());
}
