//! 편집기 세션(P5) — 화면이 쓰는 문서 상태. 원본은 건드리지 않고 작업 폴더에 판마다 새 사본을 쓴다.
//!
//! 편집 = 의미 모델(`HmlDocument`)을 고치고 다시 굽는다(`save_generated_hlt_as_new`). 고친 문단은 원본 줄나눔을 버리고
//! 통특기 저장 줄나눔(R15)으로 다시 흘린다 · 안 고친 문단은 원본 줄나눔 그대로다(왕복 인쇄 = 통특기 인쇄).
//! 글을 바꿔도 글자 서식은 남긴다: 앞뒤 같은 글자는 제 서식 · 새 글자는 바로 앞 글자 서식.
//! 되돌리기 = 앞 판으로. 판 0 = 연 파일의 사본(다시 굽지 않는다 — 고치지 않으면 바이트 그대로 저장된다).

use std::path::{Path, PathBuf};

use crate::{
    CharacterFormat, CoreError, ErrorCode, GeneratedImagePayload, GenerationDialect, HmlDocument,
    HmlInline, InspectDocument, InspectOptions, PagePlan, PrintOptions, WriteOptions,
    hml_document_from_hlt, inflate_hlt, inspect_path, number_paragraphs_as_new, page_plan,
};

pub struct EditorSession {
    dir: tempfile::TempDir,
    versions: Vec<Version>,
    current: usize,
    /// 연 판 · 마지막으로 저장한 판(그 판이 되돌리기 뒤 새 고침으로 버려지면 `None`). 지금 판과 다르면 「고친 것이 있다」.
    saved: Option<usize>,
    /// 다시 굽는 법. 없으면 보기 전용(내장 틀도 원본 틀도 못 쓰는 서식).
    dialect: Option<Recipe>,
    pub document: InspectDocument,
    pub plan: PagePlan,
    pub print: PrintOptions,
    /// 문단 → 서식 구간(판마다 다시 읽는다).
    spans: std::collections::BTreeMap<String, Vec<EditorSpan>>,
    /// 연 파일의 의미 모델을 뒤에서 만드는 일(pct_b 20초+ — 첫 고침이 기다리지 않게 열자마자 시작한다).
    background: Option<std::thread::JoinHandle<Result<Model, CoreError>>>,
    /// 최종본(.fin)을 열 때 읽은 식별번호(가져온 문단 차례). 첫 판 모델에 입힌다 — HLT 에는 실리지 않는다.
    opened_numbers: Vec<Option<String>>,
    /// 지금 판의 식별번호(화면 문단 차례) — 모델이 문단과 함께 들고 다닌다.
    numbers: Vec<Option<String>>,
    /// 마지막 고침이 CP949 에 없어 바꾼 글자들(붙여넣기 · 타이핑) — 화면이 알린다.
    pub cp949_substitutions: Vec<crate::Cp949Substitution>,
}

/// 판 하나: 파일 · 그 판의 의미 모델(있으면 다시 해석하지 않는다) · 다시 구운 판인가(저장 때 전체 검증).
struct Version {
    /// 화면용 파일. 다시 구운 판은 「배치본」(그림을 1×1 로 바꿔 구운 작은 파일 — 쪽 배치는 진짜와 같다).
    path: PathBuf,
    /// 다시 구운 판의 진짜 파일(그림 본문까지) — 저장·제출·인쇄가 부를 때 굽는다(`materialize`).
    real: Option<PathBuf>,
    model: Option<Model>,
    generated: bool,
}

/// 의미 모델 + 그림 본문. 그림 본문은 판끼리 나눠 쓴다(pct_a 469MB — 글·서식만 고치면 복사하지 않는다).
#[derive(Clone)]
struct Model {
    /// 이 모델을 해석해 온 파일(`document.source_sha256` 의 주인 — 검증 저장이 대조한다).
    origin: PathBuf,
    document: HmlDocument,
    payloads: std::sync::Arc<Vec<GeneratedImagePayload>>,
}

/// 찾기: 글(비면 서식만으로 — 그 서식이 이어진 구간) · 서식 조건.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct FindQuery {
    pub text: String,
    /// `None` = 서식 상관없음.
    pub format: Option<FindFormat>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindFormat {
    Bold,
    Italic,
    Underline,
    Superscript,
    Subscript,
    /// 서식 없는 글자만.
    Plain,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FindMatch {
    pub paragraph_id: String,
    pub start: usize,
    pub end: usize,
    /// 앞뒤 맥락(화면 목록용).
    #[serde(default)]
    pub context: String,
}

/// 바꿀 글의 서식: 찾은 첫 글자 서식에서 출발해 고친다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplaceFormat {
    Keep,
    Superscript,
    Subscript,
    /// 첨자만 없앤다.
    Normal,
    Bold,
    Italic,
    Underline,
    /// 서식을 모두 없앤다.
    Clear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FigureKind {
    Math,
    Chemistry,
}

impl FigureKind {
    fn label(self) -> &'static str {
        match self {
            Self::Math => "수학식",
            Self::Chemistry => "화학식",
        }
    }
}

/// 화면에 보내는 문단 하나 — 편집 창이 글을 바꿀 때 쓴다.
#[derive(Debug, Clone, serde::Serialize)]
pub struct EditorParagraph {
    pub id: String,
    pub text: String,
    pub wipo_id: Option<u32>,
    /// 식별번호(최종본을 연 문서만 · 통특기는 문단 왼쪽에 「[0001]」).
    pub number: Option<String>,
    /// 그림 · 표가 든 문단(글자 자리표 `_` 를 지우면 그림이 빠진다).
    pub has_objects: bool,
    /// 글자 서식 구간(글 [start, end) · 비트 = 밑줄 1 · 굵게 2 · 기울임 4 · 위첨자 16 · 아래첨자 32) · 크기(12 아닌 것).
    pub spans: Vec<EditorSpan>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct EditorSpan {
    pub start: u32,
    pub end: u32,
    pub style: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u32>,
}

/// 글자 한 칸(서식 · 크기 포함) 또는 그림.
#[derive(Debug, Clone, PartialEq)]
enum Cell {
    Char(char, Vec<CharacterFormat>, Option<u32>),
    Image(String),
}

impl EditorSession {
    pub fn open(source: &Path) -> Result<Self, CoreError> {
        let dir = tempfile::tempdir().map_err(|_| editor_error("workdir"))?;
        let first = dir.path().join("v0.HLT");
        // 제출파일(.fin 최종본 · .hlz)은 풀어서 HLT 로 굽고 연다.
        let package = source
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| {
                value.eq_ignore_ascii_case("fin") || value.eq_ignore_ascii_case("hlz")
            });
        let mut opened_numbers = Vec::new();
        if package {
            let work = dir.path().join("package");
            std::fs::create_dir(&work).map_err(|_| editor_error("workdir"))?;
            opened_numbers = crate::hlt_from_package(source, &work, &first)?;
        } else {
            std::fs::copy(source, &first).map_err(|_| editor_error("copy"))?;
        }
        let (document, plan) = load(&first, PrintOptions::default())?;
        let dialect = match document.summary.document_type_raw & 0x7fff {
            113 => Some(Recipe::builtin(GenerationDialect::PctPatent)),
            140 => Some(Recipe::builtin(GenerationDialect::PatentSpecificationKo)),
            // 의견서는 내장 틀이 없고 파일마다 모양 표가 다르다 — 그 파일 틀로 다시 굽는다(본문 규칙은 국내).
            7 => crate::source_generation_profile(&first)
                .ok()
                .map(|profile| Recipe {
                    dialect: GenerationDialect::PatentSpecificationKo,
                    profile: std::sync::Arc::new(profile),
                }),
            _ => None,
        };
        let editable = dialect.is_some();
        Self {
            dir,
            versions: vec![Version {
                path: first.clone(),
                real: None,
                model: None,
                generated: false,
            }],
            current: 0,
            saved: Some(0),
            dialect,
            document,
            plan,
            print: PrintOptions::default(),
            // 서식 구간은 모델이 생기면 모델에서 뽑는다(서식 목록 판독을 따로 하지 않는다).
            spans: Default::default(),
            background: editable.then(|| {
                let path = first.clone();
                std::thread::spawn(move || {
                    let (document, payloads) = hml_document_from_hlt(&path)?;
                    Ok(Model {
                        origin: path,
                        document,
                        payloads: std::sync::Arc::new(payloads),
                    })
                })
            }),
            numbers: opened_numbers.clone(),
            opened_numbers,
            cp949_substitutions: Vec::new(),
        }
        .with_spans()
        .centered_items()
    }

    /// 가운데 정렬 식별항목에 들여쓰기가 적힌 파일(통특기도 정가운데보다 오른쪽에 찍는다)은 열 때 한 번 다시 구워
    /// 정가운데로 보인다 — 생성기가 그 표제를 0 으로 굽는다. 고친 판이라 닫을 때 저장을 묻고, 되돌리면 파일 모양이다.
    fn centered_items(mut self) -> Result<Self, CoreError> {
        let off_center = self.editable()
            && self.document.sections.iter().any(|section| {
                self.document.paragraphs.iter().any(|paragraph| {
                    section.anchor_paragraph_id.as_deref() == Some(paragraph.id.as_str())
                        && paragraph.align == "center"
                        && paragraph.indent_raw != 0
                })
            });
        if off_center {
            self.edit_model(|_| Ok(()))?;
        }
        Ok(self)
    }

    /// 뒤에서 만들던 모델이 다 됐으면 받는다(안 기다린다). 화면이 보기를 새로 받을 때 부른다.
    pub fn poll(&mut self) {
        if self
            .background
            .as_ref()
            .is_some_and(|job| job.is_finished())
        {
            self.settle();
        }
    }

    /// 모델이 다 될 때까지 기다려 받는다(고치기 전에).
    fn settle(&mut self) {
        let Some(job) = self.background.take() else {
            return;
        };
        if let Ok(Ok(mut model)) = job.join()
            && self.versions[0].model.is_none()
        {
            apply_numbers(&mut model.document, &self.opened_numbers);
            self.versions[0].model = Some(model);
            if self.current == 0 {
                self.refresh_spans();
            }
        }
    }

    /// 모델이 아직 없는가(화면이 「준비 중」을 보인다).
    pub fn preparing(&self) -> bool {
        self.background.is_some()
    }

    fn with_spans(mut self) -> Self {
        if self.dialect.is_none() {
            self.spans = load_spans(self.path());
        }
        self
    }

    fn refresh_spans(&mut self) {
        let version = &mut self.versions[self.current];
        if let Some(model) = version.model.as_mut() {
            let count = model.document.paragraphs.len();
            if count <= self.document.paragraphs.len()
                && self.document.paragraphs[count..]
                    .iter()
                    .all(|paragraph| paragraph.line_ids.is_empty())
            {
                normalize_ids(&mut model.document, &self.document);
                self.spans = model_spans(&model.document);
                self.numbers = model
                    .document
                    .paragraphs
                    .iter()
                    .map(|paragraph| paragraph.number.clone())
                    .collect();
                return;
            }
            version.model = None;
        }
        self.spans = load_spans(self.path());
        // 모델이 없는 판: 연 판이면 연 번호 · 아니면 모른다.
        self.numbers = if self.current == 0 {
            self.opened_numbers.clone()
        } else {
            Vec::new()
        };
    }

    pub fn path(&self) -> &Path {
        &self.versions[self.current].path
    }

    /// 다시 굽기로 고칠 수 있는 문서인가(명세서 PCT 113 · 국내 140). 의견서·보정 명세서는 아직 못 고친다.
    pub fn editable(&self) -> bool {
        self.dialect.is_some()
    }

