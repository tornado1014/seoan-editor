//! 편집기 세션(P5) — 합성 문서로 도는 것. 실물 문서로 재는 세션 테스트는 저장소 밖에 둔다.

use hlt_core::EditorSession;

/// 새 의견서 정렬 단추: 의견서 틀은 양쪽 정렬 모양뿐이라 가운데 · 왼쪽이 양쪽으로 떨어졌다 → 정렬만 바꾼 모양을 덧붙인다.
#[test]
fn aligns_new_opinion_paragraphs_center_and_left() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("의견서.HLT");
    hlt_core::save_new_opinion_as_new(&path, hlt_core::WriteOptions::default()).unwrap();
    let mut session = EditorSession::open(&path).unwrap();
    let body = session.paragraphs()[1].id.clone();
    session
        .replace_text(&body, "가운데 문단\n왼쪽 문단")
        .unwrap();
    let paragraphs = session.paragraphs();
    session.set_alignment(&paragraphs[1].id, "center").unwrap();
    session.set_alignment(&paragraphs[2].id, "left").unwrap();
    let out = temp.path().join("정렬.HLT");
    session.save_as(&out).unwrap();
    let xml = hlt_core::export_kipo_xml_path(&out, &Default::default())
        .unwrap()
        .xml;
    assert!(xml.contains("align=\"CENTER\""), "{xml}");
    assert!(xml.contains("align=\"LEFT\""), "{xml}");
}

/// 의견서 .hlz 열기.
/// 새 의견서 → 글 · 서식 · 정렬 → hlz → 다시 열기 → 다시 hlz: 의견서 XML 이 같다(쪽 표시 · 쪽 수 포함).
#[test]
fn reopens_an_opinion_hlz_it_wrote() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("의견서.HLT");
    hlt_core::save_new_opinion_as_new(&path, hlt_core::WriteOptions::default()).unwrap();
    let mut session = EditorSession::open(&path).unwrap();
    let body = session.paragraphs()[1].id.clone();
    session
        .replace_text(
            &body,
            "Ⅰ. 통지내용\n본 출원인은 A & B 를 <받았>습니다.\n\n가운데 줄",
        )
        .unwrap();
    let ids: Vec<String> = session.paragraphs().iter().map(|p| p.id.clone()).collect();
    session
        .toggle_format(&ids[1], 0, 7, hlt_core::CharacterFormat::Bold)
        .unwrap();
    // 한 문단에 구간 둘 — 구간마다 따로 쓰면 마지막 것만 남았다.
    session
        .toggle_format_spans(
            &[(ids[2].clone(), 0, 2), (ids[2].clone(), 7, 12)],
            hlt_core::CharacterFormat::Underline,
        )
        .unwrap();
    session
        .set_paragraph_shapes(&[(ids[4].clone(), "center".into(), 0)])
        .unwrap();
    let written = temp.path().join("작성.HLT");
    session.save_as(&written).unwrap();
    let hlz_of = |path: &std::path::Path| {
        let export = hlt_core::export_fin_path(
            path,
            &Default::default(),
            &hlt_core::FinExportOptions {
                hlz_name: "의견서.hlz".into(),
                app_date: "2026-10-16".into(),
                app_time: "1".into(),
                image_time: String::new(),
                amendments: Vec::new(),
            },
        )
        .unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(export.hlz.clone())).unwrap();
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut archive.by_index(0).unwrap(), &mut xml).unwrap();
        (export.hlz, xml)
    };
    let (hlz, xml) = hlz_of(&written);
    assert!(xml.contains("<B>Ⅰ. 통지내용</B>"), "{xml}");
    assert_eq!(
        xml.matches("<U>").count(),
        2,
        "한 문단의 밑줄 구간 둘: {xml}"
    );
    let hlz_path = temp.path().join("의견서.hlz");
    std::fs::write(&hlz_path, hlz).unwrap();
    let reopened = EditorSession::open(&hlz_path).expect("의견서 hlz 를 연다");
    assert!(reopened.editable(), "의견서로 고칠 수 있다");
    let again = temp.path().join("다시.HLT");
    let mut reopened = reopened;
    reopened.save_as(&again).unwrap();
    let (_, xml_again) = hlz_of(&again);
    assert_eq!(xml_again, xml, "hlz → 열기 → hlz 왕복");
}

/// 수식·화학식 넣기: 【수학식 N】 표제와 그림 문단은 앞 본문 문단의 첫 줄 들여쓰기를 물려받지 않는다
/// (식별항목 넣기와 같이 표제 = 들여쓰기 0 · 왼쪽 정렬 · 그림 문단 = 들여쓰기 0).
#[test]
fn inserted_figure_heading_and_image_are_not_indented() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("의견서.HLT");
    hlt_core::save_new_opinion_as_new(&path, hlt_core::WriteOptions::default()).unwrap();
    let mut session = EditorSession::open(&path).unwrap();
    let body = session.paragraphs()[1].id.clone();
    let mut png = Vec::new();
    image::RgbImage::from_pixel(8, 4, image::Rgb([255, 255, 255]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    for kind in [hlt_core::FigureKind::Math, hlt_core::FigureKind::Chemistry] {
        session
            .insert_figure(&body, kind, png.clone(), 40.0, 20.0)
            .unwrap();
    }
    let out = temp.path().join("수식.HLT");
    session.save_as(&out).unwrap();
    let saved = EditorSession::open(&out).unwrap();
    let paragraphs = &saved.document.paragraphs;
    for label in ["【수학식 1】", "【화학식 1】"] {
        let at = paragraphs
            .iter()
            .position(|p| p.text.trim() == label)
            .expect(label);
        let (heading, image) = (&paragraphs[at], &paragraphs[at + 1]);
        assert_eq!(heading.indent_raw, 0, "{label} 표제 들여쓰기");
        assert_eq!(heading.align, "left", "{label} 표제 정렬");
        assert_eq!(image.indent_raw, 0, "{label} 그림 문단 들여쓰기");
    }
}

/// 그림 파일 넣기(JPG · TIFF): 표제 없이 그림 문단 하나 · 쪽보다 큰 그림은 비율대로 줄인다 · 그림이 아니면 거부.
#[test]
fn inserts_jpeg_and_tiff_pictures_without_heading() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("의견서.HLT");
    hlt_core::save_new_opinion_as_new(&path, hlt_core::WriteOptions::default()).unwrap();
    let mut session = EditorSession::open(&path).unwrap();
    let body = session.paragraphs()[1].id.clone();
    let before = session.paragraphs().len();
    let encode = |width, format| {
        let mut bytes = Vec::new();
        image::RgbImage::from_pixel(width, 300, image::Rgb([0, 0, 0]))
            .write_to(&mut std::io::Cursor::new(&mut bytes), format)
            .unwrap();
        bytes
    };
    // TIFF 가 먼저 들어가고 JPG 가 그 앞(같은 문단 뒤)으로 온다.
    session
        .insert_picture(&body, &encode(200, image::ImageFormat::Tiff))
        .unwrap();
    session
        .insert_picture(&body, &encode(1200, image::ImageFormat::Jpeg))
        .unwrap();
    assert!(session.insert_picture(&body, b"not a picture").is_err());
    let out = temp.path().join("그림.HLT");
    session.save_as(&out).unwrap();
    let saved = EditorSession::open(&out).unwrap().paragraphs();
    assert_eq!(saved.len(), before + 2, "표제 없이 그림 문단 둘");
    assert!(
        !saved
            .iter()
            .any(|p| p.text.contains('【') && p.text != "【의견내용】")
    );
    let (document, _) = hlt_core::hml_document_from_hlt(&out).unwrap();
    let sizes: Vec<_> = document
        .paragraphs
        .iter()
        .filter(|p| !p.image_ids.is_empty())
        .map(|p| {
            let id = p.image_ids.first().expect("그림 문단");
            let image = document.images.iter().find(|i| &i.id == id).unwrap();
            (image.secondary_width, image.secondary_height)
        })
        .collect();
    // 1200×300 화소 = 900×225pt → 가로 450pt 로 반. 200×300 = 150×225pt 그대로.
    assert_eq!(sizes, [(Some(450), Some(113)), (Some(150), Some(225))]);
}

