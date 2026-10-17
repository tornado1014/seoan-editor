mod support;

use hlt_core::{InspectOptions, inspect_path};

/// 정본 `domestic_출원서.HLT` 실측(2026-08-27): 표 안 44줄이 전부 줄 높이 130 인
/// 문단모양(레코드 12, 22)을 쓴다. 판독기가 줄 높이를 한 레코드 앞에서 읽고 있으면
/// 레코드 22 가 레코드 21 의 160 을 물고 나온다. 그러면 다시 구울 때 표가 통특기에서
/// 쪽을 넘긴다(결함 A).
#[test]
fn reads_the_line_spacing_of_the_shape_the_line_actually_uses() {
    let fixture = support::application_table_fixture();
    let document = inspect_path(fixture.path(), InspectOptions::default()).unwrap();

    let shape = |id: &str| {
        document
            .shapes
            .iter()
            .find(|shape| shape.id == id)
            .unwrap_or_else(|| panic!("{id} 가 없다"))
    };

    // 표 안 문단모양 둘. 정본 바이트: 둘 다 들여쓰기 0, 정렬 2048, 줄 높이 130.
    for id in ["shape:12", "shape:22"] {
        let record = shape(id);
        assert_eq!(
            (record.indent_raw, record.align_raw, record.lspace),
            (0, 2048, 130),
            "{id} 가 정본 바이트와 다르다"
        );
    }

    // 표 바로 앞 본문 줄이 쓰는 모양. 정본 바이트: 들여쓰기 800, 정렬 2048, 줄 높이 280.
    let body = shape("shape:14");
    assert_eq!(
        (body.indent_raw, body.align_raw, body.lspace),
        (800, 2048, 280)
    );
}

/// ⛔ 정렬 코드는 1/1024/2048 뿐이 아니다. 실물 74건 전수에 상위 비트가 붙은 값이
/// `0x8001`, `0x1800`, `0x2800`, `0x8800`, `0xC400`, `0xC800` 으로 143 레코드 있다.
/// 그 값을 「테이블 끝」으로 보고 자르면 뒤 문단모양을 통째로 잃고, 그 모양을 쓰는 줄은
/// 다시 구울 때 기본값으로 무너진다. 실물 5건이 이 자리에 걸린다(정본 PCT 포함:
/// 모양 20 = 들여쓰기 800, 정렬 34816, 줄 높이 280 을 8줄이 쓴다).
/// 뜻은 `& 0xFFF` 로 읽고 상위 비트는 버리지 않는다.
#[test]
fn keeps_reading_shapes_past_an_alignment_with_high_bits() {
    let plain = inspect_path(
        support::application_table_fixture().path(),
        InspectOptions::default(),
    )
    .unwrap();
    let total = plain.shapes.len();
    assert!(total > 12, "표본에 모양이 {total} 개뿐이다");

    for (raw, name) in [
        (0x8001_u32, "center"),
        (0x1800, "justified"),
        (0x2800, "justified"),
        (0x8800, "justified"),
        (0xC400, "left"),
        (0xC800, "justified"),
    ] {
        let patched = support::application_table_fixture_with_align(12, raw);
        let document = inspect_path(patched.path(), InspectOptions::default()).unwrap();
        assert_eq!(
            document.shapes.len(),
            total,
            "정렬 {raw:#x} 에서 모양 테이블이 잘렸다"
        );
        let shape = document
            .shapes
            .iter()
            .find(|shape| shape.index == 12)
            .unwrap();
        assert_eq!(shape.align_raw, raw, "상위 비트를 버렸다");
        assert_eq!(shape.align, name);
        assert_eq!(shape.lspace, 130);
    }
}

/// ⛔ 표를 품은 문단이 자기 문단모양을 **표 칸 줄**에서 가져오면 안 된다.
/// 정본 `domestic_출원서` 의 `paragraph:147` 은 줄 653~697 이고, 653~696 이
/// 표 칸이고 697 이 표 뒤 본문이다. 문단 모양을 첫 줄(칸 줄, 모양 22 = 줄 높이 130)에서
/// 잡으면 다시 구울 때 그 본문 줄이 표 높이로 눌린다. 정본 PCT 에서는 같은 자리가
/// 17줄이었다(줄 1285~1301). 문단은 **칸에 들지 않은 첫 줄**의 모양을 쓴다.
/// 정본 바이트: 줄 697 은 모양 14 = 들여쓰기 800, justified, 줄 높이 280.
#[test]
fn a_paragraph_that_holds_a_table_keeps_its_own_shape() {
    let fixture = support::application_table_fixture();
    let document = inspect_path(fixture.path(), InspectOptions::default()).unwrap();

    let holder = document
        .paragraphs
        .iter()
        .find(|paragraph| paragraph.id == "paragraph:147")
        .expect("표를 품은 문단이 없다");
    assert_eq!(
        holder.line_ids.first().map(String::as_str),
        Some("line:653")
    );
    assert_eq!(holder.line_ids.last().map(String::as_str), Some("line:697"));

    assert_eq!(
        (holder.indent_raw, holder.align.as_str(), holder.lspace),
        (800, "justified", 280),
        "표를 품은 문단이 칸 줄의 모양을 물고 나왔다"
    );
}

