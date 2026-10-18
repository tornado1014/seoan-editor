//! 쪽 배치 → PDF(P4-4). 글꼴은 사용자 시스템의 것을 경로로 받는다 — 바탕체는 재배포 금지라 동봉하지 않는다
//! (맥: Office 가 깐 `~/Library/Fonts/batang.ttc` 의 1번 = BatangChe · 통특기 인쇄 PDF 와 같은 고정폭).

use std::collections::BTreeMap;

use krilla::{
    Document,
    geom::{PathBuilder, Point, Rect, Size, Transform},
    image::Image,
    page::PageSettings,
    paint::Stroke,
    text::{Font, TextDirection},
};

use crate::{
    CoreError, ErrorCode, HltImagePayload, PagePlan,
    print::{BOLD, ITALIC},
};

/// 글자 상자 윗변 → 기준선 거리(글자 크기 배수). 우리 PDF 를 pdftotext 로 되읽은 윗변이 계획 값과 같도록 맞춘 값.
const BASELINE_FROM_TOP: f32 = 0.859;
/// 기울임 = 글자를 0.34 만큼 민 사선(통특기 인쇄의 기울임 상자 높이 12.67/11.99 = √(1+0.34²)).
const ITALIC_SLANT: f32 = 0.34;
/// 굵게 = 채움 + 얇은 윤곽선(글자 크기 배수).
const BOLD_STROKE: f32 = 0.03;
/// 밑줄: 기준선 아래 거리 · 두께(글자 크기 배수).
const UNDERLINE_DROP: f32 = 0.12;
const UNDERLINE_THICKNESS: f32 = 0.05;
/// 표 칸 테두리 굵기(pt).
const CELL_BORDER: f32 = 0.5;

/// `payloads` = `InspectDocument::image_payloads`(그림 본문을 실어 판독한 것). 없는 그림은 건너뛴다.
pub fn write_pdf(
    plan: &PagePlan,
    payloads: &BTreeMap<u32, HltImagePayload>,
    font: Vec<u8>,
    font_index: u32,
) -> Result<Vec<u8>, CoreError> {
    let font = Font::new(font.into(), font_index).ok_or_else(|| pdf_error("font"))?;
    let mut decoded: BTreeMap<u32, Image> = BTreeMap::new();
    let mut document = Document::new();
    for planned in &plan.pages {
        let settings = PageSettings::from_wh(plan.width as f32, plan.height as f32)
            .ok_or_else(|| pdf_error("page_size"))?;
        let mut page = document.start_page_with(settings);
        let mut surface = page.surface();
        for placed in &planned.images {
            let image = match decoded.get(&placed.object_code) {
                Some(image) => image.clone(),
                None => {
                    let Some(image) = payloads.get(&placed.object_code).and_then(payload_image)
                    else {
                        continue;
                    };
                    decoded.insert(placed.object_code, image.clone());
                    image
                }
            };
            let Some(size) = Size::from_wh(placed.width as f32, placed.height as f32) else {
                continue;
            };
            surface.push_transform(&Transform::from_translate(
                placed.x as f32,
                placed.top as f32,
            ));
            surface.draw_image(image, size);
            surface.pop();
        }
        for line in &planned.lines {
            for glyph in &line.glyphs {
                let size = glyph.size as f32;
                let (x, baseline) = (
                    glyph.x as f32,
                    (line.top + glyph.drop) as f32 + BASELINE_FROM_TOP * size,
                );
                let mut text = [0_u8; 4];
                // 기준선을 원점으로 옮겨 그린다 — 기울임은 그 원점에서 민다(y 아래가 +라 위쪽 글자가 오른쪽으로).
                let slant = if glyph.style & ITALIC != 0 {
                    -ITALIC_SLANT
                } else {
                    0.0
                };
                surface.push_transform(&Transform::from_row(1.0, 0.0, slant, 1.0, x, baseline));
                if glyph.style & BOLD != 0 {
                    surface.set_stroke(Some(Stroke {
                        width: BOLD_STROKE * size,
                        ..Stroke::default()
                    }));
                }
                surface.draw_text(
                    Point::from_xy(0.0, 0.0),
                    font.clone(),
                    size,
                    glyph.ch.encode_utf8(&mut text),
                    false,
                    TextDirection::Auto,
                );
                surface.set_stroke(None);
                surface.pop();
            }
        }
        // 밑줄: 줄마다 공백까지 이은 구간(`PlannedLine::underlines`) — 글자마다 그리면 공백에서 끊겼다.
        // 본문 글자 크기(12pt) 기준 — 첨자 글자에 걸린 밑줄도 본문 기준선에 긋는다.
        const FONT_SIZE: f32 = 12.0;
        for line in &planned.lines {
            let baseline = line.top as f32 + BASELINE_FROM_TOP * FONT_SIZE;
            for [from, to] in &line.underlines {
                let Some(rect) = Rect::from_xywh(
                    *from as f32,
                    baseline + UNDERLINE_DROP * FONT_SIZE,
                    (*to - *from) as f32,
                    UNDERLINE_THICKNESS * FONT_SIZE,
                ) else {
                    continue;
                };
                let mut path = PathBuilder::new();
                path.push_rect(rect);
                if let Some(path) = path.finish() {
                    surface.draw_path(&path);
                }
            }
        }
        // 표 칸 테두리(0.5pt) — 채우지 않고 선만. 쪽의 맨 끝에 그려 위 그리기의 채움 설정을 건드리지 않는다.
        if !planned.cells.is_empty() {
            surface.set_fill(None);
            surface.set_stroke(Some(Stroke {
                width: CELL_BORDER,
                ..Stroke::default()
            }));
            for cell in &planned.cells {
                let Some(rect) = Rect::from_xywh(
                    cell.x as f32,
                    cell.top as f32,
                    cell.width as f32,
                    cell.height as f32,
                ) else {
                    continue;
                };
                let mut path = PathBuilder::new();
                path.push_rect(rect);
                if let Some(path) = path.finish() {
                    surface.draw_path(&path);
                }
            }
        }
        surface.finish();
        page.finish();
    }
    document.finish().map_err(|_| pdf_error("serialize"))
}

fn pdf_error(reason: &str) -> CoreError {
    let mut error = CoreError::new(ErrorCode::StructureInvalid, "write_pdf", false);
    error.details.insert("reason".into(), reason.into());
    error
}

/// 그림 본문 → krilla 그림.
fn payload_image(payload: &HltImagePayload) -> Option<Image> {
    let rgba = crate::print::payload_rgba(payload)?;
    let (width, height) = rgba.dimensions();
    Some(Image::from_rgba8(rgba.into_raw(), width, height))
}