/// 들여쓰기 · 들여쓰기 취소(통특기처럼 누를 때마다 더하지 않고 정해진 값으로 켜고 0 으로 끈다).
#[test]
fn sets_and_clears_first_line_indent() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("의견서.HLT");
    hlt_core::save_new_opinion_as_new(&path, hlt_core::WriteOptions::default()).unwrap();
    let mut session = EditorSession::open(&path).unwrap();
    let body = session.paragraphs()[1].id.clone();
    let indent_of = |session: &mut EditorSession, name: &str| {
        let out = temp.path().join(name);
        session.save_as(&out).unwrap();
        let saved = EditorSession::open(&out).unwrap();
        saved.document.paragraphs[1].indent_raw
    };
    session.set_indent(&body, 800).unwrap();
    assert_eq!(indent_of(&mut session, "켬.HLT"), 800);
    session.set_indent(&body, 800).unwrap();
    assert_eq!(
        indent_of(&mut session, "또켬.HLT"),
        800,
        "두 번 눌러도 더하지 않는다"
    );
    session.set_indent(&body, 0).unwrap();
    assert_eq!(indent_of(&mut session, "끔.HLT"), 0);
    let error = session.set_indent(&body, -1).unwrap_err();
    assert_eq!(
        error.details.get("reason").map(String::as_str),
        Some("indent")
    );
}

/// 식별항목 표제(절 닻)는 생성기가 들여쓰기 0 으로 굽는다 — 켜도 저장 때 조용히 사라지므로 켜기를 거절한다(끄기는 된다).
#[test]
fn refuses_to_indent_an_item_heading() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("의견서.HLT");
    hlt_core::save_new_opinion_as_new(&path, hlt_core::WriteOptions::default()).unwrap();
    let mut session = EditorSession::open(&path).unwrap();
    let heading = session
        .document
        .sections
        .iter()
        .find_map(|section| section.anchor_paragraph_id.clone())
        .expect("표제");
    let error = session.set_indent(&heading, 800).unwrap_err();
    assert_eq!(
        error.details.get("reason").map(String::as_str),
        Some("heading_indent")
    );
    session.set_indent(&heading, 0).unwrap();
}

/// 편집기에 붙여넣은 CP949 밖 글자: 고침 관문에서 바뀌어 저장되고 바꾼 목록이 세션에 남는다 · 대응 없는 글자는 고침 거부.
#[test]
fn pasted_characters_outside_cp949_are_substituted_before_saving() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("의견서.HLT");
    hlt_core::save_new_opinion_as_new(&path, hlt_core::WriteOptions::default()).unwrap();
    let mut session = EditorSession::open(&path).unwrap();
    let body = session.paragraphs()[1].id.clone();
    session
        .replace_text(&body, "a\u{2212}b\u{200B}c \u{2013} d")
        .unwrap();
    let replaced: Vec<(char, String, usize)> = session
        .cp949_substitutions
        .iter()
        .map(|item| (item.from, item.to.clone(), item.count))
        .collect();
    assert_eq!(
        replaced,
        [
            ('\u{200B}', String::new(), 1),
            ('\u{2013}', "-".to_string(), 1),
            ('\u{2212}', "-".to_string(), 1)
        ]
    );
    let out = temp.path().join("붙여넣기.HLT");
    session.save_as(&out).unwrap();
    let saved = EditorSession::open(&out).unwrap().paragraphs();
    assert!(saved.iter().any(|p| p.text == "a-bc - d"), "{saved:?}");

    let error = session.replace_text(&body, "\u{1F600}").unwrap_err();
    assert_eq!(error.code, hlt_core::ErrorCode::Cp949Invalid);
}

/// 그림 크기 바꾸기(비율 잠금): 고른 그림만 배율대로 · 비율 그대로 · 쪽보다 크게는 쪽에서 멈춘다 · 없는 그림은 오류.
#[test]
fn resizing_an_image_scales_only_that_image_keeping_its_ratio() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("의견서.HLT");
    hlt_core::save_new_opinion_as_new(&path, hlt_core::WriteOptions::default()).unwrap();
    let mut session = EditorSession::open(&path).unwrap();
    let body = session.paragraphs()[1].id.clone();
    let encode = |width: u32, height: u32| {
        let mut bytes = Vec::new();
        image::RgbImage::from_pixel(width, height, image::Rgb([0, 0, 0]))
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        bytes
    };
    session.insert_picture(&body, &encode(200, 100)).unwrap();
    session.insert_picture(&body, &encode(120, 160)).unwrap();
    let sizes = |session: &EditorSession| -> Vec<(String, u32, u32)> {
        session
            .document
            .images
            .iter()
            .map(|image| (image.id.clone(), image.display_width, image.display_height))
            .collect()
    };
    let before = sizes(&session);
    assert_eq!(before.len(), 2);
    session.resize_image(&before[1].0, 0.5).unwrap();
    let out = temp.path().join("줄임.HLT");
    session.save_as(&out).unwrap();
    let after = sizes(&EditorSession::open(&out).unwrap());
    assert_eq!(
        (after[0].1, after[0].2),
        (before[0].1, before[0].2),
        "다른 그림은 그대로"
    );
    let (w, h) = (f64::from(after[1].1), f64::from(after[1].2));
    assert!(
        (w - f64::from(before[1].1) / 2.0).abs() <= 1.0,
        "{before:?} → {after:?}"
    );
    let ratio = f64::from(before[1].1) / f64::from(before[1].2);
    assert!(
        (w / h - ratio).abs() < 0.05,
        "비율 그대로 {before:?} → {after:?}"
    );

    // 아주 크게 → 쪽 본문에서 멈춘다(표시 단위 ≈ 1pt 대 · 쪽 가로 174mm ≈ 493pt 를 넘지 않는다).
    let mut session = EditorSession::open(&out).unwrap();
    let id = sizes(&session)[0].0.clone();
    session.resize_image(&id, 100.0).unwrap();
    let grown = sizes(&session)[0].clone();
    assert!(
        grown.1 > before[0].1 && f64::from(grown.1) < 520.0,
        "{grown:?}"
    );
    assert!(session.resize_image("image:없음", 2.0).is_err());
}

