use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CharacterFormat {
    Underline,
    Bold,
    Italic,
    Superscript,
    Subscript,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CharacterFormatSpan {
    pub paragraph_id: String,
    pub start: u32,
    pub end: u32,
    pub text: String,
    pub raw_code: u32,
    pub raw_attr: u32,
    pub formats: Vec<CharacterFormat>,
}

/// 글자 크기가 같은 이어진 구간. 문단 전체를 빈틈없이 덮는다.
/// 서식 스팬과 달리 **평문 구간도 낸다.** 통특기는 같은 굵게라도 본문에서 12pt,
/// 표 셀에서 10pt 를 쓴다. 크기는 서식 종류로 정해지지 않는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CharacterSizeRun {
    pub start: u32,
    pub end: u32,
    pub size: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormatParagraph {
    pub paragraph_id: String,
    pub text: String,
    pub spans: Vec<CharacterFormatSpan>,
    /// 글자 크기 구간. 문단 글자를 빈틈없이 덮는다.
    pub sizes: Vec<CharacterSizeRun>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnresolvedFormat {
    pub paragraph_id: String,
    pub line_id: String,
    pub character_index: Option<u32>,
    pub raw_code: Option<u32>,
    pub raw_attr: Option<u32>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormatInventory {
    pub source: String,
    pub source_sha256: String,
    pub paragraphs: Vec<FormatParagraph>,
    pub unresolved: Vec<UnresolvedFormat>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellCharacterFormatSpan {
    pub cell_id: String,
    pub start: u32,
    pub end: u32,
    pub text: String,
    pub raw_code: u32,
    pub raw_attr: u32,
    pub formats: Vec<CharacterFormat>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellFormatRecord {
    pub cell_id: String,
    pub table_id: String,
    pub text: String,
    pub spans: Vec<CellCharacterFormatSpan>,
    /// 글자 크기 구간. 칸 글자를 빈틈없이 덮는다. 표 셀은 본문과 다른 크기를 쓴다
    /// (PCT: 셀 10pt, 본문 12pt).
    pub sizes: Vec<CharacterSizeRun>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnresolvedCellFormat {
    pub cell_id: String,
    pub line_id: String,
    pub character_index: Option<u32>,
    pub raw_code: Option<u32>,
    pub raw_attr: Option<u32>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellFormatInventory {
    pub source: String,
    pub source_sha256: String,
    pub cells: Vec<CellFormatRecord>,
    pub unresolved: Vec<UnresolvedCellFormat>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRange {
    #[serde(with = "crate::serde_u64::as_string")]
    pub start_byte: u64,
    #[serde(with = "crate::serde_u64::as_string")]
    pub end_byte: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SectionRecord {
    pub id: String,
    pub index: u32,
    pub title: String,
    pub table_range: SourceRange,
    pub source_range: SourceRange,
    pub anchor_line_id: Option<String>,
    pub anchor_paragraph_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParagraphRecord {
    pub id: String,
    #[serde(with = "crate::serde_u64::as_string")]
    pub part: u64,
    pub section_id: Option<String>,
    pub text: String,
    pub text_truncated: bool,
    pub shape_id: Option<String>,
    pub indent_raw: u32,
    pub lspace: u32,
    pub align: String,
    pub line_ids: Vec<String>,
    pub source_range: SourceRange,
    pub wipo_id: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineRecord {
    pub id: String,
    pub paragraph_id: String,
    #[serde(with = "crate::serde_u64::as_string")]
    pub index: u64,
    /// 이 줄이 쓰는 문단모양. 줄 배열 12바이트 레코드의 첫 워드다.
    /// 표 셀은 칸마다 다른 모양을 쓸 수 있어서 문단 첫 줄만으로는 모자란다.
    pub shape_id: Option<String>,
    /// 이 줄이 **여는** 절. 절 줄지도(레지스터)가 이 줄에 실은 값이다.
    /// 절마다 정확히 한 줄이 연다(실물 50건, 정본 40개 절 전부).
    /// ⛔ 문단이 어느 절에 **딸리는가**(`ParagraphRecord::section_id`)와 다른 축이다.
    /// 표제를 달았지만 절 테이블에 없는 문단이 있다(정본 `domestic_출원서` 의
    /// `【도 12】`). 그런 줄은 여기가 `None` 이다.
    pub opens_section_id: Option<String>,
    pub source_range: SourceRange,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShapeRecord {
    pub id: String,
    pub index: u32,
    pub indent_raw: u32,
    pub lspace: u32,
    pub align_raw: u32,
    pub align: String,
    pub source_range: SourceRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentTreeKind {
    PctPatent,
    PatentSpecificationKo,
    /// 의견서(서식 7 · 실물 74건 — `0x8000` 표시가 안 붙는다).
    Opinion,
    Unknown,
}

impl DocumentTreeKind {
    /// 헤더 `0x20` 의 낮은 15비트가 서식 번호고 `0x8000` 은 따로 붙는 표시다.
    ///
    /// 실물 190건 전수(2026-08-27): 32908 = `0x8000 + 140` 이 44건(국내 출원서),
    /// 32879 = `0x8000 + 111` 이 42건, 32881 = `0x8000 + 113` 이 30건(PCT), 7 이 74건(의견서)이다.
    /// ⛔ **140 은 실물에 0건**이다. 우리 생성기가 쓰던 값이다.
    /// ⛔ 표시는 서식마다 다르다. 111, 113, 140 에는 붙고 **7(의견서)에는 안 붙는다.**
    /// `0x8000` 을 안 떼던 동안 실물 190건 중 **86건(45%)이 `Unknown`** 이었다.
    ///
    /// 111 이 무엇인지는 아직 모른다. 이름을 지어 붙이지 마라.
    pub(crate) fn from_raw(value: u32) -> Self {
        match value & 0x7fff {
            113 => Self::PctPatent,
            140 => Self::PatentSpecificationKo,
            7 => Self::Opinion,
            _ => Self::Unknown,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::PctPatent => "PCT특허",
            Self::PatentSpecificationKo => "특허 명세서등(국어)",
            Self::Opinion => "의견서",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectSummary {
    pub document_title: String,
    pub document_type_raw: u32,
    pub document_tree_kind: DocumentTreeKind,
    pub document_tree_label: String,
    pub fixed_blocks: FixedBlockFingerprints,
    #[serde(with = "crate::serde_u64::as_string")]
    pub inflated_bytes: u64,
    #[serde(with = "crate::serde_u64::as_string")]
    pub section_count: u64,
    #[serde(with = "crate::serde_u64::as_string")]
    pub paragraph_count: u64,
    #[serde(with = "crate::serde_u64::as_string")]
    pub line_count: u64,
    #[serde(with = "crate::serde_u64::as_string")]
    pub span_count: u64,
    #[serde(with = "crate::serde_u64::as_string")]
    pub table_count: u64,
    #[serde(with = "crate::serde_u64::as_string")]
    pub cell_count: u64,
    #[serde(with = "crate::serde_u64::as_string")]
    pub image_count: u64,
    #[serde(with = "crate::serde_u64::as_string")]
    pub text_index_bytes: u64,
    pub text_index_truncated: bool,
    #[serde(with = "crate::serde_u64::as_string")]
    pub paragraph_wipoid_nonzero: u64,
    #[serde(with = "crate::serde_u64::as_string")]
    pub paragraph_wipoid_distinct: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixedBlockFingerprints {
    pub shapes_sha256: String,
    pub shape_order_sha256: String,
    pub second_shape_order_sha256: String,
    pub style_table_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableRecord {
    pub id: String,
    #[serde(with = "crate::serde_u64::as_string")]
    pub occurrence: u64,
    pub anchor_section_id: Option<String>,
    pub anchor_paragraph_id: Option<String>,
    pub row_count: u32,
    pub column_count: u32,
    pub cell_ids: Vec<String>,
    pub source_range: SourceRange,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellRecord {
    pub id: String,
    pub table_id: String,
    pub row: u32,
    pub occurrence: u32,
    pub start: u32,
    pub width: u32,
    pub rowspan: u32,
    pub colspan: u32,
    /// 세로 맞춤(셀 레코드 +76): 0 위 · 1 가운데 · 그 밖은 미관측.
    #[serde(default)]
    pub vertical_align: u32,
    pub line_ids: Vec<String>,
    pub image_ids: Vec<String>,
    pub source_range: SourceRange,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageOccurrence {
    pub id: String,
    #[serde(with = "crate::serde_u64::as_string")]
    pub occurrence: u64,
    pub object_code: u32,
    pub payload_sha256: Option<String>,
    /// 통특기 렌더가 쓰는 표시 크기(pt 정수, 객체 레코드 +368/+364).
    pub display_width: u32,
    pub display_height: u32,
    /// 그림 상자(객체 레코드 +48/+52). 왕복 저장은 이 값을 그대로 다시 쓴다.
    /// 옛 판은 이 자리에 display_width 를 넣어 PCT 왕복본 그림 69장이 28% 로 줄었다
    /// (결함 D, 2026-09-06). serde 기본값은 옛 JSON 픽스처를 위한 것.
    #[serde(default)]
    pub box_width: u32,
    #[serde(default)]
    pub box_height: u32,
    pub paragraph_id: String,
    pub line_id: String,
    pub table_id: Option<String>,
    pub cell_id: Option<String>,
    pub source_range: SourceRange,
}
