use hlt_core::{
    AmendmentOptions, FinAmendment, FinExportOptions, KipoExportOptions, amendable_elements,
    build_amendment_dta, build_amendment_xml, build_xresult, diff_elements, export_fin_path,
};
use std::{
    io::{Cursor, Read},
    path::{Path, PathBuf},
};

mod support;

fn repo(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path)
}

#[test]
fn writes_the_xresult_index_like_the_authoritative_fin() {
    // xresult.inf: [APPLICATION] / APPNAME / APPTIME · [AMENDMENT] / AMDCNT / AMDnnn.
    let options = FinExportOptions {
        hlz_name: "1020230000001.hlz".into(),
        app_date: "2024-01-02".into(),
        app_time: "240102120".into(),
        image_time: String::new(),
        amendments: vec![
            FinAmendment {
                file_name: "amend1.dta".into(),
                date: "2024-06-03".into(),
                bytes: Vec::new(),
            },
            FinAmendment {
                file_name: "amend2.dta".into(),
                date: "2025-03-04".into(),
                bytes: Vec::new(),
            },
        ],
    };
    let bytes = build_xresult(&options).unwrap();
    let (text, _, _) = encoding_rs::EUC_KR.decode(&bytes);

    assert_eq!(
        text,
        "[APPLICATION]\r\nAPPNAME=1020230000001.hlz,2024-01-02,1\r\nAPPTIME=240102120\r\n\
         [AMENDMENT]\r\nAMDCNT=2\r\nAMD001=amend1.dta,2024-06-03,1\r\n\
         AMD002=amend2.dta,2025-03-04,1\r\n"
    );
}

#[test]
fn builds_amendment_entries_and_dta_like_the_authoritative_amendment() {
    // 정본 보정서 실측: 바뀐 청구항은 status="A" 로 본문을 통째로, 없어진 청구항은
    // status="D" 로 본문 없이. 그대로인 청구항은 보정서에 넣지 않는다.
    let claims = |list: &[(&str, &str)]| {
        list.iter()
            .map(|(n, t)| format!("<claim num=\"{n}\"><claim-text>{t}</claim-text></claim>\n"))
            .collect::<String>()
    };
    let before = amendable_elements(&claims(&[
        ("1", "이전 1항"),
        ("2", "그대로"),
        ("3", "삭제될 3항"),
    ]))
    .into_iter()
    .map(|(key, markup)| (key, Some(markup)))
    .collect();
    let after = amendable_elements(&claims(&[("1", "보정된 1항"), ("2", "그대로")]));
    let entries = diff_elements(&before, &after);

    assert_eq!(entries.len(), 2);
    assert_eq!(
        (
            entries[0].status.as_str(),
            entries[0].attribute_value.as_str()
        ),
        ("A", "1")
    );
    assert!(
        entries[0]
            .body
            .contains("<claim num=\"1\"><claim-text>보정된 1항")
    );
    assert_eq!(
        (
            entries[1].status.as_str(),
            entries[1].attribute_value.as_str()
        ),
        ("D", "3")
    );
    assert!(entries[1].body.is_empty());

    let xml = build_amendment_xml(
        &entries,
        &AmendmentOptions {
            document_id: "240102120".into(),
            invention_en_title: "시험 발명 명칭".into(),
            ..AmendmentOptions::default()
        },
    );
    assert!(xml.contains("<KIPO keapsVersion=\"5.6\" dtaVersion=\"003\" editorKind=\"K\">"));
    assert!(xml.contains("<AmendDOC docType=\"PCTPatentCAFDOC\" documentID=\"240102120\">"));
    assert!(xml.contains(
        "<AmendBody elementName=\"claim\" status=\"A\" attributeName=\"num\" attributeValue=\"1\">"
    ));
    assert!(xml.contains(
        "<AmendBody elementName=\"claim\" status=\"D\" attributeName=\"num\" attributeValue=\"3\">\n</AmendBody>"
    ));

    let dta = build_amendment_dta("amend1.dta", &xml).unwrap();
    let mut archive = zip::ZipArchive::new(Cursor::new(dta)).unwrap();
    assert_eq!(archive.len(), 1);
    let mut entry = archive.by_index(0).unwrap();
    // 통특기 dta 의 안쪽 이름은 CP949 원 바이트(UTF-8 깃발 없음).
    // 예전엔 UTF-8 이름을 고정하고 있었다 — 통특기가 xresult 의 CP949 이름으로 찾는 항목이라 바이트가 맞아야 한다.
    assert_eq!(
        entry.name_raw(),
        encoding_rs::EUC_KR.encode("amend1.xml").0.as_ref()
    );
    let mut inner = String::new();
    entry.read_to_string(&mut inner).unwrap();
    assert_eq!(inner, xml);
}

