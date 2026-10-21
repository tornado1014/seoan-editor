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
