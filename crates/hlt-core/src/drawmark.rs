//! 도면부호 목록 — `C:\KIPONET\NKEditor\drawmark\<문서 이름>.drf`.
//!
//! 통특기와 같은 꼴:
//! - 문서를 저장할 때 문서 이름으로 따로 쓰는 EUC-KR XML · 줄끝 CRLF · `<Part index>` 는 0부터.
//! - 「문서에 삽입」 은 고른 한 행을 커서 자리에 `구분⇥부호⇥설명` 글로 넣는다(여러 행 선택 없음).

use quick_xml::{Reader, events::Event};

use crate::{CoreError, ErrorCode};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DrawMark {
    /// 도면 구분(`도 1`).
    pub figure: String,
    /// 부호(`100`).
    pub sign: String,
    /// 설명(`냉각 장치`).
    pub description: String,
}

impl DrawMark {
    /// 「문서에 삽입」 이 넣는 글.
    pub fn insert_text(&self) -> String {
        format!("{}\t{}\t{}", self.figure, self.sign, self.description)
    }
}

pub fn write_drf(marks: &[DrawMark]) -> Result<Vec<u8>, CoreError> {
    let escape = |text: &str| quick_xml::escape::escape(text).into_owned();
    let mut xml =
        String::from("<?xml version=\"1.0\" encoding=\"euc-kr\"?>\r\n<DrawReference>\r\n");
    for (index, mark) in marks.iter().enumerate() {
        xml.push_str(&format!(
            "<Part index=\"{index}\">\r\n<DrawName>{}</DrawName>\r\n<DrawSignal>{}</DrawSignal>\r\n<DrawDes>{}</DrawDes>\r\n</Part>\r\n",
            escape(&mark.figure),
            escape(&mark.sign),
            escape(&mark.description)
        ));
    }
    xml.push_str("</DrawReference>\r\n");
    let (bytes, _, unmappable) = encoding_rs::EUC_KR.encode(&xml);
    if unmappable {
        return Err(drf_error("euc_kr_unrepresentable"));
    }
    Ok(bytes.into_owned())
}

pub fn read_drf(bytes: &[u8]) -> Result<Vec<DrawMark>, CoreError> {
    let (text, _, malformed) = encoding_rs::EUC_KR.decode(bytes);
    if malformed {
        return Err(drf_error("euc_kr_malformed"));
    }
    let mut reader = Reader::from_str(&text);
    let mut marks = Vec::new();
    let mut field: Option<&'static str> = None;
    let mut current = DrawMark {
        figure: String::new(),
        sign: String::new(),
        description: String::new(),
    };
    loop {
        match reader.read_event().map_err(|_| drf_error("xml"))? {
            Event::Start(tag) => {
                field = match tag.name().as_ref() {
                    b"DrawName" => Some("figure"),
                    b"DrawSignal" => Some("sign"),
                    b"DrawDes" => Some("description"),
                    _ => None,
                }
            }
            Event::Text(value) => {
                let value = value.decode().map_err(|_| drf_error("xml"))?;
                let value = quick_xml::escape::unescape(&value)
                    .map_err(|_| drf_error("xml"))?
                    .into_owned();
                match field {
                    Some("figure") => current.figure.push_str(&value),
                    Some("sign") => current.sign.push_str(&value),
                    Some("description") => current.description.push_str(&value),
                    _ => {}
                }
            }
            Event::End(tag) => {
                field = None;
                if tag.name().as_ref() == b"Part" {
                    marks.push(std::mem::replace(
                        &mut current,
                        DrawMark {
                            figure: String::new(),
                            sign: String::new(),
                            description: String::new(),
                        },
                    ));
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(marks)
}

fn drf_error(reason: &str) -> CoreError {
    let mut error = CoreError::new(ErrorCode::StructureInvalid, "drawmark", false);
    error.details.insert("reason".into(), reason.into());
    error
}
