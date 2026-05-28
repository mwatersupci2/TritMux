use crate::trit::block::{
    BlockError, PTryteBlock320, PTryteBlock640, PTRYTES_PER_U320, U640_WORDS,
};
use crate::trit::compact::{compact_strytes_guarded, WindowLockError};
use crate::trit::tryte::{STryte, TRITS_PER_TRYTE};
use std::error::Error;
use std::fmt;

pub const PTRYTES_PER_U640: usize = PTRYTES_PER_U320 * 2;
pub const U640S_PER_ROW9: usize = 9;
pub const ROWS_PER_GRID9: usize = 9;
pub const U640S_PER_GRID9X9: usize = U640S_PER_ROW9 * ROWS_PER_GRID9;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct U640Row9 {
    blocks: [PTryteBlock640; U640S_PER_ROW9],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct U640Grid9x9 {
    rows: [U640Row9; ROWS_PER_GRID9],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZipError {
    InvalidBase3Digit(char),
    Base3Overflow,
    TooManyU640Blocks(usize),
    InsufficientBlocks { needed: usize, available: usize },
    Block(BlockError),
    Window(WindowLockError),
}

pub fn stryte_count_for_trits(trit_len: usize) -> usize {
    trit_len.div_ceil(TRITS_PER_TRYTE)
}

pub fn u640_count_for_strytes(stryte_count: usize) -> usize {
    stryte_count.div_ceil(PTRYTES_PER_U640)
}

pub fn grid9x9_count_for_u640(u640_count: usize) -> usize {
    u640_count.div_ceil(U640S_PER_GRID9X9)
}

pub fn encode_usize_base3(mut value: usize) -> String {
    if value == 0 {
        return "0".to_string();
    }

    let mut digits = Vec::new();
    while value > 0 {
        digits.push(base3_digit_char((value % 3) as u8));
        value /= 3;
    }
    digits.iter().rev().collect()
}

pub fn decode_usize_base3(digits: &str) -> Result<usize, ZipError> {
    let mut value = 0usize;
    for ch in digits.chars() {
        let digit = base3_digit_value(ch)? as usize;
        value = value.checked_mul(3).ok_or(ZipError::Base3Overflow)?;
        value = value.checked_add(digit).ok_or(ZipError::Base3Overflow)?;
    }
    Ok(value)
}

pub fn zip_strytes_to_u640(strytes: &[STryte]) -> Result<Vec<PTryteBlock640>, ZipError> {
    let guarded = compact_strytes_guarded(strytes).map_err(ZipError::Window)?;
    Ok(zip_u320_to_u640(guarded.blocks()))
}

pub fn zip_strytes_to_u640_grids(strytes: &[STryte]) -> Result<Vec<U640Grid9x9>, ZipError> {
    zip_strytes_to_u640(strytes).map(|blocks| zip_u640_to_grids(&blocks))
}

pub fn unzip_u640_to_strytes(
    blocks: &[PTryteBlock640],
    stryte_count: usize,
) -> Result<Vec<STryte>, ZipError> {
    let needed = u640_count_for_strytes(stryte_count);
    if blocks.len() < needed {
        return Err(ZipError::InsufficientBlocks {
            needed,
            available: blocks.len(),
        });
    }

    let mut remaining = stryte_count;
    let mut strytes = Vec::with_capacity(stryte_count);

    for block in blocks.iter().take(needed) {
        let (lower_words, upper_words) = block.split_words();
        for words in [lower_words, upper_words] {
            if remaining == 0 {
                break;
            }

            let ptryte_count = remaining.min(PTRYTES_PER_U320);
            let block320 =
                PTryteBlock320::from_words(words, ptryte_count).map_err(ZipError::Block)?;
            strytes.extend(block320.ptrytes_as_strytes().map_err(ZipError::Block)?);
            remaining -= ptryte_count;
        }
    }

    if remaining > 0 {
        return Err(ZipError::InsufficientBlocks {
            needed,
            available: blocks.len(),
        });
    }

    Ok(strytes)
}

pub fn unzip_u640_grids_to_strytes(
    grids: &[U640Grid9x9],
    stryte_count: usize,
) -> Result<Vec<STryte>, ZipError> {
    let needed_u640 = u640_count_for_strytes(stryte_count);
    let available_u640 = grids.len() * U640S_PER_GRID9X9;
    if available_u640 < needed_u640 {
        return Err(ZipError::InsufficientBlocks {
            needed: needed_u640,
            available: available_u640,
        });
    }

    let blocks: Vec<PTryteBlock640> = grids
        .iter()
        .flat_map(|grid| grid.blocks().take(needed_u640))
        .take(needed_u640)
        .cloned()
        .collect();
    unzip_u640_to_strytes(&blocks, stryte_count)
}

pub fn encode_u640_base3(block: &PTryteBlock640) -> String {
    encode_words_base3(block.words())
}

pub fn decode_u640_base3(digits: &str) -> Result<PTryteBlock640, ZipError> {
    decode_words_base3::<U640_WORDS>(digits).map(PTryteBlock640::from_words)
}

pub fn zip_u640_to_grids(blocks: &[PTryteBlock640]) -> Vec<U640Grid9x9> {
    blocks
        .chunks(U640S_PER_GRID9X9)
        .map(|chunk| {
            U640Grid9x9::from_u640_blocks(chunk)
                .expect("chunks never exceed one 9x9 U640 grid")
        })
        .collect()
}

pub fn encode_grid9x9_base3_lines(grid: &U640Grid9x9) -> Vec<String> {
    grid.blocks().map(encode_u640_base3).collect()
}

pub fn decode_grid9x9_base3_lines(lines: &[&str]) -> Result<U640Grid9x9, ZipError> {
    if lines.len() > U640S_PER_GRID9X9 {
        return Err(ZipError::TooManyU640Blocks(lines.len()));
    }

    let mut blocks = Vec::with_capacity(lines.len());
    for line in lines {
        blocks.push(decode_u640_base3(line)?);
    }
    U640Grid9x9::from_u640_blocks(&blocks)
}

impl U640Row9 {
    pub fn blocks(&self) -> &[PTryteBlock640; U640S_PER_ROW9] {
        &self.blocks
    }
}

impl U640Grid9x9 {
    pub fn from_u640_blocks(blocks: &[PTryteBlock640]) -> Result<Self, ZipError> {
        if blocks.len() > U640S_PER_GRID9X9 {
            return Err(ZipError::TooManyU640Blocks(blocks.len()));
        }

        let rows = std::array::from_fn(|row| U640Row9 {
            blocks: std::array::from_fn(|column| {
                let index = row * U640S_PER_ROW9 + column;
                blocks
                    .get(index)
                    .cloned()
                    .unwrap_or_else(empty_u640_marker_block)
            }),
        });

        Ok(Self { rows })
    }

    pub fn rows(&self) -> &[U640Row9; ROWS_PER_GRID9] {
        &self.rows
    }

    pub fn blocks(&self) -> impl Iterator<Item = &PTryteBlock640> {
        self.rows.iter().flat_map(|row| row.blocks.iter())
    }

    pub fn block_count(&self) -> usize {
        U640S_PER_GRID9X9
    }
}

fn zip_u320_to_u640(blocks: &[PTryteBlock320]) -> Vec<PTryteBlock640> {
    let empty = PTryteBlock320::from_strytes(&[])
        .expect("empty pTryte block is always a valid U320 marker block");
    let mut zipped = Vec::with_capacity(blocks.len().div_ceil(2));

    for pair in blocks.chunks(2) {
        let lower = &pair[0];
        let upper = pair.get(1).unwrap_or(&empty);
        zipped.push(PTryteBlock640::from_pair(lower, upper));
    }

    zipped
}

fn empty_u640_marker_block() -> PTryteBlock640 {
    let empty = PTryteBlock320::from_strytes(&[])
        .expect("empty pTryte block is always a valid U320 marker block");
    PTryteBlock640::from_pair(&empty, &empty)
}

fn encode_words_base3<const N: usize>(words: &[u32; N]) -> String {
    let mut work = *words;
    if work.iter().all(|word| *word == 0) {
        return "0".to_string();
    }

    let mut digits = Vec::new();
    while work.iter().any(|word| *word != 0) {
        let remainder = div_rem_small(&mut work, 3);
        digits.push(base3_digit_char(remainder));
    }

    digits.iter().rev().collect()
}

fn decode_words_base3<const N: usize>(digits: &str) -> Result<[u32; N], ZipError> {
    let mut words = [0u32; N];
    for ch in digits.chars() {
        let digit = base3_digit_value(ch)? as u64;
        let mut carry = digit;

        for word in &mut words {
            let value = (*word as u64) * 3 + carry;
            *word = value as u32;
            carry = value >> 32;
        }

        if carry != 0 {
            return Err(ZipError::Base3Overflow);
        }
    }
    Ok(words)
}

fn div_rem_small(words: &mut [u32], divisor: u32) -> u8 {
    let mut carry = 0u64;
    for word in words.iter_mut().rev() {
        let value = (carry << 32) | *word as u64;
        *word = (value / divisor as u64) as u32;
        carry = value % divisor as u64;
    }
    carry as u8
}

fn base3_digit_char(digit: u8) -> char {
    match digit {
        0 => '0',
        1 => '1',
        2 => '2',
        _ => unreachable!("base3 digit must be 0, 1, or 2"),
    }
}

fn base3_digit_value(ch: char) -> Result<u8, ZipError> {
    match ch {
        '0' => Ok(0),
        '1' => Ok(1),
        '2' => Ok(2),
        other => Err(ZipError::InvalidBase3Digit(other)),
    }
}

impl fmt::Display for ZipError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ZipError::InvalidBase3Digit(ch) => write!(f, "invalid base3 digit '{ch}'"),
            ZipError::Base3Overflow => write!(f, "base3 integer exceeds U640 capacity"),
            ZipError::TooManyU640Blocks(count) => {
                write!(f, "cannot store {count} U640 blocks in one 9x9 grid")
            }
            ZipError::InsufficientBlocks { needed, available } => write!(
                f,
                "not enough zipped U640 blocks; needed {needed}, available {available}"
            ),
            ZipError::Block(err) => write!(f, "{err}"),
            ZipError::Window(err) => write!(f, "{err}"),
        }
    }
}