/// 표 칸 안 그림 넣기: 커서 자리(칸 글의 그림 = `_`)에 들어가고, 칸 폭보다 크면 줄이며, 쪽 차례(앞 칸 그림이 앞)를 지킨다.
/// 저장 · 다시 열어도 그림 수가 같고(겹쳐 읽히지 않는다), 넣은 그림 크기를 바꾸면 그 그림만 바뀐다.
#[test]
fn a_picture_goes_into_a_table_cell_at_the_caret() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("의견서.HLT");
    hlt_core::save_new_opinion_as_new(&path, hlt_core::WriteOptions::default()).unwrap();
    let mut session = EditorSession::open(&path).unwrap();
    let body = session.paragraphs()[1].id.clone();
    let encode = |width: u32, height: u32| {
        let mut bytes = Vec::new();
        image::RgbImage::from_pixel(width, height, image::Rgb([0, 0, 0]))
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        bytes
    };
    session.insert_table(&body, 1, 3).unwrap();
    let table_cells = |session: &EditorSession| -> Vec<(String, f64)> {
        let table = session.document.tables[0].id.clone();
        session
            .document
            .cells
            .iter()
            .filter(|cell| cell.table_id == table)
            .map(|cell| (cell.id.clone(), f64::from(cell.width) / 20.0))
            .collect()
    };
    let cells = table_cells(&session);
    session.replace_cell_text(&cells[0].0, "가나").unwrap();
    // 둘째 칸에 작은 그림을 먼저, 그 뒤 첫째 칸 글 끝에 칸보다 넓은 그림.
    session
        .insert_picture_in_cell(&cells[1].0, 0, &encode(40, 20))
        .unwrap();
    let cells = table_cells(&session);
    session
        .insert_picture_in_cell(&cells[0].0, 2, &encode(2000, 500))
        .unwrap();
    let cells = table_cells(&session);
    assert_eq!(session.cell_text(&cells[0].0).as_deref(), Some("가나_"));
    assert_eq!(session.cell_text(&cells[1].0).as_deref(), Some("_"));
    let images = session.document.images.clone();
    assert_eq!(
        images
            .iter()
            .map(|image| image.cell_id.clone())
            .collect::<Vec<_>>(),
        [Some(cells[0].0.clone()), Some(cells[1].0.clone())],
        "쪽 차례 = 첫째 칸 그림이 앞"
    );
    assert!(
        f64::from(images[0].display_width) <= cells[0].1,
        "{:?} · 칸 {}pt",
        images[0],
        cells[0].1
    );
    assert_eq!(
        (images[1].display_width, images[1].display_height),
        (30, 15)
    );

    // 다시 열기 전(모델 차례 = 넣은 차례)에도 크기 바꾸기가 고른 그림을 맞힌다.
    session.resize_image(&images[0].id, 0.5).unwrap();
    let live = session.document.images.clone();
    assert!(
        live[0].display_width < images[0].display_width
            && live[1].display_width == images[1].display_width,
        "{images:?} → {live:?}"
    );

    let out = temp.path().join("칸 그림.HLT");
    session.save_as(&out).unwrap();
    let mut session = EditorSession::open(&out).unwrap();
    assert_eq!(
        session.document.images.len(),
        2,
        "다시 열어도 겹치지 않는다"
    );
    let small = session.document.images[1].clone();
    let big = session.document.images[0].clone();
    session.resize_image(&big.id, 0.5).unwrap();
    let after = session.document.images.clone();
    assert_eq!(after.len(), 2);
    assert_eq!(
        (after[1].display_width, after[1].display_height),
        (small.display_width, small.display_height),
        "다른 그림은 그대로"
    );
    assert!(
        (f64::from(after[0].display_width) - f64::from(big.display_width) / 2.0).abs() <= 1.0,
        "{big:?} → {:?}",
        after[0]
    );
}

