use crate::trit::block::PTryteBlock320;
use crate::trit::compact::{compact_strytes_guarded, GuardedCompaction, WindowLockError};
use crate::trit::core::{Trit, TritError};
use crate::trit::tryte::{STryte, TRITS_PER_TRYTE};
use crate::trit::zip::{zip_strytes_to_u640_grids, U640Grid9x9, ZipError};
use std::error::Error;
use std::fmt;
use std::string::FromUtf8Error;

pub const TRITS_PER_BYTE: usize = 6;
const BYTE_TRIT_WEIGHTS: [u16; TRITS_PER_BYTE] = [243, 81, 27, 9, 3, 1];

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TritPayload {
    strytes: Vec<STryte>,
    trit_len: usize,
}

#[derive(Debug)]
pub enum PayloadError {
    Trit(TritError),
    InvalidTritLength(usize),
    InvalidSTryteCapacity {
        trit_len: usize,
        stryte_count: usize,
    },
    InvalidByteValue(u16),
    Utf8(FromUtf8Error),
}

impl TritPayload {
    pub fn new() -> Self {
        Self {
            strytes: Vec::new(),
            trit_len: 0,
        }
    }

    pub fn from_trits(trits: Vec<Trit>) -> Self {
        let mut strytes = Vec::with_capacity((trits.len() + TRITS_PER_TRYTE - 1) / TRITS_PER_TRYTE);

        for chunk in trits.chunks(TRITS_PER_TRYTE) {
            let mut lanes = [Trit::Neutral; TRITS_PER_TRYTE];
            for (index, trit) in chunk.iter().enumerate() {
                lanes[index] = *trit;
            }
            strytes.push(STryte::from_trits(lanes));
        }

        Self {
            strytes,
            trit_len: trits.len(),
        }
    }

    pub fn from_strytes(strytes: Vec<STryte>, trit_len: usize) -> Result<Self, PayloadError> {
        if trit_len > strytes.len() * TRITS_PER_TRYTE {
            return Err(PayloadError::InvalidSTryteCapacity {
                trit_len,
                stryte_count: strytes.len(),
            });
        }

        Ok(Self { strytes, trit_len })
    }

    pub fn from_bytes(bytes: &[u8]) -> Self {
        let mut trits = Vec::with_capacity(bytes.len() * TRITS_PER_BYTE);
        for byte in bytes {
            trits.extend(encode_byte(*byte));
        }
        Self::from_trits(trits)
    }

    pub fn from_utf8(text: &str) -> Self {
        Self::from_bytes(text.as_bytes())
    }