impl Error for ZipError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trit::core::Trit;

    #[test]
    fn usize_base3_round_trips() {
        for value in [0usize, 1, 2, 3, 10, 243, 65_535, usize::MAX / 3] {
            let encoded = encode_usize_base3(value);
            assert!(encoded.chars().all(|ch| matches!(ch, '0' | '1' | '2')));
            assert_eq!(decode_usize_base3(&encoded).unwrap(), value);
        }
    }

    #[test]
    fn u640_base3_round_trips_without_bigints() {
        let lower = PTryteBlock320::from_strytes(&[STryte::zero()]).unwrap();
        let upper = PTryteBlock320::from_strytes(&[STryte::from_trits(
            [Trit::Positive; TRITS_PER_TRYTE],
        )])
        .unwrap();
        let block = PTryteBlock640::from_pair(&lower, &upper);

        let encoded = encode_u640_base3(&block);
        assert!(encoded.chars().all(|ch| matches!(ch, '0' | '1' | '2')));
        assert_eq!(decode_u640_base3(&encoded).unwrap(), block);
    }

    #[test]
    fn strytes_zip_to_u640_and_unzip_back() {
        let strytes: Vec<STryte> = (0..40)
            .map(|index| {
                let trit = match index % 3 {
                    0 => Trit::Negative,
                    1 => Trit::Neutral,
                    _ => Trit::Positive,
                };
                STryte::from_trits([trit; TRITS_PER_TRYTE])
            })
            .collect();

        let zipped = zip_strytes_to_u640(&strytes).unwrap();
        assert_eq!(zipped.len(), 2);
        assert_eq!(unzip_u640_to_strytes(&zipped, strytes.len()).unwrap(), strytes);
    }

    #[test]
    fn u640_blocks_zip_into_9_by_9_grids() {
        let strytes: Vec<STryte> = (0..2600)
            .map(|index| {
                let trit = match index % 3 {
                    0 => Trit::Negative,
                    1 => Trit::Neutral,
                    _ => Trit::Positive,
                };
                STryte::from_trits([trit; TRITS_PER_TRYTE])
            })
            .collect();

        let grids = zip_strytes_to_u640_grids(&strytes).unwrap();
        assert_eq!(grids.len(), 2);
        assert_eq!(
            unzip_u640_grids_to_strytes(&grids, strytes.len()).unwrap(),
            strytes
        );
    }

    #[test]
    fn grid_lines_are_base3_encoded_u640_integers() {
        let strytes = vec![STryte::zero()];
        let grid = zip_strytes_to_u640_grids(&strytes).unwrap().remove(0);
        let lines = encode_grid9x9_base3_lines(&grid);
        assert_eq!(lines.len(), U640S_PER_GRID9X9);
        assert!(lines
            .iter()
            .all(|line| line.chars().all(|ch| matches!(ch, '0' | '1' | '2'))));

        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        assert_eq!(decode_grid9x9_base3_lines(&refs).unwrap(), grid);
    }
}