/// ⛔ 줄 높이는 통특기가 쪽을 나누는 근거다. 다시 구운 문서의 **줄마다** 높이가
/// 원본과 같아야 한다. 총합만 보면 어느 자리가 늘고 어느 자리가 줄어 상쇄된다.
///
/// 2026-08-27 에 이 대조 하나가 결함 넷을 한꺼번에 드러냈다. 판독기가 줄 높이를 한
/// 레코드 앞에서 읽던 것, 국내 프로필 자산이 줄지도와 어긋나던 것, 정렬 상위 비트에서
/// 모양 테이블을 자르던 것, 표를 품은 문단이 칸 줄의 모양을 물고 나오던 것이다.
/// 그때 국내 표 44줄이 정본 5,720 대신 10,310 으로 나왔고 통특기가 마지막 행을
/// 다음 쪽으로 밀었다.
#[test]
fn a_rebaked_domestic_specification_keeps_every_line_height() {
    let fixture = support::application_table_fixture();
    let before = inspect_path(fixture.path(), InspectOptions::default()).unwrap();

    let (document, payloads) = hlt_core::hml_document_from_hlt(fixture.path()).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("rebaked.HLT");
    hlt_core::save_generated_hlt_as_new(
        fixture.path(),
        &document,
        hlt_core::GenerationDialect::PatentSpecificationKo,
        &payloads,
        &output,
        hlt_core::WriteOptions::default(),
    )
    .unwrap();
    let after = inspect_path(&output, InspectOptions::default()).unwrap();
    // 국내도 【청구범위】·【요약서】 앞에서 쪽을 나눈다(사용자 결정 2026-10-16) — 그 두 줄만 빼고 견준다.
    // 줄 id 는 차례 번호라 문서마다 따로 뺀다.
    assert_eq!(
        support::page_break_lines(&after).len(),
        2,
        "쪽나눔 줄 = 【청구범위】·【요약서】 앞 둘"
    );
    assert!(
        support::page_break_lines(&before).is_empty(),
        "국내 정본에는 쪽나눔 줄이 없다"
    );

    let heights = |document: &hlt_core::InspectDocument| -> Vec<u32> {
        let breaks = support::page_break_lines(document);
        document
            .lines
            .iter()
            .filter(|line| !breaks.contains(&line.id))
            .map(|line| {
                let id = line.shape_id.as_deref().expect("줄에 문단모양이 없다");
                document
                    .shapes
                    .iter()
                    .find(|shape| shape.id == id)
                    .unwrap_or_else(|| panic!("{id} 이 모양 테이블에 없다"))
                    .lspace
            })
            .collect()
    };

    let source = heights(&before);
    let rebaked = heights(&after);
    assert_eq!(source.len(), 866, "표본의 줄 수가 바뀌었다");
    assert_eq!(rebaked.len(), source.len(), "다시 구운 뒤 줄 수가 달라졌다");

    let differing: Vec<_> = source
        .iter()
        .zip(&rebaked)
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(line, (a, b))| format!("줄 {line}: 원본 {a}, 다시 구운 것 {b}"))
        .collect();
    assert!(
        differing.is_empty(),
        "줄 높이가 달라진 줄 {}건:\n{}",
        differing.len(),
        differing
            .iter()
            .take(12)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert_eq!(
        source.iter().sum::<u32>(),
        235_880,
        "정본 줄 높이 합 실측값"
    );
}

/// ⛔ 절 줄지도는 「이 줄이 어느 절을 여는가」다. 페이지브레이크가 첫 줄을 차지한 문단은
/// 절 표제가 **둘째 줄**에 있다. 문단의 절을 첫 줄로 정하면 그 문단이 앞 절을 물고 나오고,
/// 다시 구울 때 그 절이 앵커를 잃는다. 구조창에서 도면이 앞 절 밑으로 말려드는 것이 이것이다.
///
/// 정본 `domestic_출원서` 실측: 줄 832 가 페이지브레이크, 833 이 `【도면】`(절 29)이고
/// 둘이 한 문단이다. 같은 꼴이 837(절 31), 844(절 34), 849(절 36) 에도 있다.
#[test]
fn a_paragraph_opened_by_a_page_break_takes_the_section_on_its_title_line() {
    let fixture = support::application_table_fixture();
    let document = inspect_path(fixture.path(), InspectOptions::default()).unwrap();

    for (line, section) in [(833_u64, 29_u32), (837, 31), (844, 34), (849, 36)] {
        let holder = document
            .paragraphs
            .iter()
            .find(|paragraph| {
                paragraph
                    .line_ids
                    .iter()
                    .any(|id| id == &format!("line:{line}"))
            })
            .unwrap_or_else(|| panic!("줄 {line} 을 품은 문단이 없다"));
        assert_eq!(
            holder.section_id.as_deref(),
            Some(format!("section:{section}").as_str()),
            "줄 {line} 의 문단이 앞 절을 물고 나왔다"
        );
    }
}