#[test]
#[ignore = "실물 work/pct_a 필요(공개 저장소에 없음 · 있으면 --ignored 로)"]
fn exports_a_submission_set_from_the_native_pct_file() {
    // 정본과 같은 3층 구조: .fin(zip) → .hlz(zip) → KIPO XML + 도면 JPG 69장.
    let export = export_fin_path(
        &repo("work/pct_a/diagnostics/pct_a_source.HLT"),
        &KipoExportOptions {
            document_id: "1700000001".into(),
            ..KipoExportOptions::default()
        },
        &FinExportOptions {
            hlz_name: "1020230000001.hlz".into(),
            app_date: "2024-01-02".into(),
            app_time: "240102120".into(),
            image_time: String::new(),
            amendments: Vec::new(),
        },
    )
    .unwrap();

    assert_eq!(export.image_count, 69);
    assert_eq!(export.claim_count, 69);
    assert_eq!(export.figure_count, 69);

    let mut fin = zip::ZipArchive::new(Cursor::new(export.fin.clone())).unwrap();
    let names: Vec<String> = (0..fin.len())
        .map(|index| fin.by_index(index).unwrap().name().to_string())
        .collect();
    assert_eq!(names, ["1020230000001.hlz", "xresult.inf"]);

    let mut hlz_bytes = Vec::new();
    fin.by_name("1020230000001.hlz")
        .unwrap()
        .read_to_end(&mut hlz_bytes)
        .unwrap();
    let mut hlz = zip::ZipArchive::new(Cursor::new(hlz_bytes)).unwrap();
    let inner: Vec<String> = (0..hlz.len())
        .map(|index| hlz.by_index(index).unwrap().name().to_string())
        .collect();
    assert_eq!(inner.len(), 70);
    assert_eq!(inner[0], "1020230000001.xml");
    assert_eq!(inner[1], "pct00001.jpg");
    assert_eq!(inner[69], "pct00069.jpg");

    let mut xml = String::new();
    hlz.by_name("1020230000001.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    assert!(xml.contains("<p num=\"0179\">"));
    assert!(xml.contains(
        "<img id=\"i0001\" he=\"190\" wi=\"133\" file=\"pct00001.jpg\" img-format=\"jpg\"/>"
    ));

    let mut jpeg = Vec::new();
    hlz.by_name("pct00001.jpg")
        .unwrap()
        .read_to_end(&mut jpeg)
        .unwrap();
    assert_eq!(&jpeg[..3], b"\xff\xd8\xff");
    let decoded = image::load_from_memory(&jpeg).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (493, 702));
}

