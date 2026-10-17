const MAX_BITS: usize = 15;
const MAX_OUTPUT_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    Stored,
    Fixed,
    Dynamic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeflateTrace {
    pub first_block_kind: BlockKind,
    pub first_hdist: u16,
    pub max_distance: usize,
    pub output_bytes: usize,
}

pub fn trace_raw(stream: &[u8]) -> Result<DeflateTrace, String> {
    let mut bits = BitReader::new(stream);
    let mut first = None;
    let mut max_distance = 0_usize;
    let mut output_bytes = 0_usize;

    loop {
        let final_block = bits.read_bits(1)? != 0;
        let block_type = bits.read_bits(2)?;
        let (kind, first_hdist) = match block_type {
            0 => {
                bits.align_to_byte();
                let length = usize::from(bits.read_u16_le()?);
                let complement = bits.read_u16_le()?;
                if complement != !(length as u16) {
                    return Err("stored length complement mismatch".to_string());
                }
                bits.skip_bytes(length)?;
                output_bytes = bounded_add(output_bytes, length)?;
                (BlockKind::Stored, 0)
            }
            1 => {
                let literal_lengths = fixed_literal_lengths();
                let distance_lengths = vec![5; 32];
                decode_huffman_block(
                    &mut bits,
                    &literal_lengths,
                    &distance_lengths,
                    &mut output_bytes,
                    &mut max_distance,
                )?;
                (BlockKind::Fixed, 32)
            }
            2 => {
                let hlit = usize::from(bits.read_bits(5)?) + 257;
                let hdist = usize::from(bits.read_bits(5)?) + 1;
                let hclen = usize::from(bits.read_bits(4)?) + 4;
                let (literal_lengths, distance_lengths) =
                    read_dynamic_trees(&mut bits, hlit, hdist, hclen)?;
                decode_huffman_block(
                    &mut bits,
                    &literal_lengths,
                    &distance_lengths,
                    &mut output_bytes,
                    &mut max_distance,
                )?;
                (BlockKind::Dynamic, hdist as u16)
            }
            _ => return Err("reserved DEFLATE block type".to_string()),
        };

        if first.is_none() {
            first = Some((kind, first_hdist));
        }
        if final_block {
            break;
        }
    }

    let (first_block_kind, first_hdist) = first.ok_or("missing DEFLATE block")?;
    Ok(DeflateTrace {
        first_block_kind,
        first_hdist,
        max_distance,
        output_bytes,
    })
}

fn read_dynamic_trees(
    bits: &mut BitReader<'_>,
    hlit: usize,
    hdist: usize,
    hclen: usize,
) -> Result<(Vec<u8>, Vec<u8>), String> {
    const ORDER: [usize; 19] = [
        16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
    ];
    let mut code_lengths = [0_u8; 19];
    for symbol in ORDER.into_iter().take(hclen) {
        code_lengths[symbol] = bits.read_bits(3)? as u8;
    }
    let code_tree = Huffman::new(&code_lengths)?;
    let total = hlit
        .checked_add(hdist)
        .ok_or("dynamic tree length overflow")?;
    let mut lengths = Vec::with_capacity(total);
    while lengths.len() < total {
        match code_tree.decode(bits)? {
            value @ 0..=15 => lengths.push(value as u8),
            16 => {
                let previous = *lengths.last().ok_or("repeat without previous length")?;
                let repeat = usize::from(bits.read_bits(2)?) + 3;
                push_repeated(&mut lengths, total, previous, repeat)?;
            }
            17 => {
                let repeat = usize::from(bits.read_bits(3)?) + 3;
                push_repeated(&mut lengths, total, 0, repeat)?;
            }
            18 => {
                let repeat = usize::from(bits.read_bits(7)?) + 11;
                push_repeated(&mut lengths, total, 0, repeat)?;
            }
            _ => return Err("invalid code-length symbol".to_string()),
        }
    }
    if lengths.get(256).copied().unwrap_or_default() == 0 {
        return Err("missing end-of-block symbol".to_string());
    }
    Ok((lengths[..hlit].to_vec(), lengths[hlit..].to_vec()))
}

fn push_repeated(
    lengths: &mut Vec<u8>,
    total: usize,
    value: u8,
    repeat: usize,
) -> Result<(), String> {
    if lengths.len().saturating_add(repeat) > total {
        return Err("code-length repeat exceeds tree".to_string());
    }
    lengths.extend(std::iter::repeat_n(value, repeat));
    Ok(())
}