    pub fn paragraphs(&self) -> Vec<EditorParagraph> {
        let tables: std::collections::BTreeSet<&str> = self
            .document
            .tables
            .iter()
            .filter_map(|table| table.anchor_paragraph_id.as_deref())
            .collect();
        self.document
            .paragraphs
            .iter()
            .enumerate()
            .map(|(index, paragraph)| EditorParagraph {
                id: paragraph.id.clone(),
                text: paragraph.text.clone(),
                wipo_id: paragraph.wipo_id,
                number: self.numbers.get(index).cloned().flatten(),
                has_objects: tables.contains(paragraph.id.as_str())
                    || self
                        .document
                        .images
                        .iter()
                        .any(|image| image.paragraph_id == paragraph.id),
                spans: self.spans.get(&paragraph.id).cloned().unwrap_or_default(),
            })
            .collect()
    }

    /// 문단 글 바꾸기. 줄바꿈(`\n`)은 문단 나누기다. 그림 자리표 `_` 수가 줄면 그 그림이 빠진다.
    pub fn replace_text(&mut self, paragraph_id: &str, text: &str) -> Result<(), CoreError> {
        self.replace_text_breaking(paragraph_id, text, false)
    }

    /// 강제 쪽나눔(워드 Ctrl+Enter): [`Self::replace_text`] 와 같되 나뉜 둘째 문단이 새 쪽에서 시작한다.
    pub fn replace_text_with_page_break(
        &mut self,
        paragraph_id: &str,
        text: &str,
    ) -> Result<(), CoreError> {
        if !text.contains('\n') {
            return Err(editor_error("page_break_needs_split"));
        }
        self.replace_text_breaking(paragraph_id, text, true)
    }

    fn replace_text_breaking(
        &mut self,
        paragraph_id: &str,
        text: &str,
        page_break: bool,
    ) -> Result<(), CoreError> {
        self.edit_model(|model| {
            let index = model
                .paragraphs
                .iter()
                .position(|paragraph| paragraph.id == paragraph_id)
                .ok_or_else(|| editor_error("paragraph"))?;
            if model.paragraphs[index].table_id.is_some() {
                return Err(editor_error("table_paragraph"));
            }
            let old = cells(&model.paragraphs[index].inlines);
            let pieces: Vec<&str> = text.split('\n').collect();
            let new: Vec<char> = pieces.join("\u{1}").chars().collect();
            let edited = splice(&old, &new);
            // 문단 나누기: 자리표 `\u{1}` 에서 가른다. 새 문단은 앞 문단 모양을 잇는다.
            let mut parts: Vec<Vec<Cell>> = vec![Vec::new()];
            for cell in edited {
                if cell == Cell::Char('\u{1}', Vec::new(), None)
                    || matches!(&cell, Cell::Char('\u{1}', ..))
                {
                    parts.push(Vec::new());
                } else {
                    parts.last_mut().unwrap().push(cell);
                }
            }
            let template = model.paragraphs[index].clone();
            let mut replacements = Vec::with_capacity(parts.len());
            for (part, cells) in parts.into_iter().enumerate() {
                let mut paragraph = template.clone();
                if part > 0 {
                    paragraph.id = format!("{}:{part}", template.id);
                    paragraph.marker = None;
                    paragraph.page_break = page_break && part == 1;
                    // 나눠서 새로 생긴 문단은 식별번호가 없다(보정서에서 새 문단).
                    paragraph.number = None;
                }
                paragraph.inlines = inlines(&cells);
                // 모델의 문단 글은 그림을 뺀 글자다(생성기가 런과 대조한다).
                paragraph.text = cells
                    .iter()
                    .filter_map(|cell| match cell {
                        Cell::Char(ch, ..) => Some(*ch),
                        Cell::Image(_) => None,
                    })
                    .collect();
                paragraph.image_ids = cells
                    .iter()
                    .filter_map(|cell| match cell {
                        Cell::Image(id) => Some(id.clone()),
                        Cell::Char(..) => None,
                    })
                    .collect();
                paragraph.source_line_bytes.clear();
                replacements.push(paragraph);
            }
            model.paragraphs.splice(index..=index, replacements);
            Ok(())
        })
    }

    /// 문단 정렬 · 들여쓰기(파일 단위, 국문 800 = 4mm)를 한 판으로 바꾼다 — 오프라인 정렬(화학식 · 표 제목)용.
    /// 생성기 모양표에 있는 정렬만 받는다(그 밖은 조용히 양쪽 정렬로 떨어진다). 들여쓰기가 바뀐 문단은 줄을 다시 나눈다.
    pub fn set_paragraph_shapes(
        &mut self,
        shapes: &[(String, String, i32)],
    ) -> Result<(), CoreError> {
        if shapes
            .iter()
            .any(|(_, align, _)| !matches!(align.as_str(), "center" | "left" | "justified"))
        {
            return Err(editor_error("align"));
        }
        self.edit_model(|model| {
            for (paragraph_id, align, indent) in shapes {
                let paragraph = model
                    .paragraphs
                    .iter_mut()
                    .find(|paragraph| &paragraph.id == paragraph_id)
                    .ok_or_else(|| editor_error("paragraph"))?;
                if paragraph.table_id.is_some() {
                    return Err(editor_error("table_paragraph"));
                }
                if paragraph.indent_raw != *indent {
                    paragraph.source_line_bytes.clear();
                }
                paragraph.align = align.clone();
                paragraph.indent_raw = *indent;
            }
            Ok(())
        })
    }

    /// 문단 정렬 하나만 바꾼다(편집기 정렬 단추). 가운데는 들여쓰기 0 — 통특기 가운데 정렬은 들여쓰기 뒤 남는
    /// 폭의 반을 밀어 정가운데에서 벗어난다. 왼쪽 · 양쪽은 들여쓰기를 그대로 둔다.
    /// ponytail: 가운데 → 양쪽으로 돌아와도 들여쓰기는 0 으로 남는다(원래 값을 기억하지 않는다).
    pub fn set_alignment(&mut self, paragraph_id: &str, align: &str) -> Result<(), CoreError> {
        if !matches!(align, "center" | "left" | "justified") {
            return Err(editor_error("align"));
        }
        self.edit_model(|model| {
            let paragraph = model
                .paragraphs
                .iter_mut()
                .find(|paragraph| paragraph.id == paragraph_id)
                .ok_or_else(|| editor_error("paragraph"))?;
            if paragraph.table_id.is_some() {
                return Err(editor_error("table_paragraph"));
            }
            if align == "center" && paragraph.indent_raw != 0 {
                paragraph.indent_raw = 0;
                paragraph.source_line_bytes.clear();
            }
            paragraph.align = align.to_string();
            Ok(())
        })
    }

    /// 문단 첫 줄 들여쓰기(편집기 「들여쓰기」 · 「들여쓰기 취소」 단추 — 통특기처럼 정해진 값으로 켜고 0 으로 끈다).
    pub fn set_indent(&mut self, paragraph_id: &str, indent_raw: i32) -> Result<(), CoreError> {
        if !(0..=4000).contains(&indent_raw) {
            return Err(editor_error("indent"));
        }
        self.edit_model(|model| {
            let anchor = model.sections.iter().any(|section| {
                section.paragraph_ids.first().map(String::as_str) == Some(paragraph_id)
            });
            let paragraph = model
                .paragraphs
                .iter_mut()
                .find(|paragraph| paragraph.id == paragraph_id)
                .ok_or_else(|| editor_error("paragraph"))?;
            if paragraph.table_id.is_some() {
                return Err(editor_error("table_paragraph"));
            }
            // 생성기가 고친 식별항목 표제(절 닻)를 들여쓰기 0 으로 굽는다(`generate_body.rs` is_anchor) — 켜면 저장 때 조용히 사라진다.
            // ponytail: 표제 들여쓰기가 필요해지면 「모양을 아는 표제」를 모델에 실어 생성기 규칙에서 빼라.
            if anchor && paragraph.marker.is_some() && indent_raw != 0 {
                return Err(editor_error("heading_indent"));
            }
            if paragraph.indent_raw != indent_raw {
                paragraph.indent_raw = indent_raw;
                paragraph.source_line_bytes.clear();
            }
            Ok(())
        })
    }

    /// 문단을 앞 문단 끝에 붙인다(쪽 위 편집의 문단 첫머리 Backspace · 끝 Delete). 표 문단 · 표제(절 닻)는 붙이지 않는다.
    pub fn merge_with_previous(&mut self, paragraph_id: &str) -> Result<(), CoreError> {
        self.edit_model(|model| {
            let index = model
                .paragraphs
                .iter()
                .position(|paragraph| paragraph.id == paragraph_id)
                .filter(|index| *index > 0)
                .ok_or_else(|| editor_error("paragraph"))?;
            let (before, after) = model.paragraphs.split_at_mut(index);
            let (previous, current) = (before.last_mut().unwrap(), &after[0]);
            if previous.table_id.is_some() || current.table_id.is_some() {
                return Err(editor_error("table_paragraph"));
            }
            if previous.marker.is_some() || current.marker.is_some() {
                return Err(editor_error("heading_paragraph"));
            }
            previous.inlines.extend(current.inlines.iter().cloned());
            previous.text.push_str(&current.text);
            previous.image_ids.extend(current.image_ids.iter().cloned());
            previous.source_line_bytes.clear();
            let previous_id = previous.id.clone();
            for image in &mut model.images {
                if image.paragraph_id.as_deref() == Some(paragraph_id) {
                    image.paragraph_id = Some(previous_id.clone());
                }
            }
            model.paragraphs.remove(index);
            Ok(())
        })
    }

    /// 글자 서식 켜기/끄기: 문단 글(그림 = `_` 한 칸)의 [start, end) 글자. 구간이 이미 전부 그 서식이면 끈다.
    /// 위첨자와 아래첨자는 서로 끈다.
    pub fn toggle_format(
        &mut self,
        paragraph_id: &str,
        start: usize,
        end: usize,
        format: CharacterFormat,
    ) -> Result<(), CoreError> {
        self.toggle_format_spans(&[(paragraph_id.to_string(), start, end)], format)
    }

    /// 여러 문단에 걸친 고르기의 서식: 문단마다 [start, end). 켤지 끌지는 고른 전체로 정한다(한 고침 · 되돌리기 한 번).
    pub fn toggle_format_spans(
        &mut self,
        spans: &[(String, usize, usize)],
        format: CharacterFormat,
    ) -> Result<(), CoreError> {
        if spans.iter().all(|(_, start, end)| start >= end) {
            return Err(editor_error("empty_range"));
        }
        self.edit_model(|model| {
            // 같은 문단의 구간들은 한 글자 목록에 차례로 건다 — 구간마다 따로 읽어 쓰면 마지막 구간만 남았다
            // (의견서 hlz 가져오기 · 한 문단에 굵게 구간 여럿 · 2026-10-16).
            let mut edited: Vec<(usize, Vec<(usize, usize)>, Vec<Cell>)> = Vec::new();
            for (id, start, end) in spans {
                let index = model
                    .paragraphs
                    .iter()
                    .position(|paragraph| &paragraph.id == id)
                    .ok_or_else(|| editor_error("paragraph"))?;
                if !edited.iter().any(|(at, ..)| *at == index) {
                    edited.push((index, Vec::new(), cells(&model.paragraphs[index].inlines)));
                }
                let entry = edited.iter_mut().find(|(at, ..)| *at == index).unwrap();
                if start > end || *end > entry.2.len() {
                    return Err(editor_error("range"));
                }
                entry.1.push((*start, *end));
            }
            let on = !edited.iter().all(|(_, ranges, cells)| {
                ranges.iter().all(|(start, end)| {
                    cells[*start..*end].iter().all(|cell| match cell {
                        Cell::Char(_, formats, _) => formats.contains(&format),
                        Cell::Image(_) => true,
                    })
                })
            });
            let opposite = match format {
                CharacterFormat::Superscript => Some(CharacterFormat::Subscript),
                CharacterFormat::Subscript => Some(CharacterFormat::Superscript),
                _ => None,
            };
            for (index, ranges, mut cells) in edited {
                let picked: std::collections::BTreeSet<usize> = ranges
                    .iter()
                    .flat_map(|(start, end)| *start..*end)
                    .collect();
                for at in picked {
                    if let Cell::Char(_, formats, _) = &mut cells[at] {
                        formats.retain(|value| *value != format);
                        if on {
                            formats.retain(|value| Some(*value) != opposite);
                            formats.push(format);
                        }
                    }
                }
                let paragraph = &mut model.paragraphs[index];
                paragraph.inlines = inlines(&cells);
                // 첨자는 줄 폭을 바꾼다 — 원본 줄나눔을 버리고 다시 흘린다.
                paragraph.source_line_bytes.clear();
            }
            Ok(())
        })
    }

