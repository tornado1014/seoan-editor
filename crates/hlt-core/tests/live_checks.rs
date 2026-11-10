//! 상시 점검: 제출 전 점검을 오류 · 경고로 가르고, 문서를 바꾸지 않는다.

use hlt_core::{
    EditorSession, Severity, SpecKind, SpecOptions, SubmitIssue, SubmitKind,
    save_new_specification, severity,
};

fn issue(kind: SubmitKind, code: Option<&'static str>) -> SubmitIssue {
    SubmitIssue {
        kind,
        code,
        paragraph_id: None,
        detail: String::new(),
    }
}

#[test]
fn severity_splits_coded_rules_from_confirm_only_ones() {
    assert_eq!(
        severity(&issue(SubmitKind::Structure, Some("E-218"))),
        Severity::Error
    );
    for code in ["E-249", "E-266", "E-271", "E-215"] {
        assert_eq!(
            severity(&issue(SubmitKind::Format, Some(code))),
            Severity::Warning,
            "{code}"
        );
    }
    assert_eq!(
        severity(&issue(SubmitKind::Spacing, None)),
        Severity::Warning
    );
    assert_eq!(
        severity(&issue(SubmitKind::Citation, None)),
        Severity::Error
    );
}

/// 빈 식별번호 · 겹친 공백 · 청구항 인용 문제가 있는 세션(고침 둘).
fn edited() -> (tempfile::TempDir, EditorSession) {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("명세서.HLT");
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
    let texts: Vec<(String, String)> = session
        .paragraphs()
        .iter()
        .map(|p| (p.id.clone(), p.text.clone()))
        .collect();
    let at = texts
        .iter()
        .position(|(_, t)| t.contains("청구항 1"))
        .unwrap();
    let body = &texts[at + 1].0;
    session
        .replace_text(body, "제 3 항에 있어서,  장치.")
        .unwrap();
    (temp, session)
}

#[test]
fn live_checks_cover_every_submission_issue_and_grade_them() {
    let (_temp, session) = edited();
    let live = session.live_checks();
    let mut want: Vec<_> = session
        .submission_checks()
        .into_iter()
        .map(|i| (i.code, i.paragraph_id))
        .collect();
    let mut got: Vec<_> = live
        .iter()
        .map(|i| (i.code, i.paragraph_id.clone()))
        .collect();
    want.sort();
    got.sort();
    assert!(!want.is_empty());
    assert_eq!(got, want);
    assert!(
        live.iter().any(|i| i.severity == Severity::Error),
        "{live:?}"
    );
    assert!(
        live.iter().any(|i| i.severity == Severity::Warning),
        "{live:?}"
    );
}

#[test]
fn live_checks_do_not_change_the_document() {
    let (temp, mut session) = edited();
    let (_twin_temp, mut twin) = edited();
    let state = |s: &EditorSession| format!("{:?}", (s.paragraphs(), s.is_dirty(), s.revision()));
    let before = state(&session);
    let _ = session.live_checks();
    let _ = session.live_checks();
    assert_eq!(state(&session), before);
    let (a, b) = (temp.path().join("a.HLT"), temp.path().join("b.HLT"));
    session.save_as(&a).unwrap();
    twin.save_as(&b).unwrap();
    assert_eq!(std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap());
    // 되돌리기 칸수도 그대로 — 고침 하나를 거쳐 처음 판까지 물러서면 같은 걸음 수다.
    let steps = |s: &mut EditorSession| (0..10).take_while(|_| s.undo().unwrap()).count();
    assert_eq!(steps(&mut session), steps(&mut twin));
}

#[test]
fn revision_grows_on_edit_undo_and_redo() {
    let (_temp, mut session) = edited();
    let r0 = session.revision();
    assert!(session.undo().unwrap());
    let r1 = session.revision();
    assert!(session.redo().unwrap());
    let r2 = session.revision();
    assert!(r0 < r1 && r1 < r2);
}
