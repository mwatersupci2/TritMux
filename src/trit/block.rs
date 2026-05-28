use crate::trit::core::{Trit, TRIT_LANE_BITS, TRIT_LANE_MARKER, TRIT_LANE_MASK};
use crate::trit::tryte::{STryte, TRITS_PER_TRYTE};
use std::error::Error;
use std::fmt;

pub const U320_WORDS: usize = 10;
pub const U640_WORDS: usize = 20;
pub const PTRYTES_PER_U320: usize = 16;
pub const PTRYTE_GAP_LANES: usize = 1;
pub const LANES_PER_PTRYTE: usize = TRITS_PER_TRYTE + PTRYTE_GAP_LANES;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PTryteBlock320 {
    words: [u32; U320_WORDS],
    ptryte_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PTryteBlock640 {
    words: [u32; U640_WORDS],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockError {
    TooManySTrytes(usize),
    InvalidPTryteIndex(usize),
    InvalidPTryteLane {
        ptryte_index: usize,
        lane_index: usize,
        bits: u32,
    },
    MissingGapMarker(usize),
}

impl PTryteBlock320 {
    pub fn from_strytes(strytes: &[STryte]) -> Result<Self, BlockError> {
        if strytes.len() > PTRYTES_PER_U320 {
            return Err(BlockError::TooManySTrytes(strytes.len()));
        }

        let mut block = Self {
            words: [u32::MAX; U320_WORDS],
            ptryte_count: strytes.len(),
        };

        for (slot, stryte) in strytes.iter().enumerate() {
            let trits = stryte.trits();
            for (lane, trit) in trits.iter().enumerate() {
                block.write_lane(slot * LANES_PER_PTRYTE + lane, trit.lane_bits());
            }
            block.write_lane(slot * LANES_PER_PTRYTE + TRITS_PER_TRYTE, TRIT_LANE_MARKER);
        }

        Ok(block)
    }

    pub fn from_words(words: [u32; U320_WORDS], ptryte_count: usize) -> Result<Self, BlockError> {
        if ptryte_count > PTRYTES_PER_U320 {
            return Err(BlockError::TooManySTrytes(ptryte_count));
        }

        let block = Self {
            words,
            ptryte_count,
        };

        for index in 0..ptryte_count {
            block.ptryte_as_stryte(index)?;
        }

        Ok(block)
    }

    pub fn pack_many(strytes: &[STryte]) -> Vec<Self> {
        strytes
            .chunks(PTRYTES_PER_U320)
            .map(|chunk| {
                Self::from_strytes(chunk)
                    .expect("chunks never exceed the U320 pTryte packing capacity")
            })
            .collect()
    }

    pub fn words(&self) -> &[u32; U320_WORDS] {
        &self.words
    }

    pub fn ptryte_count(&self) -> usize {
        self.ptryte_count
    }

    pub fn ptryte_as_stryte(&self, index: usize) -> Result<STryte, BlockError> {
        if index >= self.ptryte_count {
            return Err(BlockError::InvalidPTryteIndex(index));
        }

        let gap_lane = self.read_lane(index * LANES_PER_PTRYTE + TRITS_PER_TRYTE);
        if gap_lane != TRIT_LANE_MARKER {
            return Err(BlockError::MissingGapMarker(index));
        }

        let mut trits = [Trit::Neutral; TRITS_PER_TRYTE];
        for (lane, slot) in trits.iter_mut().enumerate() {
            let bits = self.read_lane(index * LANES_PER_PTRYTE + lane);
            *slot = Trit::from_lane_bits(bits).map_err(|_| BlockError::InvalidPTryteLane {
                ptryte_index: index,
                lane_index: lane,
                bits,
            })?;
        }

        Ok(STryte::from_trits(trits))
    }

    pub fn ptrytes_as_strytes(&self) -> Result<Vec<STryte>, BlockError> {
        (0..self.ptryte_count)
            .map(|index| self.ptryte_as_stryte(index))
            .collect()
    }

    pub fn gap_lane_bits(&self, index: usize) -> Result<u32, BlockError> {
        if index >= self.ptryte_count {
            return Err(BlockError::InvalidPTryteIndex(index));
        }
        Ok(self.read_lane(index * LANES_PER_PTRYTE + TRITS_PER_TRYTE))
    }

    fn write_lane(&mut self, lane_index: usize, bits: u32) {
        let word_index = lane_index / 16;
        let bit_shift = ((lane_index % 16) * TRIT_LANE_BITS) as u32;
        let mask = TRIT_LANE_MASK << bit_shift;
        self.words[word_index] =
            (self.words[word_index] & !mask) | ((bits & TRIT_LANE_MASK) << bit_shift);
    }

    fn read_lane(&self, lane_index: usize) -> u32 {
        let word_index = lane_index / 16;
        let bit_shift = ((lane_index % 16) * TRIT_LANE_BITS) as u32;
        (self.words[word_index] >> bit_shift) & TRIT_LANE_MASK
    }
}

impl PTryteBlock640 {
    pub fn from_pair(lower: &PTryteBlock320, upper: &PTryteBlock320) -> Self {
        let mut words = [u32::MAX; U640_WORDS];
        words[..U320_WORDS].copy_from_slice(lower.words());
        words[U320_WORDS..].copy_from_slice(upper.words());
        Self { words }
    }

    pub fn from_words(words: [u32; U640_WORDS]) -> Self {
        Self { words }
    }

    pub fn words(&self) -> &[u32; U640_WORDS] {
        &self.words
    }

    pub fn split_words(&self) -> ([u32; U320_WORDS], [u32; U320_WORDS]) {
        let mut lower = [u32::MAX; U320_WORDS];
        let mut upper = [u32::MAX; U320_WORDS];
        lower.copy_from_slice(&self.words[..U320_WORDS]);
        upper.copy_from_slice(&self.words[U320_WORDS..]);
        (lower, upper)
    }
}

impl fmt::Display for BlockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BlockError::TooManySTrytes(count) => {
                write!(f, "cannot pack {count} sTrytes into one U320 pTryte block")
            }
            BlockError::InvalidPTryteIndex(index) => write!(f, "invalid pTryte index {index}"),
            BlockError::InvalidPTryteLane {
                ptryte_index,
                lane_index,
                bits,
            } => write!(
                f,
                "invalid pTryte lane bits {bits:b} at pTryte {ptryte_index}, lane {lane_index}"
            ),
            BlockError::MissingGapMarker(index) => {
                write!(f, "missing compacted gap marker after pTryte {index}")
            }
        }
    }
}