    /// 여러 문단에 걸친 고르기 지우기: `from` 문단의 [from_offset..] 와 `to` 문단의 [..to_offset] 와 그 사이 문단을 지우고
    /// 두 문단을 하나로 잇는다(한 고침 · 되돌리기 한 번). 표 문단 · 표제(절 닻)가 걸리면 문단 붙이기처럼 거부한다.
    pub fn delete_range(
        &mut self,
        from_id: &str,
        from_offset: usize,
        to_id: &str,
        to_offset: usize,
    ) -> Result<(), CoreError> {
        self.edit_model(|model| {
            let position = |id: &str| {
                model
                    .paragraphs
                    .iter()
                    .position(|paragraph| paragraph.id == id)
                    .ok_or_else(|| editor_error("paragraph"))
            };
            let (first, last) = (position(from_id)?, position(to_id)?);
            if first >= last {
                return Err(editor_error("range"));
            }
            let span = &model.paragraphs[first..=last];
            if span.iter().any(|paragraph| paragraph.table_id.is_some()) {
                return Err(editor_error("table_paragraph"));
            }
            if span.iter().any(|paragraph| paragraph.marker.is_some()) {
                return Err(editor_error("heading_paragraph"));
            }
            let head = cells(&model.paragraphs[first].inlines);
            let tail = cells(&model.paragraphs[last].inlines);
            if from_offset > head.len() || to_offset > tail.len() {
                return Err(editor_error("range"));
            }
            let joined: Vec<Cell> = head[..from_offset]
                .iter()
                .chain(&tail[to_offset..])
                .cloned()
                .collect();
            let keep_id = model.paragraphs[first].id.clone();
            let paragraph = &mut model.paragraphs[first];
            paragraph.inlines = inlines(&joined);
            paragraph.text = joined
                .iter()
                .filter_map(|cell| match cell {
                    Cell::Char(ch, ..) => Some(*ch),
                    Cell::Image(_) => None,
                })
                .collect();
            paragraph.image_ids = joined
                .iter()
                .filter_map(|cell| match cell {
                    Cell::Image(id) => Some(id.clone()),
                    Cell::Char(..) => None,
                })
                .collect();
            paragraph.source_line_bytes.clear();
            // 남은 그림은 첫 문단으로 옮긴다 · 지운 그림은 renumber 가 뺀다(어디서도 안 가리킨다).
            let kept: std::collections::BTreeSet<String> =
                paragraph.image_ids.iter().cloned().collect();
            for image in &mut model.images {
                if kept.contains(&image.id) {
                    image.paragraph_id = Some(keep_id.clone());
                }
            }
            model.paragraphs.drain(first + 1..=last);
            Ok(())
        })
    }

    /// 글자 크기(pt) — 문단 글 [start, end). 본문 기본은 12pt(KIPO 명세서 서식) · 범위 6~30pt.
    pub fn set_size(
        &mut self,
        paragraph_id: &str,
        start: usize,
        end: usize,
        size: u32,
    ) -> Result<(), CoreError> {
        if start >= end || !(6..=30).contains(&size) {
            return Err(editor_error("size_range"));
        }
        self.edit_model(|model| {
            let paragraph = model
                .paragraphs
                .iter_mut()
                .find(|paragraph| paragraph.id == paragraph_id)
                .ok_or_else(|| editor_error("paragraph"))?;
            let mut cells = cells(&paragraph.inlines);
            if end > cells.len() {
                return Err(editor_error("range"));
            }
            for cell in &mut cells[start..end] {
                if let Cell::Char(_, _, value) = cell {
                    // 12pt 는 방언 기본값이라 명시하지 않는다(원본 런과 같은 꼴).
                    *value = (size != 12).then_some(size);
                }
            }
            paragraph.inlines = inlines(&cells);
            paragraph.source_line_bytes.clear();
            Ok(())
        })
    }

    /// 표 칸 글 바꾸기(서식·그림 보존 규칙은 문단과 같다). 줄바꿈은 안 된다.
    pub fn replace_cell_text(&mut self, cell_id: &str, text: &str) -> Result<(), CoreError> {
        if text.contains('\n') {
            return Err(editor_error("cell_multiline"));
        }
        self.edit_model(|model| {
            let cell = model
                .cells
                .iter_mut()
                .find(|cell| cell.id == cell_id)
                .ok_or_else(|| editor_error("cell"))?;
            let edited = splice(&cells(&cell.inlines), &text.chars().collect::<Vec<_>>());
            set_cell(cell, &edited);
            Ok(())
        })
    }

    /// 표 칸 글(그림 = `_`) — 편집 창이 보여 준다.
    pub fn cell_text(&self, cell_id: &str) -> Option<String> {
        let parsed;
        let model = match &self.versions[self.current].model {
            Some(model) => &model.document,
            None => {
                parsed = hml_document_from_hlt(self.path()).ok()?.0;
                &parsed
            }
        };
        let cell = model.cells.iter().find(|cell| cell.id == cell_id)?;
        Some(
            cells(&cell.inlines)
                .iter()
                .map(|value| match value {
                    Cell::Char(ch, ..) => *ch,
                    Cell::Image(_) => '_',
                })
                .collect(),
        )
    }

    /// 표 칸 글 전부(그림 = `_`) — 쪽 위 칸 편집이 쓴다. 편집 모델이 아직 없으면(열자마자 · 준비 중) 비어 있다
    /// (그때 화면은 칸 하나씩 `cell_text` 로 묻는다 — 큰 문서를 화면마다 다시 판독하지 않게).
    pub fn cell_texts(&self) -> std::collections::BTreeMap<String, String> {
        let Some(model) = &self.versions[self.current].model else {
            return Default::default();
        };
        model
            .document
            .cells
            .iter()
            .filter(|cell| !cell.merge_continuation)
            .map(|cell| {
                let text = cells(&cell.inlines)
                    .iter()
                    .map(|value| match value {
                        Cell::Char(ch, ..) => *ch,
                        Cell::Image(_) => '_',
                    })
                    .collect();
                (cell.id.clone(), text)
            })
            .collect()
    }

    /// 찾기 — 문단 글(그림 = `_`)에서 글·서식 조건이 맞는 구간들(문서 차례).
    pub fn find(&mut self, query: &FindQuery) -> Vec<FindMatch> {
        // 서식 조건은 모델의 서식 구간을 본다 — 뒤에서 만들던 모델을 기다린다.
        self.settle();
        let needle: Vec<char> = query.text.chars().collect();
        let mut out = Vec::new();
        for paragraph in self.paragraphs() {
            let chars: Vec<char> = paragraph.text.chars().collect();
            let mut style = vec![0_u32; chars.len()];
            for span in &paragraph.spans {
                for at in span.start as usize..(span.end as usize).min(chars.len()) {
                    style[at] |= span.style;
                }
            }
            let fits = |at: usize| match query.format {
                None => true,
                Some(FindFormat::Plain) => style[at] == 0,
                Some(format) => style[at] & find_bit(format) != 0,
            };
            let mut push = |start: usize, end: usize| {
                let from = start.saturating_sub(12);
                let to = (end + 12).min(chars.len());
                out.push(FindMatch {
                    paragraph_id: paragraph.id.clone(),
                    start,
                    end,
                    context: chars[from..to].iter().collect(),
                });
            };
            if needle.is_empty() {
                // 서식만: 그 서식이 이어진 구간 하나하나(그림 자리 제외).
                if query.format.is_none() {
                    continue;
                }
                let mut at = 0;
                while at < chars.len() {
                    if fits(at) && chars[at] != '_' {
                        let start = at;
                        while at < chars.len() && fits(at) && chars[at] != '_' {
                            at += 1;
                        }
                        push(start, at);
                    } else {
                        at += 1;
                    }
                }
                continue;
            }
            let mut at = 0;
            while at + needle.len() <= chars.len() {
                if chars[at..at + needle.len()] == needle[..] && (at..at + needle.len()).all(fits) {
                    push(at, at + needle.len());
                    at += needle.len();
                } else {
                    at += 1;
                }
            }
        }
        out
    }

    /// 바꾸기 — 찾은 구간들을 새 글로(서식은 `format` 대로). 한 번 다시 굽는다(되돌리기 한 번).
    /// 구간은 지금 판의 `find` 결과여야 한다(같은 문단 안 구간은 뒤에서부터 바꾼다).
    /// 새 글이 비었는데 서식을 바꾸라면 **서식만 바꾼다**(글은 그대로 · 글자마다 제 서식에서 고친다) —
    /// 전엔 찾은 글을 지웠다(2026-10-16 사용자: 아래첨자만 찾아 위첨자로 바꾸자 글자가 사라졌다).
    pub fn replace(
        &mut self,
        matches: &[FindMatch],
        replacement: &str,
        format: ReplaceFormat,
    ) -> Result<usize, CoreError> {
        if matches.is_empty() {
            return Ok(0);
        }
        let new: Vec<char> = replacement.chars().collect();
        if new.contains(&'\n') {
            return Err(editor_error("replace_multiline"));
        }
        let mut by_paragraph: std::collections::BTreeMap<&str, Vec<&FindMatch>> =
            Default::default();
        for found in matches {
            by_paragraph
                .entry(found.paragraph_id.as_str())
                .or_default()
                .push(found);
        }
        self.edit_model(|model| {
            for (id, mut found) in by_paragraph {
                let paragraph = model
                    .paragraphs
                    .iter_mut()
                    .find(|paragraph| paragraph.id == id)
                    .ok_or_else(|| editor_error("paragraph"))?;
                let mut cells = cells(&paragraph.inlines);
                found.sort_by_key(|value| std::cmp::Reverse(value.start));
                for found in found {
                    if found.start >= found.end || found.end > cells.len() {
                        return Err(editor_error("range"));
                    }
                    if cells[found.start..found.end]
                        .iter()
                        .any(|cell| matches!(cell, Cell::Image(_)))
                    {
                        return Err(editor_error("replace_image"));
                    }
                    if new.is_empty() && format != ReplaceFormat::Keep {
                        for cell in &mut cells[found.start..found.end] {
                            if let Cell::Char(_, formats, _) = cell {
                                reformat(formats, format);
                            }
                        }
                        continue;
                    }
                    let (mut formats, size) = match &cells[found.start] {
                        Cell::Char(_, formats, size) => (formats.clone(), *size),
                        Cell::Image(_) => unreachable!(),
                    };
                    reformat(&mut formats, format);
                    let inserted = new.iter().map(|ch| Cell::Char(*ch, formats.clone(), size));
                    cells.splice(found.start..found.end, inserted);
                }
                paragraph.inlines = inlines(&cells);
                paragraph.text = cells
                    .iter()
                    .filter_map(|cell| match cell {
                        Cell::Char(ch, ..) => Some(*ch),
                        Cell::Image(_) => None,
                    })
                    .collect();
                paragraph.source_line_bytes.clear();
            }
            Ok(())
        })?;
        Ok(matches.len())
    }