    pub fn from_storage_digits(digits: &str) -> Result<Self, PayloadError> {
        let mut trits = Vec::with_capacity(digits.len());
        for ch in digits.chars() {
            trits.push(Trit::from_storage_char(ch).map_err(PayloadError::Trit)?);
        }
        Ok(Self::from_trits(trits))
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, PayloadError> {
        let trits = self.trits();
        if trits.len() % TRITS_PER_BYTE != 0 {
            return Err(PayloadError::InvalidTritLength(trits.len()));
        }

        let mut bytes = Vec::with_capacity(trits.len() / TRITS_PER_BYTE);
        for chunk in trits.chunks_exact(TRITS_PER_BYTE) {
            bytes.push(decode_byte(chunk)?);
        }
        Ok(bytes)
    }

    pub fn to_utf8_string(&self) -> Result<String, PayloadError> {
        String::from_utf8(self.to_bytes()?).map_err(PayloadError::Utf8)
    }

    pub fn as_storage_digits(&self) -> String {
        self.trits()
            .iter()
            .map(|trit| trit.storage_char())
            .collect()
    }

    pub fn len_trits(&self) -> usize {
        self.trit_len
    }

    pub fn len_strytes(&self) -> usize {
        self.strytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.trit_len == 0
    }

    pub fn strytes(&self) -> &[STryte] {
        &self.strytes
    }

    pub fn trits(&self) -> Vec<Trit> {
        let mut trits = Vec::with_capacity(self.trit_len);
        for stryte in &self.strytes {
            for trit in stryte.trits() {
                if trits.len() == self.trit_len {
                    return trits;
                }
                trits.push(trit);
            }
        }
        trits
    }

    pub fn ptryte_blocks(&self) -> Vec<PTryteBlock320> {
        PTryteBlock320::pack_many(&self.strytes)
    }

    pub fn guarded_compaction(&self) -> Result<GuardedCompaction, WindowLockError> {
        compact_strytes_guarded(&self.strytes)
    }

    pub fn zipped_u640_grids(&self) -> Result<Vec<U640Grid9x9>, ZipError> {
        zip_strytes_to_u640_grids(&self.strytes)
    }
}

pub fn encode_byte(byte: u8) -> [Trit; TRITS_PER_BYTE] {
    let mut value = byte as u16;
    let mut trits = [Trit::Negative; TRITS_PER_BYTE];

    for (index, weight) in BYTE_TRIT_WEIGHTS.iter().enumerate() {
        let digit = (value / weight) as u8;
        value %= weight;
        trits[index] =
            Trit::from_storage_digit(digit).expect("byte encoding weights produce base-3 digits");
    }

    trits
}

pub fn decode_byte(trits: &[Trit]) -> Result<u8, PayloadError> {
    if trits.len() != TRITS_PER_BYTE {
        return Err(PayloadError::InvalidTritLength(trits.len()));
    }

    let mut value = 0u16;
    for (trit, weight) in trits.iter().zip(BYTE_TRIT_WEIGHTS) {
        value += trit.storage_digit() as u16 * weight;
    }

    if value > u8::MAX as u16 {
        return Err(PayloadError::InvalidByteValue(value));
    }

    Ok(value as u8)
}

impl fmt::Display for PayloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PayloadError::Trit(err) => write!(f, "{err}"),
            PayloadError::InvalidTritLength(len) => {
                write!(f, "invalid trit payload length {len}; expected a multiple of 6")
            }
            PayloadError::InvalidSTryteCapacity {
                trit_len,
                stryte_count,
            } => write!(
                f,
                "trit length {trit_len} exceeds {stryte_count} sTryte capacity"
            ),
            PayloadError::InvalidByteValue(value) => {
                write!(f, "trit group decodes to invalid byte value {value}")
            }
            PayloadError::Utf8(err) => write!(f, "invalid UTF-8 payload: {err}"),
        }
    }
}

impl Error for PayloadError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_bytes_round_trip_through_trits() {
        let bytes: Vec<u8> = (0..=u8::MAX).collect();
        let payload = TritPayload::from_bytes(&bytes);
        assert_eq!(payload.len_trits(), 256 * TRITS_PER_BYTE);
        assert_eq!(payload.to_bytes().unwrap(), bytes);
    }

    #[test]
    fn text_round_trips_through_storage_digits() {
        let payload = TritPayload::from_utf8("hello trinary notes");
        let digits = payload.as_storage_digits();
        assert!(digits.chars().all(|ch| matches!(ch, '0' | '1' | '2')));
        assert_eq!(
            TritPayload::from_storage_digits(&digits)
                .unwrap()
                .to_utf8_string()
                .unwrap(),
            "hello trinary notes"
        );
    }

    #[test]
    fn payload_uses_strytes_in_ram() {
        let payload = TritPayload::from_utf8("abc");
        assert_eq!(payload.len_trits(), 18);
        assert_eq!(payload.len_strytes(), 2);
        assert!(payload.strytes().iter().all(|stryte| stryte.has_u32_marker()));
    }

    #[test]
    fn payload_can_compact_strytes_into_ptryte_blocks() {
        let payload = TritPayload::from_utf8("thirty-two-byte-ish payload sample");
        let blocks = payload.ptryte_blocks();
        assert!(!blocks.is_empty());
        assert_eq!(blocks[0].ptrytes_as_strytes().unwrap()[0], payload.strytes()[0]);
    }

    #[test]
    fn payload_compaction_uses_sliding_lock_windows() {
        let payload = TritPayload::from_utf8("guarded payload");
        let compaction = payload.guarded_compaction().unwrap();
        assert_eq!(compaction.windows().len(), payload.len_strytes());
        assert_eq!(compaction.blocks()[0].ptrytes_as_strytes().unwrap()[0], payload.strytes()[0]);
    }

    #[test]
    fn payload_zips_into_9_by_9_u640_grids() {
        let payload = TritPayload::from_utf8("grid zipped payload");
        let grids = payload.zipped_u640_grids().unwrap();
        assert_eq!(grids.len(), 1);
        assert_eq!(grids[0].block_count(), 81);
    }
}
