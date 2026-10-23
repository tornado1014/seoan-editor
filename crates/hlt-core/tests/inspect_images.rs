mod support;

use std::path::{Path, PathBuf};

use hlt_core::{InspectOptions, inspect_path};
use sha2::{Digest, Sha256};

fn repo(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path)
}

#[test]
fn preserves_281_image_occurrences_independent_of_object_code() {
    let cases = [
        ("fixtures/align/align_a.HLT", 1_u32, 288_u32),
        ("work/pct_a/pct_a_full_v5.HLT", 24_u32, 304_u32),
    ];
    for (relative, first_code, last_code) in cases {
        let document = inspect_path(
            &repo(relative),
            InspectOptions {
                include_image_identities: true,
                ..InspectOptions::default()
            },
        )
        .unwrap();
        assert_eq!(document.images.len(), 281);
        assert_eq!(document.images.first().unwrap().id, "image:0");
        assert_eq!(document.images.last().unwrap().id, "image:280");
        assert_eq!(document.images.first().unwrap().object_code, first_code);
        assert_eq!(document.images.last().unwrap().object_code, last_code);
        let identities: Vec<_> = document
            .images
            .iter()
            .map(|image| image.payload_sha256.as_deref().unwrap())
            .collect();
        let sequence = serde_json::to_vec(&identities).unwrap();
        assert_eq!(
            format!("{:X}", Sha256::digest(sequence)),
            "B90834A67EBCDA65AF267A9E441BDB44C466B68624143385FDE740337683684A"
        );
        assert_eq!(
            document
                .images
                .iter()
                .filter(|image| image.cell_id.is_some())
                .count(),
            16
        );
        for (occurrence, image) in document.images.iter().enumerate() {
            assert_eq!(image.occurrence, occurrence as u64);
            assert!(image.source_range.start_byte < image.source_range.end_byte);
            assert!(image.paragraph_id.starts_with("paragraph:"));
            assert!(image.line_id.starts_with("line:"));
        }
    }
}

#[test]
#[ignore = "실물 work/pct_a 필요(공개 저장소에 없음 · 있으면 --ignored 로)"]
fn reads_grayscale_tiff_payloads_from_the_native_pct_file() {
    // 네이티브 PCT 저장본 실측: 이미지 70장이 전부 8비트 그레이스케일 TIFF 다
    // (SamplesPerPixel=1, BitsPerSample=8, Photometric=1, Compression=1,
    //  StripOffsets=364, StripByteCounts = 가로 × 세로). pct_a 저장본은 RGB 라
    // 리더는 두 가지를 다 읽어야 한다.
    let document = inspect_path(
        &repo("work/pct_a/diagnostics/pct_a_source.HLT"),
        InspectOptions {
            include_image_identities: true,
            ..InspectOptions::default()
        },
    )
    .unwrap();

    assert_eq!(document.images.len(), 69);
    let identities: Vec<_> = document
        .images
        .iter()
        .filter_map(|image| image.payload_sha256.as_deref())
        .collect();
    assert_eq!(identities.len(), 69);
}

/// 국내출원 저장본의 1비트 G4 압축 TIFF 는 우리가 아직 못 푼다. 픽셀 경로는 이미
/// 원본 바이트만 보존하고 넘어가는데, **지문 계산은 통째로 죽고 있었다.**
/// 그래서 `inspect --kind images` 가 국내 명세서에서 `geometry_invalid` 로 떨어졌다.
/// 못 푸는 그림은 지문을 비우고, 판독 자체는 계속 되어야 한다.
#[test]
fn a_compressed_tiff_leaves_the_fingerprint_empty_instead_of_failing() {
    let fixture = support::domestic_specification_fixture();
    let document = hlt_core::inspect_path(
        fixture.path(),
        hlt_core::InspectOptions {
            include_image_identities: true,
            ..hlt_core::InspectOptions::default()
        },
    )
    .unwrap();

    assert!(!document.images.is_empty());
    assert!(
        document
            .images
            .iter()
            .any(|image| image.payload_sha256.is_none()),
        "못 푸는 그림이 하나도 없다면 표본이 바뀐 것이다"
    );
}