    /// 식별항목 넣기(통특기 우클릭 「식별항목」): `at` 문단 뒤(`before` 면 앞)에 「【종류 번호】」 표제와 빈 본문 문단을 넣는다.
    /// 종류 = 통특기 메뉴 7종 · 번호 = 영문 · 숫자(통특기: 「일련번호는 영문과 숫자만 허용」) · 뒤 번호는 밀지 않는다.
    /// 돌려주는 값 = 빈 본문 문단 id(커서를 둔다 · 다시 구우면 문단 id 가 차례로 다시 매겨지니 넣은 뒤 차례로 읽는다).
    pub fn insert_item(
        &mut self,
        at: &str,
        before: bool,
        kind: &str,
        number: &str,
    ) -> Result<String, CoreError> {
        if !is_item_heading(&format!("【{kind} {number}】")) {
            return Err(editor_error("item"));
        }
        let mut body_at = 0;
        self.edit_model(|model| {
            let index = model
                .paragraphs
                .iter()
                .position(|paragraph| paragraph.id == at)
                .ok_or_else(|| editor_error("paragraph"))?;
            let fresh = |suffix: &str| {
                (0..)
                    .map(|n| format!("{at}:item{n}{suffix}"))
                    .find(|id| model.paragraphs.iter().all(|paragraph| &paragraph.id != id))
                    .expect("unbounded")
            };
            let (heading_id, body_id) = (fresh("h"), fresh("b"));
            // 모양: 본문 = 가까운 본문 문단(커서가 표제 위여도 본문 모양) · 표제 = 같은 종류 표제가 있으면 그것, 없으면 가까운 앞 표제
            // (본문의 첫 줄 들여쓰기를 표제가 물려받지 않게).
            // 표제 = 【…】 한 줄(원본 HLT 에서 읽은 표제는 marker 가 없다 — 글로 알아본다).
            let is_heading = |paragraph: &crate::HmlParagraph| {
                paragraph.marker.is_some() || {
                    let text = paragraph.text.trim();
                    text.starts_with('【')
                        && text.ends_with('】')
                        && text.matches('】').count() == 1
                }
            };
            let is_body = |paragraph: &crate::HmlParagraph| {
                !is_heading(paragraph) && paragraph.table_id.is_none()
            };
            let body_shape = model.paragraphs[..=index]
                .iter()
                .rev()
                .chain(&model.paragraphs[index + 1..])
                .find(|paragraph| is_body(paragraph))
                .unwrap_or(&model.paragraphs[index])
                .clone();
            let prefix = format!("【{kind} ");
            let heading_shape = model
                .paragraphs
                .iter()
                .find(|paragraph| {
                    is_heading(paragraph) && paragraph.text.trim().starts_with(&prefix)
                })
                .or_else(|| {
                    model.paragraphs[..=index]
                        .iter()
                        .rev()
                        .find(|paragraph| is_heading(paragraph))
                })
                .unwrap_or(&body_shape)
                .clone();
            let blank = |shape: crate::HmlParagraph| crate::HmlParagraph {
                marker: None,
                page_break: false,
                table_id: None,
                image_ids: Vec::new(),
                inlines: Vec::new(),
                source_line_bytes: Vec::new(),
                text: String::new(),
                tail_text: String::new(),
                // 절 소속은 넣는 자리를 따른다(renumber 가 다시 맞춘다).
                section_id: model.paragraphs[index].section_id.clone(),
                number: None,
                ..shape
            };
            // 식별항목 표제는 들여쓰지 않는다(생성기는 절 닻 표제만 들여쓰기 0 으로 맞춘다 — 새 표제는 닻이 아니라 값을 직접 준다).
            let heading_base = crate::HmlParagraph {
                indent_raw: 0,
                align: "left".into(),
                ..blank(heading_shape)
            };
            let base = blank(body_shape);
            let text = format!("【{kind} {number}】");
            let heading = crate::HmlParagraph {
                id: heading_id,
                marker: Some(text.clone()),
                text: text.clone(),
                inlines: vec![HmlInline::Text {
                    text,
                    formats: Vec::new(),
                    size: None,
                }],
                ..heading_base
            };
            let body = crate::HmlParagraph {
                id: body_id,
                ..base
            };
            let at = if before { index } else { index + 1 };
            model.paragraphs.splice(at..at, [heading, body]);
            body_at = at + 1;
            Ok(())
        })?;
        self.paragraphs()
            .get(body_at)
            .map(|paragraph| paragraph.id.clone())
            .ok_or_else(|| editor_error("paragraph"))
    }

    /// 단락삭제(통특기 「단락삭제」 대화상자): 종류 하나 · 번호 여럿을 한 번의 고침으로 지운다(되돌리기 한 번).
    /// 【청구항 N】은 표제와 내용 문단(다음 표제 앞까지)을 다 지운다(통특기와 같다).
    /// 다른 종류는 표제(+ 넣을 때 생긴 빈 문단)만. 번호는 다시 매기지 않는다(통특기도 4 는 4).
    pub fn remove_items(&mut self, kind: &str, numbers: &[String]) -> Result<(), CoreError> {
        if numbers.is_empty() {
            return Err(editor_error("no_item"));
        }
        let headings: Vec<String> = numbers
            .iter()
            .map(|number| format!("【{kind} {}】", number.trim()))
            .collect();
        if !headings.iter().all(|heading| is_item_heading(heading)) {
            return Err(editor_error("not_item"));
        }
        let with_content = kind == "청구항";
        self.edit_model(|model| {
            for heading in &headings {
                let index = model
                    .paragraphs
                    .iter()
                    .position(|paragraph| paragraph.text.trim() == heading)
                    .ok_or_else(|| editor_error("item_missing"))?;
                let end = if with_content {
                    index
                        + 1
                        + model.paragraphs[index + 1..]
                            .iter()
                            .take_while(|paragraph| !is_bracket_heading(&paragraph.text))
                            .count()
                } else {
                    // 넣을 때 같이 생긴 빈 문단도.
                    let blank_after = model.paragraphs.get(index + 1).is_some_and(|next| {
                        next.text.is_empty()
                            && next.image_ids.is_empty()
                            && next.table_id.is_none()
                            && !is_item_heading(&next.text)
                    });
                    index + 1 + usize::from(blank_after)
                };
                if model.paragraphs[index..end]
                    .iter()
                    .any(|paragraph| paragraph.table_id.is_some())
                {
                    return Err(editor_error("table_paragraph"));
                }
                model.paragraphs.drain(index..end);
            }
            Ok(())
        })
    }

    /// 수식·화학식 넣기: `after` 문단 뒤에 「【수학식 N】」(또는 화학식) 표제 + 그림 문단을 넣는다.
    /// 그림 = PNG(화면의 수식·구조식 편집기가 그린 것) · 표시 크기 pt. 번호 N = 앞에 있는 같은 표제 수 + 1 ·
    /// 뒤 표제는 하나씩 민다(본문 안 「수학식 3」 같은 참조는 안 고친다).
    pub fn insert_figure(
        &mut self,
        after: &str,
        kind: FigureKind,
        png: Vec<u8>,
        width_pt: f64,
        height_pt: f64,
    ) -> Result<(), CoreError> {
        image::load_from_memory_with_format(&png, image::ImageFormat::Png)
            .map_err(|_| editor_error("figure_png"))?;
        self.place_figure(after, Some(kind.label()), png, width_pt, height_pt)
    }

    /// 그림 크기 바꾸기(비율 잠금): 판독 그림 `image_id` 의 상자 · 표시 크기를 모두 `scale` 배로.
    /// 쪽 본문(가로 174 · 세로 240mm)보다 커지지 않게 배율을 깎는다. 판독 그림 차례 = 모델 그림 차례(`renumber` 가 닻 차례로 매긴다).
    pub fn resize_image(&mut self, image_id: &str, scale: f64) -> Result<(), CoreError> {
        if !(scale.is_finite() && scale > 0.0) {
            return Err(editor_error("image_scale"));
        }
        let index = self
            .document
            .images
            .iter()
            .position(|image| image.id == image_id)
            .ok_or_else(|| editor_error("image"))?;
        let count = self.document.images.len();
        // 표 칸 안 그림은 칸 폭(pt = 칸 폭 / 20 · 인쇄가 칸 상자를 그리는 값)보다 넓어지지 않는다.
        let read = &self.document.images[index];
        let cell_fit = read
            .cell_id
            .as_deref()
            .and_then(|id| self.document.cells.iter().find(|cell| cell.id == id))
            .filter(|_| read.display_width > 0)
            .map_or(f64::INFINITY, |cell| {
                f64::from(cell.width) / 20.0 / f64::from(read.display_width)
            });
        self.edit_model(|model| {
            if model.images.len() != count {
                return Err(editor_error("image"));
            }
            let image = &mut model.images[index];
            let (width, height) = (image.width.unwrap_or(1), image.height.unwrap_or(1));
            // 상자 단위 = 0.282mm(`docx.rs` 의 표시 단위와 같다).
            let fit = (174.0 / (f64::from(width) * 0.282)).min(240.0 / (f64::from(height) * 0.282));
            let scale = scale.min(fit).min(cell_fit);
            let scaled = |value: Option<u32>| {
                value.map(|value| ((f64::from(value) * scale).round() as u32).max(1))
            };
            image.width = scaled(image.width);
            image.height = scaled(image.height);
            image.secondary_width = scaled(image.secondary_width);
            image.secondary_height = scaled(image.secondary_height);
            Ok(())
        })
    }

    /// 그림 파일 넣기(JPG · TIFF · PNG …): `after` 문단 뒤에 그림 문단 하나(표제 없음).
    /// 크기 = 96dpi 화소 → pt, 쪽보다 크면 비율 그대로 줄인다.
    pub fn insert_picture(&mut self, after: &str, bytes: &[u8]) -> Result<(), CoreError> {
        let picture = image::load_from_memory(bytes).map_err(|_| editor_error("figure_image"))?;
        let (width, height) = (
            picture.width() as f64 * 0.75,
            picture.height() as f64 * 0.75,
        );
        // ponytail: 쪽 글 상자 = 가로 450 · 세로 650pt 로 어림 — 용지 · 여백을 읽게 되면 그 값으로.
        let scale = (450.0 / width).min(650.0 / height).min(1.0);
        let mut png = Vec::new();
        picture
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .map_err(|_| editor_error("figure_image"))?;
        self.place_figure(
            after,
            None,
            png,
            (width * scale).max(1.0),
            (height * scale).max(1.0),
        )
    }

    /// 표 칸 안 그림 넣기: 칸 글의 `offset`(그림 = `_` 한 글자 · 화면 커서와 같은 셈) 자리에.
    /// 크기 = 96dpi 화소 → pt, 칸 폭 · 세로 650pt 보다 크면 비율 그대로 줄인다.
    /// 칸 `inlines` · `image_ids` 에 들고, 그림의 문단 = 표 닻 문단(판독 관례).
    pub fn insert_picture_in_cell(
        &mut self,
        cell_id: &str,
        offset: usize,
        bytes: &[u8],
    ) -> Result<(), CoreError> {
        let picture = image::load_from_memory(bytes).map_err(|_| editor_error("figure_image"))?;
        let cell_pt = self
            .document
            .cells
            .iter()
            .find(|cell| cell.id == cell_id)
            .map(|cell| f64::from(cell.width) / 20.0)
            .ok_or_else(|| editor_error("cell"))?;
        let (width, height) = (
            picture.width() as f64 * 0.75,
            picture.height() as f64 * 0.75,
        );
        // ponytail: 칸 안 여백을 4pt 로 어림 — 칸 여백을 읽게 되면 그 값으로.
        let scale = ((cell_pt - 4.0).max(1.0) / width)
            .min(650.0 / height)
            .min(1.0);
        let (width_pt, height_pt) = ((width * scale).max(1.0), (height * scale).max(1.0));
        let mut png = Vec::new();
        picture
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .map_err(|_| editor_error("figure_image"))?;
        self.edit_model_with(|model, payloads| {
            let image_id = (model.images.len()..)
                .map(|n| format!("image:new:{n}"))
                .find(|id| model.images.iter().all(|image| &image.id != id))
                .expect("unbounded");
            let cell = model
                .cells
                .iter_mut()
                .find(|cell| cell.id == cell_id)
                .ok_or_else(|| editor_error("cell"))?;
            let mut edited = cells(&cell.inlines);
            edited.insert(offset.min(edited.len()), Cell::Image(image_id.clone()));
            set_cell(cell, &edited);
            let table_id = cell.table_id.clone();
            let table = model
                .tables
                .iter()
                .find(|table| table.id == table_id)
                .ok_or_else(|| editor_error("cell"))?;
            let anchor = table.paragraph_id.clone();
            model.images.push(crate::HmlImage {
                id: image_id.clone(),
                occurrence: 0,
                name: format!("{image_id}.png"),
                width: Some((width_pt * 96.0 / 72.0).round() as u32),
                height: Some((height_pt * 96.0 / 72.0).round() as u32),
                secondary_width: Some(width_pt.round() as u32),
                secondary_height: Some(height_pt.round() as u32),
                paragraph_id: Some(anchor),
                cell_id: Some(cell_id.to_string()),
            });
            payloads.push(GeneratedImagePayload {
                image_id,
                bytes: png,
            });
            Ok(())
        })
    }