fn decode_huffman_block(
    bits: &mut BitReader<'_>,
    literal_lengths: &[u8],
    distance_lengths: &[u8],
    output_bytes: &mut usize,
    max_distance: &mut usize,
) -> Result<(), String> {
    const LENGTH_BASE: [usize; 29] = [
        3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115,
        131, 163, 195, 227, 258,
    ];
    const LENGTH_EXTRA: [u8; 29] = [
        0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
    ];
    const DISTANCE_BASE: [usize; 30] = [
        1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
        2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
    ];
    const DISTANCE_EXTRA: [u8; 30] = [
        0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12,
        13, 13,
    ];

    let literal_tree = Huffman::new(literal_lengths)?;
    let distance_tree = Huffman::new(distance_lengths)?;
    loop {
        let symbol = literal_tree.decode(bits)?;
        match symbol {
            0..=255 => *output_bytes = bounded_add(*output_bytes, 1)?,
            256 => return Ok(()),
            257..=285 => {
                let length_index = symbol - 257;
                let length = LENGTH_BASE[length_index]
                    + usize::from(bits.read_bits(usize::from(LENGTH_EXTRA[length_index]))?);
                let distance_symbol = distance_tree.decode(bits)?;
                if distance_symbol >= DISTANCE_BASE.len() {
                    return Err("reserved distance symbol".to_string());
                }
                let distance = DISTANCE_BASE[distance_symbol]
                    + usize::from(bits.read_bits(usize::from(DISTANCE_EXTRA[distance_symbol]))?);
                if distance == 0 || distance > *output_bytes {
                    return Err("distance exceeds decoded output".to_string());
                }
                *max_distance = (*max_distance).max(distance);
                *output_bytes = bounded_add(*output_bytes, length)?;
            }
            _ => return Err("reserved literal/length symbol".to_string()),
        }
    }
}

fn bounded_add(current: usize, added: usize) -> Result<usize, String> {
    let next = current.checked_add(added).ok_or("output length overflow")?;
    if next > MAX_OUTPUT_BYTES {
        return Err("trace output limit exceeded".to_string());
    }
    Ok(next)
}

fn fixed_literal_lengths() -> Vec<u8> {
    let mut lengths = vec![0_u8; 288];
    lengths[0..=143].fill(8);
    lengths[144..=255].fill(9);
    lengths[256..=279].fill(7);
    lengths[280..=287].fill(8);
    lengths
}

struct Huffman {
    entries: Vec<(u16, u8, usize)>,
    max_bits: u8,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Result<Self, String> {
        let mut counts = [0_u16; MAX_BITS + 1];
        for &length in lengths {
            if usize::from(length) > MAX_BITS {
                return Err("Huffman code exceeds fifteen bits".to_string());
            }
            if length != 0 {
                counts[usize::from(length)] += 1;
            }
        }
        let max_bits = lengths.iter().copied().max().unwrap_or(0);
        if max_bits == 0 {
            return Err("empty Huffman tree".to_string());
        }
        let mut next_code = [0_u16; MAX_BITS + 1];
        let mut code = 0_u16;
        for bit_length in 1..=MAX_BITS {
            code = (code + counts[bit_length - 1]) << 1;
            next_code[bit_length] = code;
        }
        let mut entries = Vec::new();
        for (symbol, &length) in lengths.iter().enumerate() {
            if length == 0 {
                continue;
            }
            let canonical = next_code[usize::from(length)];
            next_code[usize::from(length)] += 1;
            entries.push((reverse_bits(canonical, length), length, symbol));
        }
        Ok(Self { entries, max_bits })
    }

    fn decode(&self, bits: &mut BitReader<'_>) -> Result<usize, String> {
        let mut code = 0_u16;
        for length in 1..=self.max_bits {
            code |= bits.read_bits(1)? << (length - 1);
            if let Some((_, _, symbol)) = self
                .entries
                .iter()
                .find(|(candidate, bits, _)| *bits == length && *candidate == code)
            {
                return Ok(*symbol);
            }
        }
        Err("invalid Huffman code".to_string())
    }
}

fn reverse_bits(mut value: u16, length: u8) -> u16 {
    let mut reversed = 0_u16;
    for _ in 0..length {
        reversed = (reversed << 1) | (value & 1);
        value >>= 1;
    }
    reversed
}

struct BitReader<'a> {
    bytes: &'a [u8],
    bit_offset: usize,
}

impl<'a> BitReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            bit_offset: 0,
        }
    }

    fn read_bits(&mut self, count: usize) -> Result<u16, String> {
        if count > 16 || self.bit_offset.saturating_add(count) > self.bytes.len() * 8 {
            return Err("truncated DEFLATE stream".to_string());
        }
        let mut value = 0_u16;
        for output_bit in 0..count {
            let input_bit = self.bit_offset + output_bit;
            let bit = (self.bytes[input_bit / 8] >> (input_bit % 8)) & 1;
            value |= u16::from(bit) << output_bit;
        }
        self.bit_offset += count;
        Ok(value)
    }

    fn align_to_byte(&mut self) {
        self.bit_offset = (self.bit_offset + 7) & !7;
    }

    fn read_u16_le(&mut self) -> Result<u16, String> {
        if !self.bit_offset.is_multiple_of(8) {
            return Err("unaligned stored block".to_string());
        }
        let start = self.bit_offset / 8;
        let bytes = self
            .bytes
            .get(start..start + 2)
            .ok_or("truncated stored block")?;
        self.bit_offset += 16;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn skip_bytes(&mut self, count: usize) -> Result<(), String> {
        if !self.bit_offset.is_multiple_of(8) {
            return Err("unaligned stored block payload".to_string());
        }
        let bits = count.checked_mul(8).ok_or("stored length overflow")?;
        if self.bit_offset.saturating_add(bits) > self.bytes.len() * 8 {
            return Err("truncated stored block payload".to_string());
        }
        self.bit_offset += bits;
        Ok(())
    }
}