/// 도면 보정: 최종본(.fin)을 열어 도 2 그림을 바꾸고 청구항 2 를 두 줄로 나누면 보정서에 실물(통특기 도면 보정)과 같은 꼴로 실린다 —
/// 청구항 뒤 `<AmendBody elementName="figure" … attributeValue="2">\n<figure num="2"><img … file="pat00003.jpg" …/></figure>\n</AmendBody>`
/// (그림 이름 = 최종본 그림 다음 번호 · 그림은 dta 안). 크기만 바꾼 도면도 `figure A`(이름 그대로) · 손대지 않으면 「변경 없음」.
#[test]
fn a_replaced_or_resized_drawing_rides_in_the_amendment() {
    use std::io::Read;
    let temp = tempfile::tempdir().unwrap();
    let fin = synthetic_fin(temp.path());
    let amend = |session: &mut EditorSession| {
        let after = session.materialize().unwrap();
        hlt_core::export_amendment_from_fin(
            &fin,
            &after,
            "spec_1.dta",
            "2026-10-22",
            &Default::default(),
            &Default::default(),
        )
        .map_err(|error| error.details["reason"].clone())
    };

    // 그림 바꾸기 + 청구항 줄 나누기.
    let mut session = EditorSession::open(&fin).unwrap();
    let figure_two = session.document.images[1].id.clone();
    let mut ring = image::RgbImage::from_pixel(400, 200, image::Rgb([255, 255, 255]));
    for x in 100..300 {
        ring.put_pixel(x, 100, image::Rgb([0, 0, 0]));
    }
    let mut bytes = Vec::new();
    ring.write_to(
        &mut std::io::Cursor::new(&mut bytes),
        image::ImageFormat::Png,
    )
    .unwrap();
    let width = session.document.images[1].display_width;
    session.replace_picture(&figure_two, &bytes).unwrap();
    let shown = &session.document.images[1];
    assert_eq!(shown.display_width, width, "폭은 그대로");
    assert!(
        (shown.box_height * 2).abs_diff(shown.box_width) <= 2,
        "상자도 새 그림 비율: {} × {}",
        shown.box_width,
        shown.box_height
    );
    assert!(
        (shown.display_height * 2).abs_diff(width) <= 2,
        "높이는 새 그림 비율(2:1): {} × {}",
        shown.display_width,
        shown.display_height
    );
    let claim = session
        .paragraphs()
        .into_iter()
        .find(|p| p.text.starts_with("제1항에 있어서"))
        .unwrap()
        .id;
    session
        .replace_text(&claim, "제1항에 있어서,\n덮개를 더 포함하는 장치.")
        .unwrap();
    let export = amend(&mut session).unwrap();
    let tail = export.xml.split_once("<Amendment>\n").unwrap().1;
    assert!(
        tail.starts_with(
            "<AmendBody elementName=\"claim\" status=\"A\" attributeName=\"num\" attributeValue=\"2\">\n\
             <claim num=\"2\"><claim-text>제1항에 있어서,<br/>덮개를 더 포함하는 장치.</claim-text></claim></AmendBody>\n\
             <AmendBody elementName=\"figure\" status=\"A\" attributeName=\"num\" attributeValue=\"2\">\n\
             <figure num=\"2\"><img id=\"i0002\" he=\"40\" wi=\"80\" file=\"pat00003.jpg\" img-format=\"jpg\"/></figure>\n\
             </AmendBody>\n</Amendment>"
        ),
        "{tail}"
    );
    let mut dta = zip::ZipArchive::new(std::io::Cursor::new(export.dta.clone())).unwrap();
    let mut picture = Vec::new();
    dta.by_name("pat00003.jpg")
        .unwrap()
        .read_to_end(&mut picture)
        .unwrap();
    let picture = image::load_from_memory(&picture).unwrap();
    assert_eq!(
        (picture.width(), picture.height()),
        (400, 200),
        "새 그림이 dta 에"
    );

    // 크기만 바꾸기 → 그 도면만 `figure A` · 이름 그대로.
    let mut session = EditorSession::open(&fin).unwrap();
    let figure_one = session.document.images[0].id.clone();
    session.resize_image(&figure_one, 1.5).unwrap();
    let export = amend(&mut session).unwrap();
    let kinds: Vec<&str> = export
        .xml
        .split("<AmendBody elementName=\"")
        .skip(1)
        .collect();
    assert_eq!(kinds.len(), 1, "{}", export.xml);
    assert!(kinds[0].starts_with("figure\""), "{}", export.xml);
    assert!(kinds[0].contains("file=\"pat00001.jpg\""), "{}", export.xml);

    // 손대지 않으면 「변경된 내용이 없습니다」.
    let mut session = EditorSession::open(&fin).unwrap();
    let body = session.paragraphs()[4].id.clone();
    session.set_alignment(&body, "left").unwrap();
    session.set_alignment(&body, "justified").unwrap();
    assert_eq!(amend(&mut session).unwrap_err(), "amendment_empty");
}

/// 표제(【도 3b】 · 【수학식 1】) 바로 앞 빈 줄 지우기: 빈 줄 끝에서 Delete(= 표제를 빈 줄에 붙이기)도,
/// 앞 문단 끝부터 표제 앞까지 고른 지우기도 빈 줄만 없앤다 — 「표제 문단이 걸려 있어」 거절이 아니다.
#[test]
fn deletes_an_empty_line_before_a_heading() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("의견서.HLT");
    hlt_core::save_new_opinion_as_new(&path, hlt_core::WriteOptions::default()).unwrap();
    let mut png = Vec::new();
    image::RgbImage::from_pixel(8, 4, image::Rgb([255, 255, 255]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let prepared = |session: &mut EditorSession| -> Vec<String> {
        let body = session.paragraphs()[1].id.clone();
        session.replace_text(&body, "본문\n").unwrap();
        let empty = session.paragraphs()[2].id.clone();
        session
            .insert_figure(&empty, hlt_core::FigureKind::Math, png.clone(), 40.0, 20.0)
            .unwrap();
        let texts: Vec<String> = session
            .paragraphs()
            .iter()
            .map(|p| p.text.clone())
            .collect();
        assert_eq!(texts[1..4], ["본문", "", "【수학식 1】"], "{texts:?}");
        session.paragraphs()[1..4]
            .iter()
            .map(|p| p.id.clone())
            .collect()
    };
    let after = |session: &EditorSession| -> Vec<String> {
        session.paragraphs()[1..3]
            .iter()
            .map(|p| p.text.clone())
            .collect()
    };

    let mut session = EditorSession::open(&path).unwrap();
    let ids = prepared(&mut session);
    session.merge_with_previous(&ids[2]).unwrap();
    assert_eq!(after(&session), ["본문", "【수학식 1】"], "Delete");

    let mut session = EditorSession::open(&path).unwrap();
    let ids = prepared(&mut session);
    session.delete_range(&ids[0], 2, &ids[2], 0).unwrap();
    assert_eq!(after(&session), ["본문", "【수학식 1】"], "고른 지우기");

    let mut session = EditorSession::open(&path).unwrap();
    let ids = prepared(&mut session);
    session.delete_range(&ids[1], 0, &ids[2], 0).unwrap();
    assert_eq!(
        after(&session),
        ["본문", "【수학식 1】"],
        "빈 줄 맨 앞부터 고른 지우기"
    );

    // 표제 바로 뒤 빈 줄에서 Backspace: 빈 줄만 지운다.
    let mut session = EditorSession::open(&path).unwrap();
    prepared(&mut session);
    let image = session.paragraphs()[4].id.clone();
    session.replace_text(&image, "\n_").unwrap();
    let texts: Vec<String> = session.paragraphs()[3..6]
        .iter()
        .map(|p| p.text.clone())
        .collect();
    assert_eq!(texts, ["【수학식 1】", "", "_"], "표제 뒤 빈 줄");
    let empty = session.paragraphs()[4].id.clone();
    session.merge_with_previous(&empty).unwrap();
    let texts: Vec<String> = session.paragraphs()[3..5]
        .iter()
        .map(|p| p.text.clone())
        .collect();
    assert_eq!(texts, ["【수학식 1】", "_"], "Backspace");

    // 글이 있는 문단에 표제를 붙이는 것은 여전히 거절한다.
    let mut session = EditorSession::open(&path).unwrap();
    let ids = prepared(&mut session);
    session.merge_with_previous(&ids[1]).unwrap();
    let heading = session.paragraphs()[2].id.clone();
    let error = session.merge_with_previous(&heading).unwrap_err();
    assert_eq!(error.details["reason"], "heading_paragraph");
}

/// 합성 최종본: 도면 2장 · 청구항 2 · 문단 2 짜리 명세서를 굽고 제출파일(.fin)로.
fn synthetic_fin(dir: &std::path::Path) -> std::path::PathBuf {
    synthetic_fin_with(dir, SYNTHETIC_SPEC)
}

const SYNTHETIC_SPEC: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<KIPO><PatentCAFDOC>
<description><invention-title>시험 장치{TEST APPARATUS}</invention-title>
<technical-field><p num="0001">본 발명은 시험 장치에 관한 것이다.</p></technical-field>
</description>
<claims>
<claim num="1"><claim-text>몸체를 포함하는 장치.</claim-text></claim>
<claim num="2"><claim-text>제1항에 있어서, 덮개를 더 포함하는 장치.</claim-text></claim>
</claims>
<abstract><summary><p num="0001a">시험 장치.</p></summary><abstract-figure><figref num="1"/></abstract-figure></abstract>
<drawings>
<figure num="1"><img id="i0001" he="60" wi="80" file="pat00001.jpg" img-format="jpg"/></figure>
<figure num="2"><img id="i0002" he="60" wi="80" file="pat00002.jpg" img-format="jpg"/></figure>
</drawings>
</PatentCAFDOC></KIPO>"#;

/// `xml` 명세서(그림 pat00001 · pat00002)를 굽고 제출파일(.fin)로.
fn synthetic_fin_with(dir: &std::path::Path, spec: &str) -> std::path::PathBuf {
    use std::io::Write;
    // 통특기 최종본처럼 hlz 에 명세서 XML 을 글자 그대로 싣는다 — 서안으로 구워 낸 fin 이면 서안이 못 읽는 모양이
    // 이미 깎여 있어 왕복 손실이 안 보인다.
    let jpeg = |shift: u32| {
        let mut image = image::RgbImage::from_pixel(400, 300, image::Rgb([255, 255, 255]));
        for x in 50 + shift..200 + shift {
            for y in 50..200 {
                image.put_pixel(x, y, image::Rgb([0, 0, 0]));
            }
        }
        let mut bytes = Vec::new();
        image
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Jpeg,
            )
            .unwrap();
        bytes
    };
    let mut hlz = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut hlz));
        let options = zip::write::SimpleFileOptions::default();
        for (name, bytes) in [
            ("spec.xml", spec.as_bytes().to_vec()),
            ("pat00001.jpg", jpeg(0)),
            ("pat00002.jpg", jpeg(80)),
        ] {
            zip.start_file(name, options).unwrap();
            zip.write_all(&bytes).unwrap();
        }
        zip.finish().unwrap();
    }
    let fin = dir.join("spec.fin");
    let bytes = hlt_core::build_fin(
        &hlz,
        &hlt_core::FinExportOptions {
            hlz_name: "spec.hlz".into(),
            app_date: "2026-10-22".into(),
            app_time: "1761100000".into(),
            image_time: String::new(),
            amendments: Vec::new(),
        },
    )
    .unwrap();
    std::fs::write(&fin, bytes).unwrap();
    fin
}