    /// 그림 문단 넣기 · `label` 이 있으면 그 표제를 앞에 달고 뒤 같은 표제 번호를 민다.
    fn place_figure(
        &mut self,
        after: &str,
        label: Option<&str>,
        png: Vec<u8>,
        width_pt: f64,
        height_pt: f64,
    ) -> Result<(), CoreError> {
        if !(1.0..=500.0).contains(&width_pt) || !(1.0..=700.0).contains(&height_pt) {
            return Err(editor_error("figure_size"));
        }
        self.edit_model_with(|model, payloads| {
            let index = model
                .paragraphs
                .iter()
                .position(|paragraph| paragraph.id == after)
                .ok_or_else(|| editor_error("paragraph"))?;
            // 표제 없는 그림은 뒤 번호를 밀지 않는다.
            let prefix = format!("【{} ", label.unwrap_or_default());
            let number_of = |text: &str| -> Option<u32> {
                label?;
                text.trim()
                    .strip_prefix(prefix.as_str())?
                    .strip_suffix('】')?
                    .parse()
                    .ok()
            };
            let number = model.paragraphs[..=index]
                .iter()
                .filter(|paragraph| number_of(&paragraph.text).is_some())
                .count() as u32
                + 1;
            for paragraph in &mut model.paragraphs[index + 1..] {
                if let Some(old) = number_of(&paragraph.text) {
                    let text = format!("{prefix}{}】", old + 1);
                    paragraph.marker = Some(text.clone());
                    paragraph.inlines = vec![HmlInline::Text {
                        text: text.clone(),
                        formats: Vec::new(),
                        size: None,
                    }];
                    paragraph.text = text;
                    paragraph.source_line_bytes.clear();
                }
            }
            let image_id = (model.images.len()..)
                .map(|n| format!("image:new:{n}"))
                .find(|id| model.images.iter().all(|image| &image.id != id))
                .expect("unbounded");
            let template = model.paragraphs[index].clone();
            // 앞 문단의 첫 줄 들여쓰기를 물려받지 않는다(표제 · 그림이 오른쪽으로 밀려 들어갔다).
            let base = crate::HmlParagraph {
                indent_raw: 0,
                marker: None,
                page_break: false,
                table_id: None,
                image_ids: Vec::new(),
                inlines: Vec::new(),
                source_line_bytes: Vec::new(),
                text: String::new(),
                tail_text: String::new(),
                number: None,
                ..template
            };
            let heading = label.map(|label| {
                let heading_text = format!("【{label} {number}】");
                crate::HmlParagraph {
                    id: format!("{after}:{}h", label),
                    marker: Some(heading_text.clone()),
                    text: heading_text.clone(),
                    inlines: vec![HmlInline::Text {
                        text: heading_text,
                        formats: Vec::new(),
                        size: None,
                    }],
                    // 식별항목 넣기(`insert_item`)의 표제와 같은 모양.
                    align: "left".into(),
                    ..base.clone()
                }
            });
            let body = crate::HmlParagraph {
                id: match label {
                    Some(label) => format!("{after}:{label}i"),
                    None => format!("{after}:{image_id}"),
                },
                image_ids: vec![image_id.clone()],
                inlines: vec![HmlInline::Image {
                    image_id: image_id.clone(),
                }],
                ..base
            };
            model.images.push(crate::HmlImage {
                id: image_id.clone(),
                // 새 그림은 앞 그림들 뒤 차례를 받아야 한다 — 넣는 자리 앞의 그림 수로 정하고 뒤 그림을 민다.
                occurrence: 0,
                name: format!("{image_id}.png"),
                // 상자 = 96dpi 화소 · 표시 = pt(통특기 객체 레코드 관례).
                width: Some((width_pt * 96.0 / 72.0).round() as u32),
                height: Some((height_pt * 96.0 / 72.0).round() as u32),
                secondary_width: Some(width_pt.round() as u32),
                secondary_height: Some(height_pt.round() as u32),
                paragraph_id: Some(body.id.clone()),
                cell_id: None,
            });
            payloads.push(GeneratedImagePayload {
                image_id,
                bytes: png,
            });
            model
                .paragraphs
                .splice(index + 1..index + 1, heading.into_iter().chain([body]));
            Ok(())
        })
    }

    /// 표 그리기: `after` 문단 뒤에 빈 문단을 넣고 그 자리를 `rows`×`columns` 새 표로(열 폭 = 통특기 새 표 기본).
    /// 돌려줌 = 새 표 첫 칸 id(화면이 그 칸을 연다).
    pub fn insert_table(
        &mut self,
        after: &str,
        rows: u32,
        columns: u32,
    ) -> Result<String, CoreError> {
        let mut slot = None;
        self.edit_model(|model| {
            let index = model
                .paragraphs
                .iter()
                .position(|paragraph| paragraph.id == after)
                .ok_or_else(|| editor_error("paragraph"))?;
            let id = (0..)
                .map(|n| format!("{after}:table{n}"))
                .find(|id| model.paragraphs.iter().all(|paragraph| &paragraph.id != id))
                .expect("unbounded");
            let template = model.paragraphs[index].clone();
            model.paragraphs.insert(
                index + 1,
                crate::HmlParagraph {
                    id: id.clone(),
                    marker: None,
                    page_break: false,
                    table_id: None,
                    image_ids: Vec::new(),
                    inlines: Vec::new(),
                    source_line_bytes: Vec::new(),
                    text: String::new(),
                    tail_text: String::new(),
                    number: None,
                    indent_raw: 0,
                    ..template
                },
            );
            let decision = table_decision(crate::apply_table_edits(
                model,
                &[crate::TableOperation::NewTable {
                    paragraph_id: id,
                    rows,
                    columns,
                }],
            )?)?;
            // 표 목록 자리(문서 차례) — 문단 id 는 고친 뒤 다시 매겨지므로 자리로 찾는다.
            slot = model
                .tables
                .iter()
                .position(|table| Some(&table.id) == decision.table_id.as_ref());
            Ok(())
        })?;
        // 칸 id 는 고친 뒤 판독 id 로 맞춰진다(`align_cell_ids`) — 맞춘 모델에서 첫 칸을 읽는다.
        self.versions[self.current]
            .model
            .as_ref()
            .and_then(|model| model.document.tables.get(slot?))
            .and_then(|table| table.cell_ids.first().cloned())
            .ok_or_else(|| editor_error("table"))
    }

    /// 표 편집(줄 · 칸 넣기/지우기 · 합치기 · 나누기) 하나 — 되돌리기 한 번.
    /// 엔진이 건너뛰면(마지막 줄 · 그림 든 칸 …) 고치지 않고 `table_skipped:<이유>` 로 알린다.
    pub fn table_edit(
        &mut self,
        operation: crate::TableOperation,
    ) -> Result<crate::TableEditDecision, CoreError> {
        let mut done = None;
        self.edit_model(|model| {
            done = Some(table_decision(crate::apply_table_edits(
                model,
                &[operation],
            )?)?);
            Ok(())
        })?;
        Ok(done.expect("edit ran"))
    }

    /// 표 지우기: 표와 칸을 없애고 그 문단은 표 뒤 글만 남긴 보통 문단으로. 그림 든 칸이 있으면 거부.
    pub fn delete_table(&mut self, table_id: &str) -> Result<(), CoreError> {
        self.edit_model(|model| {
            if model
                .cells
                .iter()
                .any(|cell| cell.table_id == table_id && !cell.image_ids.is_empty())
            {
                return Err(editor_error("table_skipped:cell_contains_image"));
            }
            let table = model
                .tables
                .iter()
                .position(|table| table.id == table_id)
                .ok_or_else(|| editor_error("table"))?;
            model.tables.remove(table);
            model.cells.retain(|cell| cell.table_id != table_id);
            let paragraph = model
                .paragraphs
                .iter_mut()
                .find(|paragraph| paragraph.table_id.as_deref() == Some(table_id))
                .ok_or_else(|| editor_error("paragraph"))?;
            // ponytail: 표 뒤 글의 글자 서식은 버린다(tail 은 글만 실린다) — 서식이 필요해지면 tail inlines 를 모델에 싣는다.
            let tail = std::mem::take(&mut paragraph.tail_text);
            paragraph.table_id = None;
            paragraph.inlines = if tail.is_empty() {
                Vec::new()
            } else {
                vec![HmlInline::Text {
                    text: tail.clone(),
                    formats: Vec::new(),
                    size: None,
                }]
            };
            paragraph.text = tail;
            paragraph.source_line_bytes.clear();
            Ok(())
        })
    }

    /// 칸의 표 격자 자리(줄 · 칸 · 병합 크기 · 표 크기).
    pub fn cell_position(&self, cell_id: &str) -> Option<crate::CellPosition> {
        let parsed;
        let model = match &self.versions[self.current].model {
            Some(model) => &model.document,
            None => {
                parsed = hml_document_from_hlt(self.path()).ok()?.0;
                &parsed
            }
        };
        crate::cell_position(model, cell_id).ok().flatten()
    }

    /// 문단번호 넣기.
    pub fn number_paragraphs(&mut self) -> Result<(), CoreError> {
        let real = self.materialize()?;
        let next = self.next_path();
        number_paragraphs_as_new(&real, &next, WriteOptions::default())?;
        self.push(next)
    }

    /// 지금 판의 진짜 파일(그림 본문까지). 다시 구운 판이면 이때 굽는다(pct_b 수십 초 — 저장·제출·인쇄·문단번호만 부른다).
    pub fn materialize(&mut self) -> Result<PathBuf, CoreError> {
        let index = self.current;
        let version = &self.versions[index];
        if !version.generated {
            return Ok(version.path.clone());
        }
        if let Some(real) = version.real.as_ref().filter(|real| real.exists()) {
            return Ok(real.clone());
        }
        let (Some(model), Some(dialect)) = (&version.model, &self.dialect) else {
            return Err(editor_error("version_missing"));
        };
        let real = self.fresh("HLT");
        write_fast(&model.document, dialect, &model.payloads, &real)?;
        self.versions[index].real = Some(real.clone());
        Ok(real)
    }

    pub fn undo(&mut self) -> Result<bool, CoreError> {
        if self.current == 0 {
            return Ok(false);
        }
        self.drop_superseded_file();
        self.current -= 1;
        self.ensure_file()?;
        self.reload()?;
        Ok(true)
    }

    pub fn redo(&mut self) -> Result<bool, CoreError> {
        if self.current + 1 >= self.versions.len() {
            return Ok(false);
        }
        self.drop_superseded_file();
        self.current += 1;
        self.ensure_file()?;
        self.reload()?;
        Ok(true)
    }

