use hlt_core::{
    KipoExportOptions, display_units_to_mm, display_units_to_mm_import, export_kipo_xml_path,
};
use std::path::{Path, PathBuf};

fn repo(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path)
}

#[test]
fn exports_the_native_pct_file_like_the_authoritative_fin_xml() {
    // 통특기 제출 XML 기준:
    // 번호 문단 181개(0001~0179 + 0001a + 0002a), 청구항 69개, 도면 69개.
    // 밑줄 표제는 <u>…</u>, 표는 CALS 이고 열 폭은 셀 폭 × 1.25 (3600 → 4500).
    let options = KipoExportOptions {
        document_id: "1700000001".into(),
        ..KipoExportOptions::default()
    };
    let export =
        export_kipo_xml_path(&repo("work/pct_a/diagnostics/pct_a_source.HLT"), &options).unwrap();

    assert_eq!(export.paragraph_count, 179); // 본문 번호 문단 (요약 0001a, 대표도 0002a 제외)
    assert_eq!(export.claim_count, 69);
    assert_eq!(export.figure_count, 69);
    assert_eq!(export.images.len(), 69);

    assert!(
        export
            .xml
            .starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<KIPO")
    );
    assert!(
        export
            .xml
            .contains("<PCTPatentCAFDOC documentID=\"1700000001\">")
    );
    assert!(export.xml.contains("<p num=\"0001\"><u>관련 출원</u></p>"));
    let second = export.xml.split("<p num=\"0002\">").nth(1).unwrap();
    assert!(opens_with(second, 23, "d9addecc41f4491e"));
    // 표는 OASIS CALS 이고 tgroup 이 그 네임스페이스를 직접 선언해야 한다.
    // KIPO 공식 스키마(Epasl/INCLUDE/Schema/PatentCAFDOC.xsd)가 요구하고, 정본 .fin 도 같다.
    assert!(
        export
            .xml
            .contains("<tgroup xmlns='http://www.oasis-open.org/tables/exchange/1.0' cols=\"2\">")
    );
    assert!(
        export.xml.contains(
            "<colspec colnum=\"1\" align=\"justify\" colname=\"col1\" colwidth=\"4500\"/>"
        )
    );
    assert!(export.xml.contains("<title>구성품 시험 결과</title>"));
    // 표 셀 안 문자 서식. 통특기 XML 은 셀 안에 <b> 를
    // 13개 쓴다. 우리는 셀 글자를 그냥 escape 만 하고 있었다.
    assert!(
        export
            .xml
            .contains("<entry align=\"justify\" colname=\"col1\"><b>구성품</b></entry>")
    );
    assert!(
        export
            .xml
            .contains("<entry align=\"justify\" colname=\"col2\"><b>t-값</b></entry>")
    );
    assert!(
        export
            .xml
            .contains("<p num=\"0002a\"><figref num=\"1\"/></p>")
    );
    assert!(
        export
            .xml
            .contains("<claim num=\"1\"><claim-text>컴퓨터 구현 방법으로서,<br/>")
    );

    let first = &export.images[0];
    assert_eq!(first.file, "pct00001.jpg");
    assert_eq!((first.width_mm, first.height_mm), (133, 190));
}

#[test]
fn converts_display_units_to_millimetres() {
    // 실측 회귀: 표시 필드 355 → 133mm, 505 → 190mm, 256 → 96mm, 396 → 149mm.
    assert_eq!(display_units_to_mm(355), 133);
    assert_eq!(display_units_to_mm(505), 190);
    assert_eq!(display_units_to_mm(256), 96);
    assert_eq!(display_units_to_mm(396), 149);
    // 통특기 제출 변환(writer 문서 12장). 옛 계수 939/2500 은
    // 이 네 값을 +1 로 틀렸고(79/195/180/178), 3/8 은 481→181 을 틀린다.
    assert_eq!(display_units_to_mm(209), 78);
    assert_eq!(display_units_to_mm(518), 194);
    assert_eq!(display_units_to_mm(478), 179);
    assert_eq!(display_units_to_mm(473), 177);
    assert_eq!(display_units_to_mm(481), 181);
    assert_eq!(display_units_to_mm(340), 128);
    // import 규약(pct_b.fin 대 pct_b.HLT, 2026-09-06): 179/477 이 −1 로 틀리던 다섯 값.
    assert_eq!(display_units_to_mm_import(401), 151);
    assert_eq!(display_units_to_mm_import(449), 169);
    assert_eq!(display_units_to_mm_import(473), 178);
    assert_eq!(display_units_to_mm_import(518), 195);
    assert_eq!(display_units_to_mm_import(590), 222);
    assert_eq!(display_units_to_mm(401), 150);
}