/// 차례 보정: 1차 보정서가 든 최종본(.fin)을 열면 1차가 적용된 글이고, 거기서 만든 보정서는 2차다 —
/// 새로 고친 것만 싣고(1차 항목은 다시 싣지 않는다) `dtaVersion="003"` · `xresult` 에 `AMDCNT=2` · `AMD002`.
/// 문단 번호는 편집 세션이 든 식별번호로 매긴다(서안이 구운 HLT 에는 식별번호가 없다) — 문단을 나눠도 거절하지 않는다.
#[test]
fn a_second_amendment_builds_on_the_first() {
    use std::io::Read;
    let temp = tempfile::tempdir().unwrap();
    let fin = synthetic_fin(temp.path());
    let amend = |fin: &std::path::Path, session: &mut EditorSession, dta: &str| {
        let after = session.materialize().unwrap();
        let kipo = hlt_core::KipoExportOptions {
            paragraph_numbers: session.paragraph_numbers(),
            ..Default::default()
        };
        hlt_core::export_amendment_from_fin(
            fin,
            &after,
            dta,
            "2026-10-22",
            &kipo,
            &hlt_core::AmendmentOptions {
                dta_version: String::new(), // 차수로 매긴다(GUI · CLI 와 같다)
                ..Default::default()
            },
        )
        .map_err(|error| error.details["reason"].clone())
    };
    let find = |session: &EditorSession, head: &str| {
        session
            .paragraphs()
            .into_iter()
            .find(|p| p.text.starts_with(head))
            .unwrap_or_else(|| panic!("{head}"))
            .id
    };
    let xresult = |fin: &[u8]| {
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(fin.to_vec())).unwrap();
        let mut raw = Vec::new();
        archive
            .by_name("xresult.inf")
            .unwrap()
            .read_to_end(&mut raw)
            .unwrap();
        encoding_rs::EUC_KR.decode(&raw).0.into_owned()
    };

    // 1차: 청구항 1.
    let mut session = EditorSession::open(&fin).unwrap();
    let claim = find(&session, "몸체를");
    session
        .replace_text(&claim, "몸체와 바닥을 포함하는 장치.")
        .unwrap();
    let first = amend(&fin, &mut session, "spec_1차.dta").unwrap();
    assert!(first.xml.contains("dtaVersion=\"002\""), "{}", first.xml);
    let first_fin = temp.path().join("spec_1차.fin");
    std::fs::write(&first_fin, &first.fin).unwrap();

    // 2차: 1차가 든 최종본을 연다 → 1차 글이 보인다 · 청구항 2 와 문단 0001 을 고치고, 문단 0001 은 둘로 나눈다.
    let mut session = EditorSession::open(&first_fin).unwrap();
    find(&session, "몸체와 바닥을");
    let claim = find(&session, "제1항에 있어서");
    session
        .replace_text(&claim, "제1항에 있어서, 뚜껑을 더 포함하는 장치.")
        .unwrap();
    let body = find(&session, "본 발명은");
    session
        .replace_text(
            &body,
            "본 발명은 시험 장치에 관한 것이다.\n특히 덮개에 관한 것이다.",
        )
        .unwrap();
    let second = amend(&first_fin, &mut session, "spec_2차.dta").unwrap();
    assert!(second.xml.contains("dtaVersion=\"003\""), "{}", second.xml);
    let items: Vec<&str> = second.xml.split("<AmendBody ").skip(1).collect();
    assert_eq!(
        items.len(),
        2,
        "1차 항목은 다시 싣지 않는다: {}",
        second.xml
    );
    assert!(
        items[0].starts_with("elementName=\"p\" status=\"A\" attributeName=\"num\" attributeValue=\"0001\">\n<p num=\"0001\">본 발명은 시험 장치에 관한 것이다.<br/>특히 덮개에 관한 것이다.</p>"),
        "나눈 줄은 앞 번호 문단에 <br/>: {}",
        items[0]
    );
    assert!(
        items[1].contains("attributeValue=\"2\"") && items[1].contains("뚜껑을"),
        "{}",
        items[1]
    );
    let index = xresult(&second.fin);
    assert!(index.contains("AMDCNT=2"), "{index}");
    assert!(
        index.contains("AMD001=spec_1차.dta,") && index.contains("AMD002=spec_2차.dta,"),
        "{index}"
    );

    // 2차까지 든 최종본: 둘 다 적용된 글 · 손대지 않으면 「변경 없음」.
    let second_fin = temp.path().join("spec_2차.fin");
    std::fs::write(&second_fin, &second.fin).unwrap();
    let mut session = EditorSession::open(&second_fin).unwrap();
    find(&session, "몸체와 바닥을");
    find(&session, "제1항에 있어서, 뚜껑을");
    let body = find(&session, "본 발명은");
    session.set_alignment(&body, "left").unwrap();
    session.set_alignment(&body, "justified").unwrap();
    assert_eq!(
        amend(&second_fin, &mut session, "spec_3차.dta").unwrap_err(),
        "amendment_empty"
    );
}