    /// 떠나는 판의 파일을 지운다 — 모델을 들고 있는 다시 구운 판만(돌아오면 모델로 다시 굽는다).
    /// 판마다 파일(pct_a 27MB)이 작업 폴더에 쌓이지 않게.
    fn drop_superseded_file(&mut self) {
        let version = &mut self.versions[self.current];
        if version.generated && version.model.is_some() {
            std::fs::remove_file(&version.path).ok();
            if let Some(real) = version.real.take() {
                std::fs::remove_file(real).ok();
            }
        }
    }

    /// 지금 판의 파일이 없으면(지운 판으로 돌아왔다) 모델로 다시 굽는다.
    fn ensure_file(&mut self) -> Result<(), CoreError> {
        let version = &self.versions[self.current];
        if version.path.exists() {
            return Ok(());
        }
        let (Some(model), Some(dialect)) = (&version.model, &self.dialect) else {
            return Err(editor_error("version_missing"));
        };
        write_light(&model.document, dialect, &model.payloads, &version.path)
    }

    pub fn set_print_options(&mut self, print: PrintOptions) -> Result<(), CoreError> {
        self.print = print;
        self.reload()
    }

    /// 다른 이름으로 저장 — 있는 파일은 덮지 않는다. 다시 구운 판은 검증하는 저장 경로로 한 번 더 굽는다
    /// (편집 중 판은 빠르게 쓰느라 되읽기 검증을 건너뛴다).
    pub fn save_as(&mut self, output: &Path) -> Result<(), CoreError> {
        if output.exists() {
            return Err(CoreError::new(ErrorCode::OutputExists, "save_as", false));
        }
        let version = &self.versions[self.current];
        match (&version.model, &self.dialect) {
            (Some(model), Some(recipe)) if version.generated => {
                crate::save_generated_hlt_with_profile_as_new(
                    &model.origin,
                    &model.document,
                    recipe.dialect,
                    &recipe.profile,
                    &model.payloads,
                    output,
                    WriteOptions::default(),
                )?;
            }
            _ => {
                std::fs::copy(self.path(), output).map_err(|_| editor_error("save_as"))?;
            }
        }
        self.saved = Some(self.current);
        Ok(())
    }

    /// 저장(덮어쓰기): 지금 판을 `target` 에 쓴다 — 있던 파일을 바꾼다(같은 폴더 임시 파일에 쓴 뒤 한 번에 바꿔 끼운다 ·
    /// 쓰다 실패해도 원본이 반쯤 깨지지 않는다). 세션은 연 파일을 `v0.HLT` 로 복사해 쓰므로 원본을 덮어도 되돌리기는 그대로다.
    pub fn save_over(&mut self, target: &Path) -> Result<(), CoreError> {
        self.settle();
        let parent = target
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let staged = (0..)
            .map(|n| parent.join(format!(".seoan-save-{}-{n}.HLT", std::process::id())))
            .find(|path| !path.exists())
            .expect("unbounded");
        let written = self.save_as(&staged).and_then(|()| {
            std::fs::rename(&staged, target).map_err(|_| editor_error("save_replace"))
        });
        if written.is_err() {
            let _ = std::fs::remove_file(&staged);
        }
        written
    }

    /// 그림 하나를 PNG 로(화면 그리기용). 배치본(다시 구운 판)의 그림은 1×1 이라 모델의 진짜 본문에서 그린다
    /// (객체 차례 = 모델 그림 차례).
    pub fn image_png(&self, object_code: u32) -> Option<Vec<u8>> {
        let rgba = match &self.versions[self.current] {
            Version {
                generated: true,
                model: Some(model),
                ..
            } => {
                let at = self
                    .document
                    .images
                    .iter()
                    .find(|image| image.object_code == object_code)?
                    .occurrence as usize;
                let id = &model.document.images.get(at)?.id;
                let payload = model
                    .payloads
                    .iter()
                    .find(|payload| &payload.image_id == id)?;
                image::load_from_memory(&payload.bytes).ok()?.to_rgba8()
            }
            _ => crate::print::payload_rgba(self.document.image_payloads.get(&object_code)?)?,
        };
        let mut out = std::io::Cursor::new(Vec::new());
        rgba.write_to(&mut out, image::ImageFormat::Png).ok()?;
        Some(out.into_inner())
    }

    /// Word(.docx) — 지금 판을 진짜 HLT 로 굽고 의미 모델로 되돌려 쓴다. 식별번호는 싣지 않는다.
    pub fn docx(&mut self) -> Result<Vec<u8>, CoreError> {
        let real = self.materialize()?;
        let (model, _) = crate::hml_document_from_hlt(&real)?;
        let (document, _) = load(&real, self.print)?;
        let mut pngs = std::collections::BTreeMap::new();
        for image in &document.images {
            let Some(rgba) = document
                .image_payloads
                .get(&image.object_code)
                .and_then(crate::print::payload_rgba)
            else {
                continue;
            };
            let mut out = std::io::Cursor::new(Vec::new());
            if rgba.write_to(&mut out, image::ImageFormat::Png).is_ok() {
                pngs.insert(image.id.clone(), out.into_inner());
            }
        }
        crate::docx_bytes(&model, &pngs)
    }

    /// 인쇄 PDF — 그림 본문이 있어야 해서 진짜 파일을 판독한다(배치는 화면과 같다).
    #[cfg(feature = "pdf")]
    pub fn print_pdf(&mut self, font: Vec<u8>, font_index: u32) -> Result<Vec<u8>, CoreError> {
        if !self.versions[self.current].generated {
            return crate::write_pdf(&self.plan, &self.document.image_payloads, font, font_index);
        }
        let real = self.materialize()?;
        let (document, plan) = load(&real, self.print)?;
        crate::write_pdf(&plan, &document.image_payloads, font, font_index)
    }

    /// 의미 모델을 고치고 다시 구워 새 판으로 쌓는다. 그림 본문은 지금 판에서 그대로 가져간다.
    pub(crate) fn edit_model<F>(&mut self, edit: F) -> Result<(), CoreError>
    where
        F: FnOnce(&mut HmlDocument) -> Result<(), CoreError>,
    {
        self.edit_model_with(|model, _| edit(model))
    }

    pub(crate) fn edit_model_with<F>(&mut self, edit: F) -> Result<(), CoreError>
    where
        F: FnOnce(&mut HmlDocument, &mut Vec<GeneratedImagePayload>) -> Result<(), CoreError>,
    {
        let dialect = self
            .dialect
            .clone()
            .ok_or_else(|| editor_error("not_editable"))?;
        self.settle();
        let base = match &self.versions[self.current].model {
            Some(model) => model.clone(),
            None => {
                let (mut document, payloads) = hml_document_from_hlt(self.path())?;
                if self.current == 0 {
                    apply_numbers(&mut document, &self.opened_numbers);
                }
                // 이 판의 모델도 남긴다(되돌린 뒤 다시 고칠 때 다시 해석하지 않게).
                let model = Model {
                    origin: self.path().to_path_buf(),
                    document,
                    payloads: std::sync::Arc::new(payloads),
                };
                self.versions[self.current].model = Some(model.clone());
                model
            }
        };
        let mut document = base.document;
        let mut payloads = base.payloads;
        // 그림 본문은 고칠 때(수식 넣기)만 복사한다 — 글·서식 고침은 빈 목록에 덧붙인 것이 없으면 그대로 나눠 쓴다.
        let mut payloads_for_edit: Vec<GeneratedImagePayload> = Vec::new();
        self.cp949_substitutions.clear();
        edit(&mut document, &mut payloads_for_edit)?;
        // 모든 모델 고침이 지나는 관문 — 붙여넣은 글의 CP949 밖 글자를 여기서 바꾼다(대응 없는 글자는 고침 거부).
        let substitutions = crate::make_cp949_safe(&mut document)?;
        if !payloads_for_edit.is_empty() {
            std::sync::Arc::make_mut(&mut payloads).extend(payloads_for_edit);
        }
        renumber(&mut document);
        let used: std::collections::BTreeSet<&str> = document
            .images
            .iter()
            .map(|image| image.id.as_str())
            .collect();
        if payloads
            .iter()
            .any(|payload| !used.contains(payload.image_id.as_str()))
        {
            std::sync::Arc::make_mut(&mut payloads)
                .retain(|payload| used.contains(payload.image_id.as_str()));
        }
        // 배치본만 굽는다(그림 1×1 — pct_b 3.6GB 대신 수 MB). 진짜 파일은 저장·제출·인쇄 때(`materialize`).
        let next = self.fresh("layout.HLT");
        write_light(&document, &dialect, &payloads, &next)?;
        self.drop_superseded_file();
        self.discard_redo();
        self.versions.push(Version {
            path: next,
            real: None,
            model: Some(Model {
                origin: base.origin,
                document,
                payloads,
            }),
            generated: true,
        });
        self.current = self.versions.len() - 1;
        self.reload()?;
        if let Some(model) = self.versions[self.current].model.as_mut() {
            align_cell_ids(&mut model.document, &self.document);
        }
        self.cp949_substitutions = substitutions;
        Ok(())
    }

    /// 되돌린 뒤 고치면 그 뒤 판들을 버린다 — 저장한 판이 그 가운데 있으면 「저장한 판」도 사라진다.
    fn discard_redo(&mut self) {
        if self.saved.is_some_and(|saved| saved > self.current) {
            self.saved = None;
        }
        self.versions.truncate(self.current + 1);
    }

    /// 연(또는 마지막으로 저장한) 뒤 고친 것이 있는가(닫을 때 저장을 묻는다). 되돌려 그 판으로 오면 없다.
    pub fn is_dirty(&self) -> bool {
        self.saved != Some(self.current)
    }

    fn next_path(&self) -> PathBuf {
        self.fresh("HLT")
    }

    /// 세션 폴더의 새 판 파일 이름: 디스크에도 없고 어느 판도 가리키지 않는 이름.
    /// ⛔ 「판 개수」로 지으면 되돌린 뒤 고칠 때 아직 버리지 않은 판과 이름이 겹쳐, 쓸모없어진 판 파일을 지우는 단계가
    /// 방금 쓴 새 판을 지웠다(처음까지 되돌리기 → 고치기 → 되돌리기 → 다시 하기 → 고치기 = `NotFound`).
    fn fresh(&self, suffix: &str) -> PathBuf {
        (self.versions.len()..)
            .map(|n| self.dir.path().join(format!("v{n}.{suffix}")))
            .find(|path| {
                !path.exists()
                    && self
                        .versions
                        .iter()
                        .all(|version| &version.path != path && version.real.as_ref() != Some(path))
            })
            .expect("unbounded")
    }

    /// 새 판을 쌓는다(되돌린 뒤 고치면 그 뒤 판들은 버린다). 제자리 고침(문단번호)은 모델이 없다.
    fn push(&mut self, next: PathBuf) -> Result<(), CoreError> {
        self.discard_redo();
        self.versions.push(Version {
            path: next,
            real: None,
            model: None,
            generated: false,
        });
        self.current = self.versions.len() - 1;
        self.reload()
    }

    /// 지금 판을 판독해 쪽 그림을 만든다. 모델이 있으면 문단 id 를 판독 id(문단 차례)에 맞추고 서식 구간도 모델에서 뽑는다
    /// (서식 목록 판독 2초를 아낀다). 문단 수가 안 맞으면 모델을 버린다(다음 고침에서 파일로 다시 해석).
    fn reload(&mut self) -> Result<(), CoreError> {
        let (document, plan) = load(self.path(), self.print)?;
        self.document = document;
        self.plan = plan;
        // 판독엔 줄 없는 끝 문단 하나가 더 있다(모델은 안 싣는다) — 그 밖엔 문단 차례가 같아야 모델을 쓴다.
        self.refresh_spans();
        Ok(())
    }
}

/// 다시 굽는 법: 본문 규칙(방언) + 틀(내장 프로필 또는 원본 파일에서 뽑은 프로필).
#[derive(Clone)]
struct Recipe {
    dialect: GenerationDialect,
    profile: std::sync::Arc<crate::GenerationProfile>,
}