/// ⛔ 절 줄지도는 절마다 슬롯이 **정확히 하나**다(실물 50건 실측, 정본도 40개 전부 하나).
/// 두 문단이 같은 절을 물면 뒷것이 앞것을 덮어 그 절이 앵커를 잃는다.
///
/// 정본 `domestic_출원서` 는 절 40개가 줄 0, 1, 4, 9, ... 861 을 하나씩 연다.
/// `【도 12】`(줄 864)는 절 테이블에 없어서 아무 절도 열지 않는다. 다시 구운 문서가
/// 그 자리에 절 40 을 한 번 더 찍으면 안 된다.
#[test]
fn a_rebaked_domestic_specification_keeps_every_section_anchor() {
    let fixture = support::application_table_fixture();
    let before = inspect_path(fixture.path(), InspectOptions::default()).unwrap();

    let (document, payloads) = hlt_core::hml_document_from_hlt(fixture.path()).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("rebaked.HLT");
    hlt_core::save_generated_hlt_as_new(
        fixture.path(),
        &document,
        hlt_core::GenerationDialect::PatentSpecificationKo,
        &payloads,
        &output,
        hlt_core::WriteOptions::default(),
    )
    .unwrap();
    let after = inspect_path(&output, InspectOptions::default()).unwrap();

    // 국내도 【청구범위】·【요약서】 앞에서 쪽을 나눈다(사용자 결정 2026-10-16) — 그 두 줄을 빼고 줄 차례로 견준다.
    // 줄 id 는 차례 번호라 문서마다 따로 뺀다. 쪽나눔 줄이 연 절은 바로 뒤 표제 줄이 연 것으로 친다.
    assert_eq!(
        support::page_break_lines(&after).len(),
        2,
        "쪽나눔 줄 = 【청구범위】·【요약서】 앞 둘"
    );
    let kept = |document: &hlt_core::InspectDocument| -> Vec<String> {
        let breaks = support::page_break_lines(document);
        document
            .lines
            .iter()
            .filter(|line| !breaks.contains(&line.id))
            .map(|line| line.id.clone())
            .collect()
    };
    let anchors = |document: &hlt_core::InspectDocument| -> Vec<(u32, Option<usize>)> {
        let order = kept(document);
        document
            .sections
            .iter()
            .map(|section| {
                (
                    section.index,
                    section.anchor_line_id.as_ref().map(|id| {
                        let at = document
                            .lines
                            .iter()
                            .position(|line| &line.id == id)
                            .unwrap();
                        let id = if support::page_break_lines(document).contains(id) {
                            &document.lines[at + 1].id
                        } else {
                            id
                        };
                        order.iter().position(|line| line == id).unwrap()
                    }),
                )
            })
            .collect()
    };

    assert_eq!(before.sections.len(), 40, "표본의 절 수가 바뀌었다");
    assert_eq!(anchors(&after), anchors(&before), "절 앵커가 달라졌다");

    // 절 목록의 앵커는 절마다 첫 슬롯만 집으므로 중복을 못 본다. 줄지도 원본을 댄다.
    // 쪽나눔 줄이 연 절은 바로 뒤 표제 줄로 옮겨 견준다.
    let register = |document: &hlt_core::InspectDocument| -> Vec<Option<String>> {
        let breaks = support::page_break_lines(document);
        let mut carried = None;
        let mut out = Vec::new();
        for line in &document.lines {
            if breaks.contains(&line.id) {
                carried = line.opens_section_id.clone();
            } else {
                out.push(carried.take().or_else(|| line.opens_section_id.clone()));
            }
        }
        out
    };
    let source = register(&before);
    let rebaked = register(&after);
    assert_eq!(source.len(), 866);
    let differing: Vec<_> = source
        .iter()
        .zip(&rebaked)
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(line, (a, b))| format!("줄 {line}: 원본 {a:?}, 다시 구운 것 {b:?}"))
        .collect();
    assert!(
        differing.is_empty(),
        "절 줄지도가 달라진 줄 {}건:
{}",
        differing.len(),
        differing.iter().take(12).cloned().collect::<Vec<_>>().join(
            "
"
        )
    );
    assert_eq!(
        source.iter().filter(|slot| slot.is_some()).count(),
        40,
        "정본 줄지도의 슬롯 수 실측값"
    );

    // 앵커 목록만 보면 중복을 놓친다(판독기가 첫 슬롯만 집는다). 절을 여는 문단이
    // 절마다 하나뿐인지 따로 본다. `【도 12】`(줄 864)는 절 테이블에 없으므로
    // 아무 절도 열지 않아야 한다.
    let opening: Vec<_> = after
        .sections
        .iter()
        .filter_map(|section| section.anchor_paragraph_id.clone())
        .collect();
    let mut unique = opening.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(
        unique.len(),
        opening.len(),
        "한 문단이 절 여럿을 연다고 나온다"
    );
    let tail = after
        .paragraphs
        .iter()
        .find(|paragraph| paragraph.line_ids.iter().any(|id| id == &kept(&after)[864]))
        .expect("줄 864 를 품은 문단이 없다");
    assert!(
        !opening.iter().any(|id| id == &tail.id),
        "절 테이블에 없는 `【도 12】` 문단이 절을 연다"
    );
}