/// 손대지 않은 최종본은 보정이 아니다 — 통특기 최종본 XML 의 모양(실물 232건에서 왕복이 깨지던 것)을 열고 다시 내도 같다:
/// 빈 번호 문단 · 탭만 든 문단 · 끝에 빈 줄이 있는 청구항 · 명칭 끝 공백 · 요약 끝 빈 줄 · 쪼개진 밑줄 구간.
#[test]
fn an_untouched_final_version_is_not_an_amendment() {
    let temp = tempfile::tempdir().unwrap();
    let spec = SYNTHETIC_SPEC
        .replace("시험 장치{TEST APPARATUS}", "시험 장치{TEST APPARATUS} ")
        .replace(
            "<p num=\"0001\">본 발명은 시험 장치에 관한 것이다.</p>",
            "<p num=\"0001\">본 발명은 <u>시험 </u><u>장치에</u> 관한 것이다.</p>\n<p num=\"0002\">\t</p>\n<p num=\"0003\"></p>",
        )
        .replace("<p num=\"0001a\">시험 장치.</p>", "<p num=\"0001a\">시험 장치.<br/> </p>")
        .replace(
            "</technical-field>",
            "</technical-field>\n<citation-list>\n<patent-literature>\n<p num=\"0004\"><patcit num=\"0001\"><text>KR 10-2099-0000001 A</text></patcit>\n<patcit num=\"0002\" dnum=\"KR10-0000002\" dnum-type=\"publication number\"><document-id><country>KR</country><doc-number>10-0000002</doc-number><kind>B</kind><name></name><date></date></document-id></patcit>\n</p>\n</patent-literature>\n<non-patent-literature>\n<p num=\"0005\"><nplcit num=\"0001\"><text> 홍길동, 시험 서적, 2001</text></nplcit>\n<nplcit num=\"0002\"><text> 김철수, 시험 학회지, 2002</text></nplcit>\n</p>\n</non-patent-literature></citation-list>",
        )
        .replace(
            "몸체를 포함하는 장치.</claim-text>",
            "몸체를 포함하는 장치.<br/></claim-text>",
        )
        .replace(
            "덮개를 더 포함하는 장치.</claim-text>",
            "덮개를 더 포함하는 장치. <br/><br/><br/></claim-text>",
        );
    let fin = synthetic_fin_with(temp.path(), &spec);
    // 선행기술문헌 줄 = 통특기 「최종본 불러오기」 줄(실측): 서지 꼴은 괄호 뒤 공백 둘 · 글 앞 공백은 지운다.
    let lines: Vec<String> = EditorSession::open(&fin)
        .unwrap()
        .paragraphs()
        .into_iter()
        .map(|p| p.text)
        .skip_while(|text| text != "【선행기술문헌】")
        .take(7)
        .collect();
    assert_eq!(
        lines,
        [
            "【선행기술문헌】",
            "【특허문헌】",
            "(특허문헌 1) KR 10-2099-0000001 A",
            "(특허문헌 2)  KR10-0000002 B",
            "【비특허문헌】",
            "(비특허문헌 1) 홍길동, 시험 서적, 2001",
            "(비특허문헌 2) 김철수, 시험 학회지, 2002",
        ]
    );
    let mut session = EditorSession::open(&fin).unwrap();
    let body = session
        .paragraphs()
        .into_iter()
        .find(|p| p.text.starts_with("본 발명은"))
        .unwrap()
        .id;
    session.set_alignment(&body, "left").unwrap();
    session.set_alignment(&body, "justified").unwrap();
    let after = session.materialize().unwrap();
    let kipo = hlt_core::KipoExportOptions {
        paragraph_numbers: session.paragraph_numbers(),
        ..Default::default()
    };
    let result = hlt_core::export_amendment_from_fin(
        &fin,
        &after,
        "spec_1.dta",
        "2026-10-22",
        &kipo,
        &Default::default(),
    );
    match result {
        Err(error) => assert_eq!(error.details["reason"], "amendment_empty"),
        Ok(export) => panic!("손대지 않았는데 보정 항목: {}", export.xml),
    }
}

