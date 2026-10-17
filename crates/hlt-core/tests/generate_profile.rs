use hlt_core::{GenerationDialect, generation_profile};
use sha2::{Digest, Sha256};

fn sha(bytes: &[u8]) -> String {
    format!("{:X}", Sha256::digest(bytes))
}

#[test]
fn generation_profiles_match_the_measured_blocks() {
    let regular = generation_profile(GenerationDialect::PatentSpecificationKo);
    // ⛔ 내장 헤더 파일에는 맨 140 이 들어 있지만 `generation_profile` 이 통특기 값으로 덮는다.
    // 실물 국내 출원서 44건은 `0x8000` 표시를 달고 있고, 그 44건이 쓰는 값은
    // `0x8000 + 140` = 32908 이다(전수).
    assert_eq!(
        u32::from_le_bytes(regular.header[0x20..0x24].try_into().unwrap()),
        32_908
    );
    assert_eq!(regular.shape_count, 13);
    assert_eq!(
        (regular.ending.len(), sha(&regular.ending)),
        (
            52,
            "4E1CD04212DE426F779DC501071C5542C4B0E16C07A4EA3C310098F6875D4BFD".to_string()
        )
    );
    // ⛔ 2026-08-27 에 모양 8, 10, 11 의 줄 높이를 정본 `domestic_출원서.HLT` 에서
    // 옮기면서 이 해시가 바뀌었다. 바뀐 것은 네 워드뿐이다(오프셋 608, 740, 752, 824).
    // 그 전에는 「모양 10 = 줄 높이 130」이라는 줄지도와 달리 자산이 280 을 담고 있어
    // 국내 표가 정본보다 1.8배 높게 그려졌다(결함 A).
    assert_eq!(
        (regular.shapes.len(), sha(&regular.shapes)),
        (
            928,
            // 2026-09-06: 모양 7(들여쓰기 0, 왼쪽, 280)의 표 경계 플래그를 뗐다(정본 domestic 모양 7 = 0). 절 앵커가 이 모양을 쓴다.
            // 2026-09-06 밤: 모양 12(표 안 가운데, 0/1/130, field7=2) 72바이트를 뒤에 붙였다.
            // 통특기 실물 center_cell.HLT 의 레코드와 바이트 동일.
            "24529BDBDFBBA7814FDDC3DC6E4DE354EDE2CB8F13D4E8E9648E5AB2E16A9544".to_string()
        )
    );
    assert_eq!(
        (regular.shape_order.len(), sha(&regular.shape_order)),
        (
            189,
            "DB67DD49C9A40477E2BBEC5137008AF099E489C54C8FC443DCCAFC4CA15106BF".to_string()
        )
    );
    assert_eq!(
        (regular.geometry.len(), sha(&regular.geometry)),
        (
            73,
            "4A04C831D80DB1FC6AC73FCEA3405725A85CF5CABA001F9B5408BF03800803EA".to_string()
        )
    );
    assert_eq!(
        (
            regular.second_shape_order.len(),
            sha(&regular.second_shape_order)
        ),
        (
            91,
            "D0DCEE996BA9BC55A3A4B1D496490DFC62742451B6F00DB736A27D5B84F0E44B".to_string()
        )
    );
    assert_eq!(
        (regular.style_table.len(), sha(&regular.style_table)),
        (
            389,
            // 2026-09-06: 표 레코드 f13/f17/f18 를 실물 값(2048, 280, 0)으로 고쳤다. 옛 값(0x2000000, 0x60000, 64)은
            // 통특기가 표 뒤 조판을 버려 51쪽 문서가 36쪽이 됐다(결함 C).
            // 2026-09-07: 블록 전체를 실물(domestic.HLT 의 앞 두 레코드 384B · 6건 공통, 360~363 만 문서별)에서 다시 떴다.
            //   옛 자산은 둘째 레코드 이름이 쓰레기(196~224)였고 그 뒤가 4바이트 밀려 있었다(264~318). 360~363 은 국내 실물 값 ffffffff.
            "A30A0974068372883E0B5E022E7E739FD81AE8965E0D54D50FEEDBEBBCD9B809".to_string()
        )
    );
    assert_eq!(
        (regular.pre_body_gap.len(), sha(&regular.pre_body_gap)),
        (
            67,
            "5D68DBBCA51DA90D4EB2C10DF506C10E82ABEEAB9FDCB1AB86870DF95562421A".to_string()
        )
    );

    let pct = generation_profile(GenerationDialect::PctPatent);
    assert_eq!(
        u32::from_le_bytes(pct.header[0x20..0x24].try_into().unwrap()),
        32_881
    );
    assert_eq!(pct.shape_count, 23);
    assert_eq!(sha(&pct.ending), sha(&regular.ending));
    assert_eq!(
        (pct.shapes.len(), sha(&pct.shapes)),
        (
            1648,
            // 2026-09-06 밤: 모양 22(표 안 가운데 = 모양 14 복사 + 정렬 1) 72바이트를 붙였다.
            "830C7A3B5C9D91EAB0834C8CF9E43E21B25A8DEAE4E5DD4384B9C59BF58ED130".to_string()
        )
    );
    assert_eq!(
        (pct.shape_order.len(), sha(&pct.shape_order)),
        (
            373,
            "64B4746DBFFC7F078F718945B70A823A03DC6A966050ED43593E87D55288C4F8".to_string()
        )
    );
    assert_eq!(
        (pct.geometry.len(), sha(&pct.geometry)),
        (
            73,
            "88D18C9EC6ACD820D3A48FF28AF86CC05731B86C2DE03AA8D8D326FFC3EA94BE".to_string()
        )
    );
    assert_eq!(
        (pct.style_table.len(), sha(&pct.style_table)),
        (
            965,
            "2C10EEAB1F6578F30F4F858320E498C65D163E2DE2655394DB0F06A41A866DDC".to_string()
        )
    );
    assert_eq!(
        (pct.pre_body_gap.len(), sha(&pct.pre_body_gap)),
        (
            67,
            "71E01E91D5FC5E5755CB9727FBAA5A5EE88D12860EB7D17EA5A10461C1BFE7C4".to_string()
        )
    );
}