/// 국내 출원서의 도면은 **1비트** 라 판독기가 픽셀을 못 낸다(`tiff_geometry` 가
/// `bits % 8 != 0` 을 거른다. 압축이든 무압축이든 같다). 그것을 JPEG 로 다시 굽겠다고
/// 들면 `pixel_format` 으로 죽고, 이름만 `.tif` 로 붙이면 알맹이가 JPEG 인 파일이 나온다.
///
/// 국내 제출 세트의 `.hlz` 는
/// `pat00001.jpg` 하나와 `pat00002.tif` ~ `pat00012.tif` 열하나를 **섞어** 담고 있고
/// XML 의 `img-format` 도 그림마다 갈린다. 이 표본의 페이로드가 정확히 그 모양이다:
/// 열둘은 1비트(우리가 못 푸는 것), 하나는 8비트다.
///
/// 즉 형식은 문서별 기본값이 아니라 **그림마다 원본이 풀리느냐**로 갈린다.
/// 풀리면 JPEG 로 굽고, 안 풀리면 원본 TIFF 바이트를 그대로 싣는다.
#[test]
fn a_domestic_submission_ships_unreadable_drawings_as_their_original_tiff() {
    let fixture = support::application_table_fixture();
    let export = export_fin_path(
        fixture.path(),
        &KipoExportOptions {
            document_id: "1979848".into(),
            ..KipoExportOptions::default()
        },
        &FinExportOptions {
            hlz_name: "1020250000002.hlz".into(),
            app_date: "2026-08-27".into(),
            app_time: "260827100".into(),
            image_time: String::new(),
            amendments: Vec::new(),
        },
    )
    .unwrap();

    let mut fin = zip::ZipArchive::new(Cursor::new(export.fin.clone())).unwrap();
    let mut hlz_bytes = Vec::new();
    fin.by_name("1020250000002.hlz")
        .unwrap()
        .read_to_end(&mut hlz_bytes)
        .unwrap();
    let mut hlz = zip::ZipArchive::new(Cursor::new(hlz_bytes)).unwrap();

    let names: Vec<String> = (0..hlz.len())
        .map(|index| hlz.by_index(index).unwrap().name().to_string())
        .collect();
    let mut xml = String::new();
    hlz.by_name("1020250000002.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();

    // 이름이 말하는 형식과 알맹이가 같아야 한다.
    let mut tif = 0;
    for name in names.iter().filter(|name| !name.ends_with(".xml")) {
        let mut bytes = Vec::new();
        hlz.by_name(name).unwrap().read_to_end(&mut bytes).unwrap();
        if name.ends_with(".tif") {
            assert_eq!(&bytes[..4], b"II*\0", "{name} 알맹이가 TIFF 가 아니다");
            tif += 1;
        } else if name.ends_with(".jpg") {
            assert_eq!(
                &bytes[..3],
                b"\xff\xd8\xff",
                "{name} 알맹이가 JPEG 가 아니다"
            );
        } else {
            panic!("모르는 형식 {name}");
        }
        let stem = name.trim_end_matches(".tif").trim_end_matches(".jpg");
        let extension = &name[stem.len() + 1..];
        assert!(
            xml.contains(&format!("file=\"{name}\" img-format=\"{extension}\"")),
            "{name} 의 img-format 이 XML 과 안 맞는다"
        );
    }
    assert!(tif > 0, "못 푸는 도면이 TIFF 로 안 실렸다");
}

#[test]
fn amendment_covers_inserted_claims_and_non_claim_elements() {
    // 보정서 규칙: 살아 있지 않은 번호(지운 항 · 새 항)에 넣으면 I · 문단은 본문 뒤 줄바꿈 ·
    // 명칭·요약은 attributeName 없이 · 문서 순서(명칭 → 문단 → 청구항 → 요약).
    let doc = |title: &str, p2: &str, claims: &str, summary: &str| {
        format!(
            "<description><invention-title>{title}</invention-title>\n<technical-field><p num=\"0001\">가</p>\n<p num=\"0002\">{p2}</p>\n</technical-field></description>\n<claims>{claims}</claims>\n<abstract><summary><p num=\"0001a\">{summary}</p>\n</summary></abstract>"
        )
    };
    let mut before: std::collections::BTreeMap<_, _> =
        amendable_elements(&doc("명칭", "나", "<claim num=\"1\"><claim-text>일</claim-text></claim>\n<claim num=\"2\"><claim-text>이</claim-text></claim>\n", "요약"))
            .into_iter()
            .map(|(key, markup)| (key, Some(markup)))
            .collect();
    hlt_core::apply_amendment(
        &mut before,
        "<AmendBody elementName=\"claim\" status=\"D\" attributeName=\"num\" attributeValue=\"2\">\n</AmendBody>\n",
    )
    .unwrap();
    let after = amendable_elements(&doc(
        "새 명칭",
        "다",
        "<claim num=\"1\"><claim-text>일</claim-text></claim>\n<claim num=\"2\"><claim-text>되살린 이</claim-text></claim>\n<claim num=\"3\"><claim-text>새 삼</claim-text></claim>\n",
        "새 요약",
    ));
    let xml = build_amendment_xml(
        &diff_elements(&before, &after),
        &AmendmentOptions::default(),
    );
    let bodies: Vec<&str> = xml
        .split("<AmendBody ")
        .skip(1)
        .map(|b| b.split_once('>').unwrap().0)
        .collect();
    assert_eq!(
        bodies,
        [
            "elementName=\"invention-title\" status=\"A\"",
            "elementName=\"p\" status=\"A\" attributeName=\"num\" attributeValue=\"0002\"",
            "elementName=\"claim\" status=\"I\" attributeName=\"num\" attributeValue=\"2\"",
            "elementName=\"claim\" status=\"I\" attributeName=\"num\" attributeValue=\"3\"",
            "elementName=\"summary\" status=\"A\"",
        ]
    );
    assert!(xml.contains("<p num=\"0002\">다</p>\n</AmendBody>"));
    assert!(xml.contains("</claim></AmendBody>"));
    assert!(xml.contains("</summary></AmendBody>"));
    // 살아 있는 요소에 I · 지운 자리에 A · 모르는 요소는 앞선 보정서 적용에서 거절한다.
    for bad in [
        "<AmendBody elementName=\"claim\" status=\"I\" attributeName=\"num\" attributeValue=\"1\">\n<claim num=\"1\"/></AmendBody>",
        "<AmendBody elementName=\"claim\" status=\"A\" attributeName=\"num\" attributeValue=\"2\">\n<claim num=\"2\"/></AmendBody>",
        "<AmendBody elementName=\"figure\" status=\"A\" attributeName=\"num\" attributeValue=\"1\">\n<figure num=\"1\"/>\n</AmendBody>",
    ] {
        assert!(
            hlt_core::apply_amendment(&mut before.clone(), bad).is_err(),
            "{bad}"
        );
    }
}

/// 통특기(DynaZip)는 UTF-8 깃발 이름을 못 푼다 — hlz 안 한글 XML 이름도 CP949 원 바이트여야 한다
/// (UTF-8 이름이면 `.fin` 을 열 때 「Error creating output file」 · 이름만 CP949 로 바꾸면 열린다).
#[test]
fn hlz_names_its_korean_xml_in_cp949() {
    let export = hlt_core::KipoExport {
        xml: "<KIPO/>".into(),
        xml_sha256: String::new(),
        claims: Vec::new(),
        images: Vec::new(),
        paragraph_count: 0,
        claim_count: 0,
        figure_count: 0,
        numbered_by_id: false,
    };
    let name = "domestic_e_명세서_최종.xml";
    let hlz = hlt_core::build_hlz(name, &export, &Default::default(), &[], "").unwrap();
    let mut archive = zip::ZipArchive::new(Cursor::new(hlz)).unwrap();
    let mut entry = archive.by_index(0).unwrap();
    assert_eq!(
        entry.name_raw(),
        encoding_rs::EUC_KR.encode(name).0.as_ref()
    );
    let mut inner = String::new();
    entry.read_to_string(&mut inner).unwrap();
    assert_eq!(inner, "<KIPO/>");
}
