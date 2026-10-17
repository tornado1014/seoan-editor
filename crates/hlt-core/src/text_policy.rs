use difflib::sequencematcher::{Opcode, SequenceMatcher};

use crate::{CoreError, ErrorCode};

const HLT_IMAGE_MARK: char = '_';
const XML_IMAGE_MARK: char = '\u{2063}';

pub fn carry_character_codes(
    old_text: &str,
    old_codes: &[u32],
    new_text: &str,
    plain_code: u32,
) -> Result<Vec<u32>, CoreError> {
    let old_characters: Vec<char> = old_text.chars().collect();
    if old_characters.len() != old_codes.len() {
        let mut error = CoreError::new(ErrorCode::ConversionFailed, "carry_codes", false);
        error.expected = Some(old_characters.len().to_string());
        error.actual = Some(old_codes.len().to_string());
        return Err(error);
    }
    let new_characters: Vec<char> = new_text.chars().collect();
    let mut matcher = SequenceMatcher::new(&old_characters, &new_characters);
    let mut output = vec![plain_code; new_characters.len()];
    for opcode in matcher.get_opcodes() {
        apply_opcode(
            &opcode,
            &old_characters,
            old_codes,
            &new_characters,
            plain_code,
            &mut output,
        );
    }
    Ok(output)
}

pub fn restore_automatic_line_breaks(
    old_text_with_breaks: &str,
    new_display_text: &str,
) -> Result<String, CoreError> {
    if new_display_text
        .chars()
        .any(|value| matches!(value, '\r' | '\n' | '\0' | '\u{0015}'))
    {
        return Err(CoreError::new(
            ErrorCode::ConversionFailed,
            "line_breaks",
            false,
        ));
    }

    let mut old_plain = Vec::new();
    let mut break_positions = Vec::new();
    let mut characters = old_text_with_breaks.chars().peekable();
    while let Some(value) = characters.next() {
        match value {
            '\r' => {
                break_positions.push(old_plain.len());
                if characters.peek() == Some(&'\n') {
                    characters.next();
                }
            }
            '\n' => break_positions.push(old_plain.len()),
            _ => old_plain.push(value),
        }
    }
    if break_positions.is_empty() {
        return Ok(new_display_text.to_string());
    }

    let new_characters: Vec<char> = new_display_text.chars().collect();
    let mut matcher = SequenceMatcher::new(&old_plain, &new_characters);
    let opcodes = matcher.get_opcodes();
    let old_len = old_plain.len();
    let new_len = new_characters.len();
    let mut output = new_characters;
    break_positions.sort_unstable_by(|left, right| right.cmp(left));
    for old_position in break_positions {
        let position = mapped_position(old_position, old_len, new_len, &opcodes);
        output.splice(position..position, ['\r', '\n']);
    }
    Ok(output.into_iter().collect())
}

fn apply_opcode(
    opcode: &Opcode,
    old_characters: &[char],
    old_codes: &[u32],
    new_characters: &[char],
    plain_code: u32,
    output: &mut [u32],
) {
    match opcode.tag.as_str() {
        "equal" => {
            for distance in 0..(opcode.first_end - opcode.first_start) {
                output[opcode.second_start + distance] = old_codes[opcode.first_start + distance];
            }
        }
        "replace" => {
            let old_width = opcode.first_end - opcode.first_start;
            for (target, slot) in output
                .iter_mut()
                .enumerate()
                .take(opcode.second_end)
                .skip(opcode.second_start)
            {
                let source = opcode.first_start
                    + (target - opcode.second_start).min(old_width.saturating_sub(1));
                *slot = if source < old_codes.len() && !is_image_mark(old_characters[source]) {
                    old_codes[source]
                } else {
                    plain_code
                };
            }
        }
        "insert" => {
            let fill = if opcode.second_start > 0 {
                let donor = opcode.second_start - 1;
                if is_image_mark(new_characters[donor]) {
                    plain_code
                } else {
                    output[donor]
                }
            } else if opcode.first_start < old_codes.len()
                && !is_image_mark(old_characters[opcode.first_start])
            {
                old_codes[opcode.first_start]
            } else {
                plain_code
            };
            output[opcode.second_start..opcode.second_end].fill(fill);
        }
        "delete" => {}
        _ => unreachable!("difflib emitted an unknown opcode"),
    }
}

fn mapped_position(position: usize, old_len: usize, new_len: usize, opcodes: &[Opcode]) -> usize {
    for opcode in opcodes {
        if opcode.first_start <= position && position < opcode.first_end {
            return if opcode.tag == "equal" {
                opcode.second_start + (position - opcode.first_start)
            } else {
                opcode.second_start
            };
        }
        if position == opcode.first_end && opcode.first_end == old_len {
            return opcode.second_end;
        }
    }
    new_len
}

fn is_image_mark(value: char) -> bool {
    matches!(value, HLT_IMAGE_MARK | XML_IMAGE_MARK)
}
