//! 문단번호 넣기 — 본문 문단마다 식별번호(`wipo_id`)를 1부터 차례로 매긴다.
//!
//! 통특기와 같은 규칙:
//! - 번호는 레지스터(절 줄지도)의 줄마다 칸 중 **문단 첫 줄** 칸에만 들어간다. 번호를 안 매기는 줄의 옛 값은 그대로 둔다.
//! - 표제(`【…】`)는 번호가 없다. 다만 `【표 N】` · `【수학식 N】` · `【화학식 N】` 은 번호를 받는다.
//! - 표는 표제와 따로 번호 하나를 끝 줄(칸 밖 닫는 줄)에 받는다 — 표제 없는 표도 받는다.
//! - `【발명의 명칭】` 절 글과 `【도면】` 절 전부는 번호가 없다. 표 칸 줄도 없다.
//! - 종류 111(최종본을 불러온 보정 명세서)에서는 아무것도 바꾸지 않는다 — 우리는 거절한다(`amendment_document`).
//! - 그 밖에 바뀌는 바이트는 없다(힙 포인터 · 저장 시각 · 버퍼 찌꺼기는 저장마다 바뀌는 잡음).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::{
    CoreError, ErrorCode, InspectDocument, SaveAsNewResult, StructuralParity, WriteOptions,
    inspect::{cell_record_size, inspect_edit_layout, locate_tail, register_paired},
    save::{InflatedPatch, open_inflated_source, publish_inflated_as_new, rewrite_inflated_spool},
};

/// 줄마다 매길 번호(줄 차례 → 번호). 문단 첫 줄에 번호, 표는 끝 줄(칸 밖 닫는 줄)에 번호.
/// 여기 없는 줄은 0 이 된다.
pub fn plan_paragraph_numbers(document: &InspectDocument) -> Result<BTreeMap<u64, u32>, CoreError> {
    let cell_lines: BTreeSet<&str> = document
        .cells
        .iter()
        .flat_map(|cell| cell.line_ids.iter().map(String::as_str))
        .collect();
    let line_index: BTreeMap<&str, u64> = document
        .lines
        .iter()
        .map(|line| (line.id.as_str(), line.index))
        .collect();
    let line_of = |id: Option<&String>| {
        id.and_then(|id| line_index.get(id.as_str()))
            .copied()
            .filter(|line| *line > 0)
            .ok_or_else(|| numbering_error("line"))
    };
    let mut counter = 0_u32;
    let mut skipping = false;
    let mut numbers = BTreeMap::new();
    for paragraph in &document.paragraphs {
        let text = paragraph.text.trim();
        let heading = text.starts_with('【') && text.ends_with('】');
        let figure = ["【표 ", "【수학식 ", "【화학식 "]
            .iter()
            .any(|prefix| text.starts_with(prefix));
        if heading && !figure {
            // `【도 N】` 은 【도면】 절 안의 표제라 건너뛰기를 끝내지 않는다.
            if !text.starts_with("【도 ") {
                skipping = matches!(text, "【발명의 명칭】" | "【도면】");
            }
            continue;
        }
        let in_table = paragraph
            .line_ids
            .iter()
            .any(|line| cell_lines.contains(line.as_str()));
        if in_table {
            // 표는 표제가 있든 없든 나온 자리에서 번호 하나를 끝 줄에 받는다
            // (pct_a: 【표 1】 과 표 사이 문단이 먼저 받고, 표제 없는 둘째 표도 받는다).
            if !skipping {
                counter += 1;
                numbers.insert(line_of(paragraph.line_ids.last())?, counter);
            }
            continue;
        }
        if skipping {
            continue;
        }
        counter += 1;
        numbers.insert(line_of(paragraph.line_ids.first())?, counter);
    }
    Ok(numbers)
}

/// 문단번호를 넣은 새 파일을 쓴다(원본은 그대로).
pub fn number_paragraphs_as_new(
    source: &Path,
    output: &Path,
    options: WriteOptions,
) -> Result<SaveAsNewResult, CoreError> {
    let mut input = open_inflated_source(source, &options.verify)?;
    // 종류 111(최종본을 불러온 보정 명세서)은 식별번호가 보정서의 기준이라 통특기도 손대지 않는다(ins · del9 무변화).
    // 통특기는 말없이 넘어가지만 우리는 거절해 이유를 알린다.
    if input.document.summary.document_type_raw & 0x7fff == 111 {
        return Err(numbering_error("amendment_document"));
    }
    let numbers = plan_paragraph_numbers(&input.document)?;
    // 되읽기 판정용: 판독기는 문단 첫 줄 칸을 문단 번호로 읽는다.
    let first_lines: Vec<Option<u64>> = input
        .document
        .paragraphs
        .iter()
        .map(|paragraph| {
            let id = paragraph.line_ids.first()?;
            input
                .document
                .lines
                .iter()
                .find(|line| &line.id == id)
                .map(|line| line.index)
        })
        .collect();
    let expected: Vec<Option<u32>> = first_lines
        .iter()
        .zip(&input.document.paragraphs)
        .map(|(line, paragraph)| {
            line.and_then(|line| numbers.get(&line).copied())
                .or(paragraph.wipo_id)
        })
        .collect();
    let inflated = input.document.summary.inflated_bytes;
    let layout = inspect_edit_layout(&mut input.spool, inflated)?;
    let header = input.layout.header;
    let tail = locate_tail(
        &mut input.spool,
        inflated,
        layout.line_base,
        layout.total_lines,
        cell_record_size(&header)?,
    )?;
    let paired = register_paired(&header);
    // 레지스터 = 줄 1.. 마다 (절, 번호) 짝 또는 번호 하나. 줄 0 은 칸이 없다.
    // 번호를 매기는 줄만 쓴다 — 통특기는 다른 줄의 옛 값을 지우지 않는다(pct_a 쪽나눔 줄의 874).
    let mut patches: Vec<InflatedPatch> = numbers
        .iter()
        .map(|(line, number)| {
            let index = if paired { 2 * (line - 1) + 1 } else { line - 1 };
            let start_byte = tail.register + 5 + 4 * index;
            InflatedPatch {
                start_byte,
                end_byte: start_byte + 4,
                replacement: number.to_le_bytes().to_vec(),
            }
        })
        .collect();
    rewrite_inflated_spool(&mut input, &mut patches)?;
    let published = publish_inflated_as_new(
        &mut input,
        output,
        &options.verify,
        |_, output_document, _| {
            let got: Vec<Option<u32>> = output_document
                .paragraphs
                .iter()
                .map(|paragraph| paragraph.wipo_id)
                .collect();
            if got == expected {
                Ok(())
            } else {
                Err(numbering_error("readback"))
            }
        },
    )?;
    Ok(SaveAsNewResult {
        input_source: input.source_name,
        output_source: published.output_source,
        input_file_size: input.verification.file_size,
        output_file_size: published.output_file_size,
        input_sha256: input.verification.input_sha256,
        output_sha256: published.output_sha256,
        writer: published.writer,
        verification: published.verification,
        structural_parity: StructuralParity {
            header_equal: true,
            inflated_equal: false,
            summary_equal: false,
            fixed_blocks_equal: true,
        },
    })
}

fn numbering_error(reason: &str) -> CoreError {
    let mut error = CoreError::new(ErrorCode::StructureInvalid, "number_paragraphs", false);
    error.details.insert("reason".into(), reason.into());
    error
}