impl Error for BlockError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u320_packs_sixteen_ptrytes_with_gap_lanes() {
        let strytes: Vec<STryte> = (0..PTRYTES_PER_U320)
            .map(|index| {
                let trit = match index % 3 {
                    0 => Trit::Negative,
                    1 => Trit::Neutral,
                    _ => Trit::Positive,
                };
                STryte::from_trits([trit; TRITS_PER_TRYTE])
            })
            .collect();

        let block = PTryteBlock320::from_strytes(&strytes).unwrap();
        assert_eq!(block.ptryte_count(), PTRYTES_PER_U320);
        assert_eq!(block.ptrytes_as_strytes().unwrap(), strytes);

        for index in 0..PTRYTES_PER_U320 {
            assert_eq!(block.gap_lane_bits(index).unwrap(), TRIT_LANE_MARKER);
        }
    }

    #[test]
    fn u320_blocks_keep_unused_lanes_marked() {
        let block = PTryteBlock320::from_strytes(&[STryte::zero()]).unwrap();
        assert_eq!(block.ptryte_count(), 1);
        assert_eq!(block.words()[1], u32::MAX);
    }

    #[test]
    fn u640_combines_two_u320_blocks_without_large_ints() {
        let lower = PTryteBlock320::from_strytes(&[STryte::zero()]).unwrap();
        let upper = PTryteBlock320::from_strytes(&[STryte::from_trits(
            [Trit::Positive; TRITS_PER_TRYTE],
        )])
        .unwrap();

        let combined = PTryteBlock640::from_pair(&lower, &upper);
        assert_eq!(&combined.words()[..U320_WORDS], lower.words());
        assert_eq!(&combined.words()[U320_WORDS..], upper.words());
    }

    #[test]
    fn u320_can_rehydrate_from_words_with_known_ptryte_count() {
        let strytes = [
            STryte::zero(),
            STryte::from_trits([Trit::Positive; TRITS_PER_TRYTE]),
        ];
        let block = PTryteBlock320::from_strytes(&strytes).unwrap();
        let restored = PTryteBlock320::from_words(*block.words(), strytes.len()).unwrap();
        assert_eq!(restored.ptrytes_as_strytes().unwrap(), strytes);
    }
}