/// 앞선 보정서 dta 안에서 XML 이 첫 항목이 아니어도(옛 통특기 도면 보정 = 그림들 뒤에 XML) 최종본을 읽는다.
#[test]
fn reads_a_previous_amendment_whose_xml_is_not_first() {
    use std::io::{Read, Write};
    let temp = tempfile::tempdir().unwrap();
    let fin = synthetic_fin(temp.path());
    let mut session = EditorSession::open(&fin).unwrap();
    let figure = session.document.images[1].id.clone();
    let mut png = Vec::new();
    image::RgbImage::from_pixel(400, 200, image::Rgb([0, 0, 0]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    session.replace_picture(&figure, &png).unwrap();
    let after = session.materialize().unwrap();
    let options = hlt_core::AmendmentOptions {
        dta_version: String::new(),
        ..Default::default()
    };
    let export = hlt_core::export_amendment_from_fin(
        &fin,
        &after,
        "spec_1.dta",
        "2026-10-22",
        &Default::default(),
        &options,
    )
    .unwrap();
    // dta 를 그림 먼저 · XML 나중으로 다시 묶고 최종본에 넣는다.
    let mut entries = Vec::new();
    let mut dta = zip::ZipArchive::new(std::io::Cursor::new(export.dta.clone())).unwrap();
    for index in 0..dta.len() {
        let mut entry = dta.by_index(index).unwrap();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        entries.push((entry.name().to_string(), bytes));
    }
    assert!(
        entries[0].0.ends_with(".xml") && entries.len() == 2,
        "{:?}",
        entries.iter().map(|e| &e.0).collect::<Vec<_>>()
    );
    entries.rotate_left(1);
    let mut reordered = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut reordered));
        for (name, bytes) in &entries {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
    }
    let mut hlz = Vec::new();
    let mut original = zip::ZipArchive::new(std::fs::File::open(&fin).unwrap()).unwrap();
    original
        .by_name("spec.hlz")
        .unwrap()
        .read_to_end(&mut hlz)
        .unwrap();
    let amended = temp.path().join("spec_1차.fin");
    let bytes = hlt_core::build_fin(
        &hlz,
        &hlt_core::FinExportOptions {
            hlz_name: "spec.hlz".into(),
            app_date: "2026-10-22".into(),
            app_time: "1761100000".into(),
            image_time: String::new(),
            amendments: vec![hlt_core::FinAmendment {
                file_name: "spec_1.dta".into(),
                date: "2026-10-22".into(),
                bytes: reordered,
            }],
        },
    )
    .unwrap();
    std::fs::write(&amended, bytes).unwrap();

    // 열면 바꾼 도 2 · 손대지 않고 보정서를 만들면 「변경 없음」(앞 보정서의 그림이 최종본 그림이다).
    let mut session = EditorSession::open(&amended).unwrap();
    let shown = &session.document.images[1];
    assert!(
        (shown.display_height * 2).abs_diff(shown.display_width) <= 2,
        "바꾼 도 2(2:1): {} × {}",
        shown.display_width,
        shown.display_height
    );
    let after = session.materialize().unwrap();
    let error = hlt_core::export_amendment_from_fin(
        &amended,
        &after,
        "spec_2.dta",
        "2026-10-22",
        &Default::default(),
        &options,
    )
    .unwrap_err();
    assert_eq!(error.details["reason"], "amendment_empty");
}

/// 식별번호는 HLT 에 실린다: 최종본(.fin)을 열어 HLT 로 저장하면 보정 명세서(종류 111)이고 문단마다 식별번호가 있다 —
/// 그 HLT 를 새로 열어(세션에 번호 기억 없음) 문단을 나눠도 보정서가 번호대로 나온다(나눈 줄은 앞 번호 문단에 `<br/>`).
/// 예전에는 HLT 에 번호를 안 써서 다시 열면 번호가 사라지고 「식별번호가 없다」로 거절됐다.
#[test]
fn identification_numbers_survive_saving_the_opened_final_version() {
    let temp = tempfile::tempdir().unwrap();
    let fin = synthetic_fin(temp.path());
    let saved = temp.path().join("보정용.HLT");
    EditorSession::open(&fin).unwrap().save_as(&saved).unwrap();
    let read = hlt_core::inspect_path(&saved, Default::default()).unwrap();
    assert_eq!(read.summary.document_type_raw, 0x8000 + 111, "보정 명세서");
    let numbered: Vec<(String, Option<u32>)> = read
        .paragraphs
        .iter()
        .filter(|p| p.wipo_id.is_some())
        .map(|p| (p.text.chars().take(6).collect(), p.wipo_id))
        .collect();
    assert_eq!(
        numbered,
        [("본 발명은 ".to_string(), Some(1))],
        "식별번호 0001"
    );

    let mut session = EditorSession::open(&saved).unwrap();
    assert!(session.editable(), "보정 명세서도 고친다");
    let body = session
        .paragraphs()
        .into_iter()
        .find(|p| p.text.starts_with("본 발명은"))
        .unwrap();
    assert_eq!(
        body.number.as_deref(),
        Some("0001"),
        "다시 연 HLT 에서 번호를 읽는다"
    );
    session
        .replace_text(
            &body.id,
            "본 발명은 시험 장치에 관한 것이다.\n특히 덮개에 관한 것이다.",
        )
        .unwrap();
    let after = session.materialize().unwrap();
    let export = hlt_core::export_amendment_from_fin(
        &fin,
        &after,
        "spec_1.dta",
        "2026-10-22",
        &Default::default(), // 세션 번호를 넘기지 않는다 — HLT 의 식별번호만으로
        &hlt_core::AmendmentOptions {
            dta_version: String::new(),
            ..Default::default()
        },
    )
    .unwrap();
    let items: Vec<&str> = export.xml.split("<AmendBody ").skip(1).collect();
    assert_eq!(items.len(), 1, "{}", export.xml);
    assert!(
        items[0].contains(
            "<p num=\"0001\">본 발명은 시험 장치에 관한 것이다.<br/>특히 덮개에 관한 것이다.</p>"
        ),
        "{}",
        export.xml
    );
}

/// 보정 명세서의 식 번호: 통특기처럼 【수학식 N】 표제가 식별번호를 들고 식 그림 줄은 0 이다(통특기 「최종본 불러오기」 실물과 줄마다 같은 규칙).
#[test]
fn a_formula_heading_carries_the_identification_number() {
    let temp = tempfile::tempdir().unwrap();
    let spec = SYNTHETIC_SPEC.replace(
        "<p num=\"0001\">본 발명은 시험 장치에 관한 것이다.</p>",
        "<p num=\"0001\">본 발명은 시험 장치에 관한 것이다.</p>\n<p num=\"0002\"><maths num=\"1\"><img id=\"i0003\" he=\"10\" wi=\"40\" file=\"pat00001.jpg\" img-format=\"jpg\"/></maths></p>\n<p num=\"0003\">여기서 식은 예시다.</p>",
    );
    let fin = synthetic_fin_with(temp.path(), &spec);
    let saved = temp.path().join("보정용.HLT");
    EditorSession::open(&fin).unwrap().save_as(&saved).unwrap();
    let read = hlt_core::inspect_path(&saved, Default::default()).unwrap();
    let ids: Vec<(String, Option<u32>)> = read
        .paragraphs
        .iter()
        .skip_while(|p| !p.text.starts_with("본 발명은"))
        .take(4)
        .map(|p| {
            (
                p.text.chars().take(7).collect(),
                p.wipo_id.filter(|id| *id != 0),
            )
        })
        .collect();
    assert_eq!(
        ids,
        [
            ("본 발명은 시".to_string(), Some(1)),
            ("【수학식 1】".to_string(), Some(2)),
            ("_".to_string(), None),
            ("여기서 식은 ".to_string(), Some(3)),
        ]
    );
}

