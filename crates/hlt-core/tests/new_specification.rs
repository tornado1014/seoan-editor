//! 새 명세서: 통특기 2026.11.05 판 「국내출원 새문서」로 만든 빈 명세서(fixtures/new_spec)가 정답지다.
//! 같은 선택사항으로 서안이 만든 문서의 문단 글 차례와 문단 모양(정렬 · 들여쓰기)이 같아야 한다.

use hlt_core::{EditorSession, SpecKind, SpecOptions, save_new_specification};

fn fixture(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/new_spec")
        .join(name)
}

/// (글, 정렬, 들여쓰기) 문단 차례.
fn shape(path: &std::path::Path) -> Vec<(String, String, i64)> {
    let session = EditorSession::open(path).unwrap();
    session
        .document
        .paragraphs
        .iter()
        // 통특기 바탕 문단은 정렬이 「기본 모양」(default)이고, 서안이 고쳐 구우면 같은 정렬을 「양쪽」(justified)으로 적는다
        // — 서안이 고친 모든 문서가 그렇다(이 기능의 차이가 아니다).
        .map(|p| {
            let align = if p.align == "default" {
                "justified".to_string()
            } else {
                p.align.clone()
            };
            (p.text.trim().to_string(), align, i64::from(p.indent_raw))
        })
        .collect()
}

fn made(kind: SpecKind, options: SpecOptions) -> Vec<(String, String, i64)> {
    let temp = tempfile::tempdir().unwrap();
    let out = temp.path().join("새 명세서.HLT");
    save_new_specification(&out, kind, options).unwrap();
    shape(&out)
}

#[test]
fn new_specifications_match_what_the_editor_makes() {
    let full = SpecOptions {
        drawings: true,
        ..Default::default()
    };
    for (name, kind, options) in [
        ("patent_plain.HLT", SpecKind::Patent, SpecOptions::default()),
        (
            "patent_deferred.HLT",
            SpecKind::Patent,
            SpecOptions {
                deferred: true,
                ..full
            },
        ),
        (
            "patent_industrial.HLT",
            SpecKind::Patent,
            SpecOptions {
                industrial: true,
                ..full
            },
        ),
        ("pct_plain.HLT", SpecKind::Pct, SpecOptions::default()),
    ] {
        assert_eq!(made(kind, options), shape(&fixture(name)), "{name}");
    }
}

#[test]
fn utility_keeps_drawings_and_pct_has_no_deferral() {
    let utility = made(SpecKind::Utility, SpecOptions::default());
    assert!(
        utility.iter().any(|(t, ..)| t == "【도면】"),
        "실용신안은 도면이 늘 있다"
    );
    assert!(utility.iter().any(|(t, ..)| t == "【고안의 명칭】"));
    // 실용신안은 서안이 아직 고쳐 굽지 못한다 — 선택사항은 거부(조용히 무시하지 않는다).
    let temp = tempfile::tempdir().unwrap();
    let refused = save_new_specification(
        &temp.path().join("u.HLT"),
        SpecKind::Utility,
        SpecOptions {
            deferred: true,
            ..Default::default()
        },
    );
    assert!(refused.is_err());
    let pct = made(
        SpecKind::Pct,
        SpecOptions {
            deferred: true,
            drawings: true,
            ..Default::default()
        },
    );
    assert!(
        pct.iter().any(|(t, ..)| t == "【청구범위】"),
        "PCT 는 제출유예가 없다"
    );
}

#[test]
fn new_specifications_pass_the_structure_checks() {
    let temp = tempfile::tempdir().unwrap();
    for (n, (kind, industrial)) in [
        (SpecKind::Patent, true),
        (SpecKind::Utility, false),
        (SpecKind::Pct, true),
    ]
    .into_iter()
    .enumerate()
    {
        let out = temp.path().join(format!("{n}.HLT"));
        save_new_specification(
            &out,
            kind,
            SpecOptions {
                drawings: true,
                industrial,
                ..Default::default()
            },
        )
        .unwrap();
        let codes: Vec<_> = EditorSession::open(&out)
            .unwrap()
            .submission_checks()
            .into_iter()
            .filter_map(|issue| issue.code)
            .filter(|code| matches!(*code, "E-244" | "E-201" | "E-249"))
            .collect();
        assert!(codes.is_empty(), "{kind:?} {codes:?}");
    }
}