impl Recipe {
    fn builtin(dialect: GenerationDialect) -> Self {
        Self {
            dialect,
            profile: std::sync::Arc::new(crate::generation_profile(dialect)),
        }
    }
}

/// 배치본 쓰기: 그림 본문을 1×1 흰 PNG 로 바꿔 굽는다. 그림 상자·표시 크기(쪽 배치가 쓰는 값)는 모델 그대로라
/// 쪽 배치는 진짜 파일과 같다(그림 높이 = 표시 pt · dpi 표지는 같은 생성기가 같게 쓴다).
fn write_light(
    document: &HmlDocument,
    dialect: &Recipe,
    payloads: &[GeneratedImagePayload],
    path: &Path,
) -> Result<(), CoreError> {
    static DOT: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    let dot = DOT.get_or_init(|| {
        let mut out = std::io::Cursor::new(Vec::new());
        image::RgbImage::from_pixel(1, 1, image::Rgb([255, 255, 255]))
            .write_to(&mut out, image::ImageFormat::Png)
            .expect("1x1 png");
        out.into_inner()
    });
    // 상자 크기가 비어 있으면 생성기는 화소 크기를 쓴다 — 1×1 이 되지 않게 진짜 본문의 크기를 채운다.
    let mut light = document.clone();
    for image in &mut light.images {
        if image.width.is_none() || image.height.is_none() {
            let size = payloads
                .iter()
                .find(|payload| payload.image_id == image.id)
                .and_then(|payload| {
                    image::ImageReader::new(std::io::Cursor::new(&payload.bytes))
                        .with_guessed_format()
                        .ok()?
                        .into_dimensions()
                        .ok()
                });
            if let Some((width, height)) = size {
                image.width.get_or_insert(width);
                image.height.get_or_insert(height);
            }
        }
    }
    let dots: Vec<GeneratedImagePayload> = light
        .images
        .iter()
        .map(|image| GeneratedImagePayload {
            image_id: image.id.clone(),
            bytes: dot.clone(),
        })
        .collect();
    write_fast(&light, dialect, &dots, path)
}

/// 편집 중 판 쓰기: 굽고 빠른 압축(수준 1)으로 쓰기만 — 되읽기 검증은 저장 때(`save_as`).
fn write_fast(
    document: &HmlDocument,
    dialect: &Recipe,
    payloads: &[GeneratedImagePayload],
    path: &Path,
) -> Result<(), CoreError> {
    let generated = crate::generate_hlt_document_with_profile(
        document,
        dialect.dialect,
        &dialect.profile,
        payloads,
    )?;
    let mut file =
        std::io::BufWriter::new(std::fs::File::create(path).map_err(|_| editor_error("write"))?);
    crate::write::write_hlt_with_level(
        &generated.header,
        &mut std::io::Cursor::new(&generated.inflated),
        &mut file,
        1,
    )?;
    Ok(())
}

/// 모델 문단 id 를 판독한 문단 id(같은 차례)로 바꾼다 — 화면은 판독 id 로 고침을 부른다.
fn normalize_ids(model: &mut HmlDocument, document: &InspectDocument) {
    let map: std::collections::BTreeMap<String, String> = model
        .paragraphs
        .iter()
        .zip(&document.paragraphs)
        .filter(|(old, new)| old.id != new.id)
        .map(|(old, new)| (old.id.clone(), new.id.clone()))
        .collect();
    if map.is_empty() {
        return;
    }
    let fix = |id: &mut String| {
        if let Some(new) = map.get(id) {
            *id = new.clone();
        }
    };
    for paragraph in &mut model.paragraphs {
        fix(&mut paragraph.id);
    }
    for section in &mut model.sections {
        section.paragraph_ids.iter_mut().for_each(fix);
    }
    for image in &mut model.images {
        if let Some(id) = image.paragraph_id.as_mut() {
            fix(id);
        }
    }
    for table in &mut model.tables {
        fix(&mut table.paragraph_id);
    }
}

/// 모델에서 문단마다 서식 구간(글자 자리 = 그림 한 칸).
fn model_spans(model: &HmlDocument) -> std::collections::BTreeMap<String, Vec<EditorSpan>> {
    let mut out = std::collections::BTreeMap::new();
    for paragraph in &model.paragraphs {
        let mut spans: Vec<EditorSpan> = Vec::new();
        for (at, cell) in cells(&paragraph.inlines).iter().enumerate() {
            let Cell::Char(_, formats, size) = cell else {
                continue;
            };
            let style = formats
                .iter()
                .fold(0, |bits, format| bits | format_bit(*format));
            for (style, size) in [(style, None), (0, size.filter(|size| *size != 12))] {
                if style == 0 && size.is_none() {
                    continue;
                }
                match spans
                    .iter_mut()
                    .rev()
                    .find(|span| span.style == style && span.size == size)
                {
                    Some(span) if span.end as usize == at => span.end += 1,
                    _ => spans.push(EditorSpan {
                        start: at as u32,
                        end: at as u32 + 1,
                        style,
                        size,
                    }),
                }
            }
        }
        if !spans.is_empty() {
            out.insert(paragraph.id.clone(), spans);
        }
    }
    out
}

fn format_bit(format: CharacterFormat) -> u32 {
    match format {
        CharacterFormat::Underline => 1,
        CharacterFormat::Bold => 2,
        CharacterFormat::Italic => 4,
        CharacterFormat::Superscript => 16,
        CharacterFormat::Subscript => 32,
    }
}

/// 모델을 고친 뒤 차례 값·요약·절 소속을 다시 맞춘다(생성기가 차례와 개수를 검사한다).
fn renumber(model: &mut HmlDocument) {
    // 그림: 어디서도 안 가리키는 그림(자리표를 지운 그림)만 빼고, 원래 차례대로 다시 센다.
    // 편집은 그림 차례를 바꾸지 않는다(문단 나누기도 글 차례 그대로) — 닻 차례 = 옛 차례.
    let mut used: std::collections::BTreeSet<String> = model
        .paragraphs
        .iter()
        .flat_map(|paragraph| paragraph.image_ids.iter().cloned())
        .collect();
    for cell in &model.cells {
        for inline in &cell.inlines {
            if let HmlInline::Image { image_id } = inline {
                used.insert(image_id.clone());
            }
        }
    }
    model.images.retain(|image| used.contains(&image.id));
    // 차례 = 본문 닻 차례. 옛 그림은 옛 차례가 곧 닻 차례다 · 새 그림(차례 0 으로 들어온 것)은 그 문단 자리에 끼운다.
    let paragraph_at = |id: Option<&str>| {
        id.and_then(|id| {
            model
                .paragraphs
                .iter()
                .position(|paragraph| paragraph.id == id)
        })
    };
    let mut old: Vec<crate::HmlImage> = Vec::new();
    let mut fresh: Vec<crate::HmlImage> = Vec::new();
    for image in std::mem::take(&mut model.images) {
        if image.id.starts_with("image:new:") && image.occurrence == 0 {
            fresh.push(image);
        } else {
            old.push(image);
        }
    }
    old.sort_by_key(|image| image.occurrence);
    // 자리 = (닻 문단 차례, 표 칸 그림이면 (표 안 칸 차례, 칸 안 그림 차례)) — 문단 자기 그림이 표 칸 그림보다 앞.
    let place = |image: &crate::HmlImage| {
        let cell = image.cell_id.as_deref().and_then(|cell_id| {
            let cell = model.cells.iter().find(|cell| cell.id == cell_id)?;
            let table = model
                .tables
                .iter()
                .find(|table| table.id == cell.table_id)?;
            let rank = table.cell_ids.iter().position(|id| id == cell_id)?;
            let within = cell.image_ids.iter().position(|id| *id == image.id)?;
            Some((rank, within))
        });
        (
            paragraph_at(image.paragraph_id.as_deref()).unwrap_or(usize::MAX),
            cell,
        )
    };
    for image in fresh {
        let at = place(&image);
        let slot = old
            .iter()
            .position(|other| place(other) > at)
            .unwrap_or(old.len());
        old.insert(slot, image);
    }
    model.images = old;
    for (occurrence, image) in model.images.iter_mut().enumerate() {
        image.occurrence = occurrence as u32;
    }
    // 절: 문단 차례가 바뀌었으니 절마다 문단 목록을 다시 모은다.
    for section in &mut model.sections {
        section.paragraph_ids = model
            .paragraphs
            .iter()
            .filter(|paragraph| paragraph.section_id.as_deref() == Some(section.id.as_str()))
            .map(|paragraph| paragraph.id.clone())
            .collect();
    }
    model.summary.image_count = model.images.len() as u64;
    model.summary.content_paragraph_count = model.paragraphs.len() as u64;
}

/// 고친 뒤 모델 칸 id 를 다시 읽은 판(쪽 계획 · 화면)의 칸 id(`cell:표:행:차례`)로 맞춘다.
/// 표 엔진이 만든 칸(`cell:…:new:…`)은 판독 id 와 갈라져, 화면이 누른 칸을 모델에서 못 찾았다(칸 창 · 「표」 메뉴).
/// 표마다 차례대로 짝짓는다 — 세로 병합 이어짐 칸은 판독에 따로 없어 빼고, 수가 안 맞는 표는 건드리지 않는다.
fn align_cell_ids(model: &mut HmlDocument, layout: &crate::InspectDocument) {
    let continuation: std::collections::BTreeSet<String> = model
        .cells
        .iter()
        .filter(|cell| cell.merge_continuation)
        .map(|cell| cell.id.clone())
        .collect();
    let mut rename = std::collections::BTreeMap::new();
    for (table, read) in model.tables.iter().zip(&layout.tables) {
        let ours: Vec<&String> = table
            .cell_ids
            .iter()
            .filter(|id| !continuation.contains(*id))
            .collect();
        if ours.len() != read.cell_ids.len() {
            continue;
        }
        for (old, new) in ours.into_iter().zip(&read.cell_ids) {
            if old != new {
                rename.insert(old.clone(), new.clone());
            }
        }
    }
    // 이어짐 칸 id 가 새 id 와 겹치면 표 꼬리에서 칸이 둘이 된다 — 꼬리표를 달아 비킨다.
    let taken: std::collections::BTreeSet<String> = rename.values().cloned().collect();
    for id in &continuation {
        if taken.contains(id) {
            rename.insert(id.clone(), format!("{id}:continued"));
        }
    }
    if rename.is_empty() {
        return;
    }
    let renamed = |id: &mut String| {
        if let Some(new) = rename.get(id.as_str()) {
            *id = new.clone();
        }
    };
    for cell in &mut model.cells {
        renamed(&mut cell.id);
    }
    for table in &mut model.tables {
        table.cell_ids.iter_mut().for_each(renamed);
    }
    for image in &mut model.images {
        if let Some(id) = image.cell_id.as_mut() {
            renamed(id);
        }
    }
}

/// 표 연산 하나의 결과 — 건너뛰었으면 `table_skipped:<이유>` 오류.
fn table_decision(plan: crate::TableEditPlan) -> Result<crate::TableEditDecision, CoreError> {
    let decision = plan
        .decisions
        .into_iter()
        .next()
        .ok_or_else(|| editor_error("table"))?;
    match decision.reason {
        Some(reason) if decision.status == crate::TableEditDecisionStatus::Skipped => {
            let reason = serde_json::to_value(reason)
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_default();
            Err(editor_error(&format!("table_skipped:{reason}")))
        }
        _ => Ok(decision),
    }
}

fn find_bit(format: FindFormat) -> u32 {
    match format {
        FindFormat::Underline => 1,
        FindFormat::Bold => 2,
        FindFormat::Italic => 4,
        FindFormat::Superscript => 16,
        FindFormat::Subscript => 32,
        FindFormat::Plain => 0,
    }
}