/// 검토보고서 보정안 걸기: 보고서(docx)의 「보정안 1」 표(추적 변경) → 청구항 바꾸기 · 지우기 · 새 항, 고른 오기 정정 →
/// 보정서에 `claim A` · `claim D` · `claim I` · `p A`. 보정 전 문언이 최종본과 다른 항은 건너뛰고 알린다 · 되돌리기 한 번.
#[test]
fn applies_a_review_report_proposal() {
    use std::io::Write;
    let temp = tempfile::tempdir().unwrap();
    let fin = synthetic_fin(temp.path());
    let p = |runs: String| format!("<w:p>{runs}</w:p>");
    let r = |text: &str| format!("<w:r><w:t xml:space=\"preserve\">{text}</w:t></w:r>");
    let ins = |text: &str| format!("<w:ins><w:r><w:t>{text}</w:t></w:r></w:ins>");
    let del = |text: &str| format!("<w:del><w:r><w:delText>{text}</w:delText></w:r></w:del>");
    let table = |label: &str, body: String| {
        format!(
            "<w:tbl><w:tr><w:tc>{}</w:tc><w:tc>{body}</w:tc></w:tr></w:tbl>",
            p(r(label))
        )
    };
    let first = [
        p(r("【청구항 1】")),
        p([
            r("몸체"),
            del("를"),
            ins("와 바닥을"),
            r(" 포함하는 장치"),
            del("."),
            ins(","),
        ]
        .concat()),
        p(ins("덮개를 더 포함하는 장치.")),
        p(del("【청구항 2】")),
        p(del("제1항에 있어서, 덮개를 더 포함하는 장치.")),
        p(ins("【청구항 3】")),
        p(ins("제1항에 있어서, 손잡이를 더 포함하는 장치.")),
    ]
    .concat();
    // 보정 전 문언이 최종본과 다르다(보고서를 다른 판에서 썼다).
    let second = [
        p(r("【청구항 1】")),
        p(format!("{}{}", del("다른 글."), ins("새 글."))),
    ]
    .concat();
    let xml = format!(
        "<w:document xmlns:w=\"w\"><w:body>{}{}{}{}</w:body></w:document>",
        p(r("4. 보정안")),
        table("보정안 1", first),
        table("보정안 2", second),
        p(r(
            "5. 기타: 단락 [0001]의 “시험 장치”는 “검사 장치”의 오기입니다."
        )),
    );
    let docx = temp.path().join("검토보고서.docx");
    {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&docx).unwrap());
        zip.start_file(
            "word/document.xml",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(xml.as_bytes()).unwrap();
        zip.finish().unwrap();
    }
    let report = hlt_core::read_review_report(&docx).unwrap();
    assert_eq!(report.proposals.len(), 2);
    assert_eq!(report.notes.len(), 1, "{:?}", report.notes);

    let mut session = EditorSession::open(&fin).unwrap();
    let texts_before: Vec<String> = session.paragraphs().into_iter().map(|p| p.text).collect();
    let failed = session
        .apply_review(
            &report.proposals[1].claims,
            report.proposals[1].tracked,
            &[],
        )
        .unwrap();
    assert_eq!(
        failed.claims_failed,
        [(
            "1".to_string(),
            "보정 전 문언이 최종본과 다르다".to_string()
        )]
    );
    let outcome = session
        .apply_review(
            &report.proposals[0].claims,
            report.proposals[0].tracked,
            &report.notes,
        )
        .unwrap();
    assert_eq!(outcome.claims_changed, ["1"]);
    assert_eq!(outcome.claims_deleted, ["2"]);
    assert_eq!(outcome.claims_added, ["3"]);
    assert!(
        outcome.claims_failed.is_empty(),
        "{:?}",
        outcome.claims_failed
    );
    assert_eq!(outcome.notes_applied, [(0, 1)]);
    let texts: Vec<String> = session.paragraphs().into_iter().map(|p| p.text).collect();
    let at = texts.iter().position(|t| t == "【청구항 1】").unwrap();
    assert_eq!(
        texts[at..at + 5],
        [
            "【청구항 1】",
            "몸체와 바닥을 포함하는 장치,",
            "덮개를 더 포함하는 장치.",
            "【청구항 3】",
            "제1항에 있어서, 손잡이를 더 포함하는 장치."
        ]
    );
    assert!(texts.contains(&"본 발명은 검사 장치에 관한 것이다.".to_string()));
    assert!(
        texts.contains(&"시험 장치{TEST APPARATUS}".to_string()),
        "번호 문단 밖은 그대로"
    );

    let after = session.materialize().unwrap();
    let export = hlt_core::export_amendment_from_fin(
        &fin,
        &after,
        "spec_1.dta",
        "2026-10-22",
        &hlt_core::KipoExportOptions {
            paragraph_numbers: session.paragraph_numbers(),
            ..Default::default()
        },
        &hlt_core::AmendmentOptions {
            dta_version: String::new(),
            ..Default::default()
        },
    )
    .unwrap();
    let items: Vec<String> = export
        .xml
        .split("<AmendBody elementName=\"")
        .skip(1)
        .map(|item| {
            let kind = item.split('"').next().unwrap();
            let status = item
                .split("status=\"")
                .nth(1)
                .unwrap()
                .split('"')
                .next()
                .unwrap();
            let number = item
                .split("attributeValue=\"")
                .nth(1)
                .unwrap()
                .split('"')
                .next()
                .unwrap();
            format!("{kind} {status} {number}")
        })
        .collect();
    assert_eq!(
        items,
        ["p A 0001", "claim A 1", "claim D 2", "claim I 3"],
        "{}",
        export.xml
    );
    // 이음매: 보정 표시가 「바뀐 것」으로 보이는 요소 = 보정서가 내는 항목.
    let marks = session.amendment_marks().expect("최종본으로 연 문서");
    let element = |label: &str, status: &str| match label.strip_prefix("청구항 ") {
        Some(number) => format!("claim {status} {number}"),
        None => format!("p {status} {}", label.trim_matches(['【', '】'])),
    };
    let mut shown: Vec<String> = marks
        .changed
        .iter()
        .map(|label| element(label, "A"))
        .chain(marks.removed.iter().map(|label| element(label, "D")))
        .chain(marks.added.iter().map(|label| element(label, "I")))
        .collect();
    let mut expected = items.clone();
    shown.sort();
    expected.sort();
    assert_eq!(shown, expected, "{marks:?}");
    assert!(session.claim_citation_issues().is_empty());
    // 보정 근거: 바뀐 항(1)과 새 항(3)이 다 덮인다(지운 항 2 는 새 말이 없다).
    let basis = session.amendment_basis().unwrap();
    let claims: Vec<(u32, bool)> = basis.iter().map(|b| (b.claim, b.added)).collect();
    assert_eq!(claims, [(1, false), (3, true)], "{basis:?}");

    // 되돌리기 한 번에 전부.
    session.undo().unwrap();
    let texts: Vec<String> = session.paragraphs().into_iter().map(|p| p.text).collect();
    assert_eq!(texts, texts_before);
    assert_eq!(
        session.amendment_marks().unwrap(),
        hlt_core::AmendmentMarks::default()
    );
}
