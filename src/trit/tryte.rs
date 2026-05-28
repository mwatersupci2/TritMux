use crate::trit::core::{Trit, TritError, TRIT_LANE_BITS, TRIT_LANE_MARKER, TRIT_LANE_MASK};
use std::error::Error;
use std::fmt;

pub const TRITS_PER_TRYTE: usize = 9;
pub const STRYTE_DATA_BITS: usize = TRITS_PER_TRYTE * TRIT_LANE_BITS;
pub const STRYTE_DATA_MASK: u32 = (1u32 << STRYTE_DATA_BITS) - 1;
pub const STRYTE_MARKER_MASK: u32 = !STRYTE_DATA_MASK;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct STryte {
    word: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum STryteError {
    MissingMarkerBits(u32),
    MarkerLane(usize),
    Trit(TritError),
}

impl STryte {
    pub fn zero() -> Self {
        Self::from_trits([Trit::Neutral; TRITS_PER_TRYTE])
    }

    pub fn from_trits(trits: [Trit; TRITS_PER_TRYTE]) -> Self {
        let mut word = STRYTE_MARKER_MASK;
        for (index, trit) in trits.iter().enumerate() {
            let shift = (index * TRIT_LANE_BITS) as u32;
            word |= trit.lane_bits() << shift;
        }
        Self { word }
    }

    pub fn from_word(word: u32) -> Result<Self, STryteError> {
        if word & STRYTE_MARKER_MASK != STRYTE_MARKER_MASK {
            return Err(STryteError::MissingMarkerBits(word));
        }

        for index in 0..TRITS_PER_TRYTE {
            let bits = lane_bits_at(word, index);
            if bits == TRIT_LANE_MARKER {
                return Err(STryteError::MarkerLane(index));
            }
            Trit::from_lane_bits(bits).map_err(STryteError::Trit)?;
        }

        Ok(Self { word })
    }

    pub fn word(self) -> u32 {
        self.word
    }

    pub fn trit(&self, index: usize) -> Option<Trit> {
        if index >= TRITS_PER_TRYTE {
            return None;
        }
        Trit::from_lane_bits(lane_bits_at(self.word, index)).ok()
    }

    pub fn trits(&self) -> [Trit; TRITS_PER_TRYTE] {
        let mut trits = [Trit::Neutral; TRITS_PER_TRYTE];
        for (index, slot) in trits.iter_mut().enumerate() {
            *slot = self
                .trit(index)
                .expect("sTryte construction prevents marker lanes in data");
        }
        trits
    }

    pub fn has_u32_marker(self) -> bool {
        self.word & STRYTE_MARKER_MASK == STRYTE_MARKER_MASK
    }

    pub fn as_storage_digits(&self) -> String {
        self.trits()
            .iter()
            .map(|trit| trit.storage_char())
            .collect()
    }
}

fn lane_bits_at(word: u32, index: usize) -> u32 {
    let shift = (index * TRIT_LANE_BITS) as u32;
    (word >> shift) & TRIT_LANE_MASK
}

impl fmt::Display for STryteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            STryteError::MissingMarkerBits(word) => {
                write!(f, "sTryte 0x{word:08x} is missing u32 marker bits")
            }
            STryteError::MarkerLane(index) => {
                write!(f, "sTryte data lane {index} is a marker")
            }
            STryteError::Trit(err) => write!(f, "{err}"),
        }
    }
}

impl Error for STryteError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_stryte_uses_neutral_trits() {
        assert_eq!(STryte::zero().as_storage_digits(), "111111111");
    }

    #[test]
    fn stryte_sets_high_marker_bits() {
        let stryte = STryte::zero();
        assert!(stryte.has_u32_marker());
        assert_eq!(stryte.word() & STRYTE_MARKER_MASK, STRYTE_MARKER_MASK);
        assert_eq!(STryte::from_word(stryte.word()).unwrap(), stryte);
    }

    #[test]
    fn stryte_rejects_marker_lane_inside_data() {
        let word = STRYTE_MARKER_MASK | TRIT_LANE_MARKER;
        assert!(matches!(
            STryte::from_word(word),
            Err(STryteError::MarkerLane(0))
        ));
    }
}