fn reformat(formats: &mut Vec<CharacterFormat>, format: ReplaceFormat) {
    let script = |formats: &mut Vec<CharacterFormat>| {
        formats.retain(|value| {
            !matches!(
                value,
                CharacterFormat::Superscript | CharacterFormat::Subscript
            )
        })
    };
    let add = |formats: &mut Vec<CharacterFormat>, value| {
        if !formats.contains(&value) {
            formats.push(value);
        }
    };
    match format {
        ReplaceFormat::Keep => {}
        ReplaceFormat::Superscript => {
            script(formats);
            formats.push(CharacterFormat::Superscript);
        }
        ReplaceFormat::Subscript => {
            script(formats);
            formats.push(CharacterFormat::Subscript);
        }
        ReplaceFormat::Normal => script(formats),
        ReplaceFormat::Bold => add(formats, CharacterFormat::Bold),
        ReplaceFormat::Italic => add(formats, CharacterFormat::Italic),
        ReplaceFormat::Underline => add(formats, CharacterFormat::Underline),
        ReplaceFormat::Clear => formats.clear(),
    }
}

fn cells(inlines: &[HmlInline]) -> Vec<Cell> {
    let mut out = Vec::new();
    for inline in inlines {
        match inline {
            HmlInline::Text {
                text,
                formats,
                size,
            } => out.extend(
                text.chars()
                    .map(|ch| Cell::Char(ch, formats.clone(), *size)),
            ),
            HmlInline::Image { image_id } => out.push(Cell::Image(image_id.clone())),
        }
    }
    out
}

/// 글자열 바꾸기 — 앞뒤 같은 칸은 그대로(서식·그림 유지) · 가운데만 새 글자로. 새 글자는 바로 앞 칸 서식.
/// 그림은 글자에서 `_` 로 보인다 — 새 글에 `_` 가 남은 만큼 앞뒤 그림이 산다.
/// 칸 글자 · 그림 차례로 칸의 `inlines` · `text`(그림 뺌) · `image_ids` 를 다시 만든다.
fn set_cell(cell: &mut crate::HmlCell, edited: &[Cell]) {
    cell.inlines = inlines(edited);
    cell.text = edited
        .iter()
        .filter_map(|value| match value {
            Cell::Char(ch, ..) => Some(*ch),
            Cell::Image(_) => None,
        })
        .collect();
    cell.image_ids = edited
        .iter()
        .filter_map(|value| match value {
            Cell::Image(id) => Some(id.clone()),
            Cell::Char(..) => None,
        })
        .collect();
}

fn splice(old: &[Cell], new: &[char]) -> Vec<Cell> {
    let visible = |cell: &Cell| match cell {
        Cell::Char(ch, ..) => *ch,
        Cell::Image(_) => '_',
    };
    let prefix = old
        .iter()
        .zip(new)
        .take_while(|(cell, ch)| visible(cell) == **ch)
        .count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(cell, ch)| visible(cell) == **ch)
        .count();
    let mut out: Vec<Cell> = old[..prefix].to_vec();
    let (formats, size) = old[..prefix]
        .iter()
        .rev()
        .chain(old.iter())
        .find_map(|cell| match cell {
            Cell::Char(_, formats, size) => Some((formats.clone(), *size)),
            Cell::Image(_) => None,
        })
        .unwrap_or_default();
    for ch in &new[prefix..new.len() - suffix] {
        out.push(Cell::Char(*ch, formats.clone(), size));
    }
    out.extend_from_slice(&old[old.len() - suffix..]);
    out
}

fn inlines(cells: &[Cell]) -> Vec<HmlInline> {
    let mut out: Vec<HmlInline> = Vec::new();
    for cell in cells {
        match cell {
            Cell::Image(id) => out.push(HmlInline::Image {
                image_id: id.clone(),
            }),
            Cell::Char(ch, formats, size) => match out.last_mut() {
                Some(HmlInline::Text {
                    text,
                    formats: last,
                    size: last_size,
                }) if last == formats && last_size == size => text.push(*ch),
                _ => out.push(HmlInline::Text {
                    text: ch.to_string(),
                    formats: formats.clone(),
                    size: *size,
                }),
            },
        }
    }
    out
}

/// 문단마다 서식 구간 · 크기 구간(편집 창이 서식을 보여 준다). 못 읽으면 비운다(화면 표시용).
fn load_spans(path: &Path) -> std::collections::BTreeMap<String, Vec<EditorSpan>> {
    let mut out: std::collections::BTreeMap<String, Vec<EditorSpan>> = Default::default();
    let Ok((inventory, _)) = crate::inspect_format_inventories(path) else {
        return out;
    };
    for paragraph in inventory.paragraphs {
        let entry = out.entry(paragraph.paragraph_id.clone()).or_default();
        for span in &paragraph.spans {
            let style = span.formats.iter().fold(0, |bits, format| {
                bits | match format {
                    CharacterFormat::Underline => 1,
                    CharacterFormat::Bold => 2,
                    CharacterFormat::Italic => 4,
                    CharacterFormat::Superscript => 16,
                    CharacterFormat::Subscript => 32,
                }
            });
            entry.push(EditorSpan {
                start: span.start,
                end: span.end,
                style,
                size: None,
            });
        }
        for run in paragraph.sizes.iter().filter(|run| run.size != 12) {
            entry.push(EditorSpan {
                start: run.start,
                end: run.end,
                style: 0,
                size: Some(run.size),
            });
        }
    }
    out
}

fn load(path: &Path, print: PrintOptions) -> Result<(InspectDocument, PagePlan), CoreError> {
    let document = inspect_path(
        path,
        InspectOptions {
            include_image_payloads: true,
            ..InspectOptions::default()
        },
    )?;
    let (_, inflated) = inflate_hlt(path)?;
    let mut plan = page_plan(&document, &inflated, print);
    let mut document = document;
    // 쪽나눔 문단은 판독 글이 폼피드(`\x0c`)로 시작한다 — 모델 글(서식 구간 · 고침 오프셋)에는 없는 칸이다.
    // 화면에 주는 글과 줄 시작 오프셋에서 떼어 모델과 같은 자리를 가리키게 한다
    // (안 떼면 그 문단의 서식이 한 칸 밀려 보이고, 고치면 폼피드가 글자로 들어간다).
    let mut lead = std::collections::HashMap::new();
    for paragraph in &mut document.paragraphs {
        let stripped = paragraph.text.trim_start_matches('\x0c');
        let count = paragraph.text.len() - stripped.len();
        if count > 0 {
            paragraph.text = stripped.to_string();
            lead.insert(paragraph.id.clone(), count as u32);
        }
    }
    for line in plan.pages.iter_mut().flat_map(|page| &mut page.lines) {
        if let (Some(start), Some(count)) = (
            line.start.as_mut(),
            line.paragraph.as_ref().and_then(|id| lead.get(id)),
        ) {
            *start = start.saturating_sub(*count);
        }
    }
    Ok((document, plan))
}

pub(crate) fn editor_error(reason: &str) -> CoreError {
    let mut error = CoreError::new(ErrorCode::StructureInvalid, "editor", false);
    error.details.insert("reason".into(), reason.into());
    error
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 크기 바꾸기: 표 칸 안 그림은 칸 폭에서 멈추고, 본문 그림은 쪽 본문 폭(174mm)까지 큰다.
    #[test]
    fn a_picture_in_a_table_cell_grows_no_wider_than_its_cell() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("의견서.HLT");
        crate::save_new_opinion_as_new(&path, crate::WriteOptions::default()).unwrap();
        let mut session = EditorSession::open(&path).unwrap();
        let body = session.paragraphs()[1].id.clone();
        let mut png = Vec::new();
        image::RgbImage::from_pixel(80, 40, image::Rgb([0, 0, 0]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        session.insert_picture(&body, &png).unwrap();
        let cell = session.insert_table(&body, 1, 3).unwrap();
        session.insert_picture_in_cell(&cell, 0, &png).unwrap();
        let placed = session.document.images.clone();
        assert_eq!(placed.len(), 2);
        let in_cell = placed.iter().find(|image| image.cell_id.is_some()).unwrap();
        let in_body = placed.iter().find(|image| image.cell_id.is_none()).unwrap();
        let cell_pt = session
            .document
            .cells
            .iter()
            .find(|c| Some(&c.id) == in_cell.cell_id.as_ref())
            .map(|c| f64::from(c.width) / 20.0)
            .unwrap();
        assert!(
            f64::from(in_cell.display_width) < cell_pt,
            "{in_cell:?} · 칸 {cell_pt}pt"
        );

        let (cell_id, body_id) = (in_cell.id.clone(), in_body.id.clone());
        session.resize_image(&cell_id, 100.0).unwrap();
        session.resize_image(&body_id, 100.0).unwrap();
        let grown = session.document.images.clone();
        assert_eq!(grown.len(), 2);
        let width = |id: &str| {
            f64::from(
                grown
                    .iter()
                    .find(|image| image.id == id)
                    .unwrap()
                    .display_width,
            )
        };
        assert!(
            width(&cell_id) <= cell_pt + 1.0 && width(&cell_id) > cell_pt * 0.9,
            "칸 그림 {} · 칸 {cell_pt}pt",
            width(&cell_id)
        );
        assert!(
            width(&body_id) > cell_pt + 50.0 && width(&body_id) < 520.0,
            "본문 그림 {}",
            width(&body_id)
        );
    }

    #[test]
    fn splice_keeps_formats_of_untouched_characters() {
        let bold = vec![CharacterFormat::Bold];
        let old = vec![
            Cell::Char('R', vec![], None),
            Cell::Char('6', vec![CharacterFormat::Subscript], None),
            Cell::Char('는', vec![], None),
            Cell::Image("image:1".into()),
            Cell::Char('끝', bold.clone(), None),
        ];
        // 「R6는 _끝」 → 「R6는 짧은 _끝」: 첨자 6 · 그림 · 굵은 끝이 산다, 새 글자는 앞 칸(는) 서식.
        let new: Vec<char> = "R6는짧은_끝".chars().collect();
        let out = splice(&old, &new);
        assert_eq!(
            out[1],
            Cell::Char('6', vec![CharacterFormat::Subscript], None)
        );
        assert_eq!(out[3], Cell::Char('짧', vec![], None));
        assert_eq!(out[5], Cell::Image("image:1".into()));
        assert_eq!(out[6], Cell::Char('끝', bold, None));
        // 그림 자리표를 지우면 그림이 빠진다.
        let out = splice(&old, &"R6는끝".chars().collect::<Vec<_>>());
        assert!(!out.iter().any(|cell| matches!(cell, Cell::Image(_))));
    }
}

/// 식별항목 표제(통특기 우클릭 「식별항목」 7종 + 영문 · 숫자 번호)인가.
fn is_item_heading(text: &str) -> bool {
    const KINDS: [&str; 7] = ["도", "표", "청구항", "수학식", "화학식", "반응식", "참고도"];
    let Some(inner) = text
        .trim()
        .strip_prefix('【')
        .and_then(|rest| rest.strip_suffix('】'))
    else {
        return false;
    };
    let Some((kind, number)) = inner.split_once(' ') else {
        return false;
    };
    KINDS.contains(&kind)
        && !number.is_empty()
        && number.chars().all(|ch| ch.is_ascii_alphanumeric())
}

/// 「【…】」 하나뿐인 문단(식별항목 · 절 표제) — 청구항 내용이 끝나는 자리.
fn is_bracket_heading(text: &str) -> bool {
    let text = text.trim();
    text.starts_with('【') && text.ends_with('】') && text.matches('】').count() == 1
}

/// 최종본에서 읽은 식별번호를 첫 판 모델에 입힌다. 문단 수가 같을 때만(어긋나면 엉뚱한 문단에 붙지 않게 버린다).
fn apply_numbers(document: &mut HmlDocument, numbers: &[Option<String>]) {
    if numbers.is_empty() || numbers.len() != document.paragraphs.len() {
        return;
    }
    for (paragraph, number) in document.paragraphs.iter_mut().zip(numbers) {
        paragraph.number = number.clone();
    }
}