/// 통특기 제출 XML 은 본문에 서식 태그를
/// 밑줄 8, 위첨자 18, 기울임 43, 굵게 1 로 쓴다(굵게 하나 더는 표 셀 안이라 `<entry>` 몫이다).
/// 우리는 표 뒤 꼬리 문단을 escape 만 하고, 끝 공백까지 걸친 스팬을 통째로 버려서
/// 밑줄 7, 위첨자 17, 기울임 33 만 썼다.
#[test]
fn carries_body_character_formats_at_the_authoritative_count() {
    let options = KipoExportOptions {
        document_id: "1700000001".into(),
        ..KipoExportOptions::default()
    };
    let export =
        export_kipo_xml_path(&repo("work/pct_a/diagnostics/pct_a_source.HLT"), &options).unwrap();

    // 표 셀 안 서식은 `<entry>` 가 따로 세므로 본문 집계에서 뺀다.
    let mut body = export.xml.clone();
    while let Some(start) = body.find("<entry") {
        let Some(end) = body[start..].find("</entry>") else {
            break;
        };
        body.replace_range(start..start + end + "</entry>".len(), "");
    }
    let count = |tag: &str| body.matches(&format!("<{tag}>")).count();
    assert_eq!(
        (count("u"), count("sup"), count("i"), count("b")),
        (8, 18, 43, 1)
    );

    // 표 뒤 꼬리 문단. 정본은 이 자리에 <i>p </i> 와 <sup>-16</sup> 을 쓴다.
    assert!(export.xml.contains("<i>p </i>= 0<i>:</i>0268"));
    assert!(export.xml.contains("2.2 x 10<sup>-16</sup>"));
    // 원문은 밑줄이 끝 공백까지 걸쳐 있다. 판독기가 문단 글자의 꼬리 공백을 잘라내므로
    // 스팬 끝을 글자 길이로 잘라 써야 한다. 정본은 공백을 태그 안에 넣는다.
    assert!(export.xml.split("<u>").skip(1).any(|part| {
        part.split("</u>")
            .next()
            .is_some_and(|inner| digest(inner) == "23e121803b8729dd")
    }));
}

mod support;

/// 국내 명세서 절 매핑. 통특기 제출 XML 기준:
/// `technical-field` 문단 1, `background-art` 문단 4, `tech-problem` 2, `tech-solution` 10,
/// `advantageous-effects` 3, `reference-signs-list` 1 이고
/// 셋은 `summary-of-invention` 안에 있다.
/// 우리는 【발명의 배경이 되는 기술】 표제를 몰라 그 문단들을 `technical-field` 에 흘려 넣었고,
/// 셋을 `description` 직계 자식으로 써서 스키마 게이트가 떨어졌고,
/// 부호의 설명을 줄마다 쪼개 여러 문단으로 썼다.
#[test]
fn maps_the_domestic_specification_sections_like_the_authoritative_fin() {
    let fixture = support::domestic_specification_fixture();
    let export = export_kipo_xml_path(fixture.path(), &KipoExportOptions::default()).unwrap();

    let section = |name: &str| -> String {
        let open = format!("<{name}>");
        let close = format!("</{name}>");
        let start = export
            .xml
            .find(&open)
            .unwrap_or_else(|| panic!("{name} 없음"));
        let end = export.xml[start..].find(&close).expect("닫는 태그");
        export.xml[start + open.len()..start + end].to_string()
    };
    let paragraphs = |body: &str| body.matches("<p num=").count();

    assert_eq!(paragraphs(&section("technical-field")), 1);
    assert_eq!(paragraphs(&section("background-art")), 4);
    assert_eq!(paragraphs(&section("tech-problem")), 2);
    assert_eq!(paragraphs(&section("tech-solution")), 10);
    assert_eq!(paragraphs(&section("advantageous-effects")), 3);
    assert_eq!(paragraphs(&section("reference-signs-list")), 1);

    let summary = section("summary-of-invention");
    for child in ["tech-problem", "tech-solution", "advantageous-effects"] {
        assert!(
            summary.contains(&format!("<{child}>")),
            "{child} 가 summary-of-invention 밖에 있다"
        );
    }
}

/// 실물 문서의 글은 저장소에 싣지 않는다 — SHA-256 앞 16자로 대조한다.
#[allow(dead_code)]
fn digest(text: &str) -> String {
    use sha2::Digest;
    format!("{:x}", sha2::Sha256::digest(text.as_bytes()))[..16].to_string()
}

/// 글 앞 `chars` 글자가 그 SHA-256(앞 16자)인가.
#[allow(dead_code)]
fn opens_with(text: &str, chars: usize, expected: &str) -> bool {
    digest(&text.chars().take(chars).collect::<String>()) == expected
}

/// 실물 본문 첫 문단(기술분야)과 둘째 문단(배경)의 첫머리 — 글 대신 지문.
#[allow(dead_code)]
const TECH_FIELD: &str = "4230137345539271";
#[allow(dead_code)]
const BACKGROUND: &str = "1a352eee416dc74b";
