//! 서열목록(ST.26 XML) — 통특기 「입력 › 서열목록 삽입」과 같은 꼴.
//! HLT 에는 파일 내용이 들어가지 않는다: 발명의 설명 끝에 `【서열목록】` 표제와 그 아래 한 줄
//! `서열목록 전자파일 첨부(<절대 경로>)` · 빈 문단을 둔다(【서열목록】 책갈피는 표제가 아니라 그 아래 줄에 걸린다).
//! 제출파일을 만들 때 그 경로의 파일을 hlz 에 `st26sdosl.xml` 로 그대로 싣고, 명세서 XML 의 `<description>` 끝에
//! `<sequence-list-doc>` 를 둔다(경로는 빼고 「서열목록 전자파일 첨부」만 · 번호를 받는다).

use std::path::Path;

use crate::{CoreError, ErrorCode};

pub const HEADING: &str = "【서열목록】";
pub const LINK_PREFIX: &str = "서열목록 전자파일 첨부";
/// hlz 안 이름(통특기가 늘 이 이름으로 바꿔 싣는다 · 원래 이름과 상관없다).
pub const HLZ_NAME: &str = "st26sdosl.xml";

pub fn link_text(path: &Path) -> String {
    format!("{LINK_PREFIX}({})", path.display())
}

/// `서열목록 전자파일 첨부(경로)` 줄이면 경로(빈 괄호면 빈 글).
pub fn link_path(text: &str) -> Option<&str> {
    text.trim()
        .strip_prefix(LINK_PREFIX)?
        .strip_prefix('(')?
        .strip_suffix(')')
}

pub fn is_link(text: &str) -> bool {
    link_path(text).is_some()
}

/// ST.26 파일인가 — 앞머리에 뿌리 요소 `<ST26SequenceListing` 이 있어야 한다(ST.25 텍스트 · 다른 XML 은 거절).
pub fn read_st26(path: &Path) -> Result<Vec<u8>, CoreError> {
    let bytes = std::fs::read(path).map_err(|_| sequence_error("sequence_file_missing"))?;
    let head = &bytes[..bytes.len().min(8192)];
    if head
        .windows(b"<ST26SequenceListing".len())
        .any(|window| window == b"<ST26SequenceListing")
    {
        Ok(bytes)
    } else {
        Err(sequence_error("sequence_not_st26"))
    }
}

pub(crate) fn sequence_error(reason: &str) -> CoreError {
    let mut error = CoreError::new(ErrorCode::UnsupportedFormat, "sequence_listing", false);
    error.details.insert("reason".into(), reason.into());
    error
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_line_round_trips_and_st26_is_checked() {
        let path = Path::new("/tmp/서열 목록.xml");
        assert_eq!(link_path(&link_text(path)), Some("/tmp/서열 목록.xml"));
        assert_eq!(link_path("서열목록 전자파일 첨부()"), Some(""));
        assert_eq!(link_path("서열목록 전자파일 첨부"), None);
        assert_eq!(link_path("서열번호 1의 핵산"), None);

        let dir = std::env::temp_dir().join(format!("seoan-seq-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let good = dir.join("a.xml");
        std::fs::write(
            &good,
            "<?xml version=\"1.0\"?>\n<ST26SequenceListing dtdVersion=\"V1_3\"></ST26SequenceListing>",
        )
        .unwrap();
        assert!(read_st26(&good).is_ok());
        let st25 = dir.join("b.app");
        std::fs::write(&st25, "<110>  applicant\n<210>  1\n").unwrap();
        let reason = |path: &Path| read_st26(path).unwrap_err().details["reason"].clone();
        assert_eq!(reason(&st25), "sequence_not_st26");
        assert_eq!(reason(&dir.join("none.xml")), "sequence_file_missing");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
