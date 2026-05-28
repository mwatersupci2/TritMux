use std::error::Error;
use std::fmt;

pub const TRIT_LANE_BITS: usize = 2;
pub const TRIT_LANE_MASK: u32 = 0b11;
pub const TRIT_LANE_BOUNDARY_MISC: u32 = 0b11;
pub const TRIT_LANE_MARKER: u32 = TRIT_LANE_BOUNDARY_MISC;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Trit {
    Negative,
    Neutral,
    Positive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TritLane {
    Low,
    Middle,
    High,
    BoundaryOrMisc,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TritError {
    InvalidDigit(char),
    InvalidBalancedValue(i8),
    InvalidStorageDigit(u8),
    InvalidLaneBits(u32),
    BoundaryOrMiscLane,
}

impl Trit {
    pub fn balanced_value(self) -> i8 {
        match self {
            Trit::Negative => -1,
            Trit::Neutral => 0,
            Trit::Positive => 1,
        }
    }

    pub fn from_balanced_value(value: i8) -> Result<Self, TritError> {
        match value {
            -1 => Ok(Trit::Negative),
            0 => Ok(Trit::Neutral),
            1 => Ok(Trit::Positive),
            other => Err(TritError::InvalidBalancedValue(other)),
        }
    }

    pub fn storage_digit(self) -> u8 {
        match self {
            Trit::Negative => 0,
            Trit::Neutral => 1,
            Trit::Positive => 2,
        }
    }

    pub fn from_storage_digit(digit: u8) -> Result<Self, TritError> {
        match digit {
            0 => Ok(Trit::Negative),
            1 => Ok(Trit::Neutral),
            2 => Ok(Trit::Positive),
            other => Err(TritError::InvalidStorageDigit(other)),
        }
    }

    pub fn storage_char(self) -> char {
        match self {
            Trit::Negative => '0',
            Trit::Neutral => '1',
            Trit::Positive => '2',
        }
    }

    pub fn lane_bits(self) -> u32 {
        self.lane().bits()
    }

    pub fn lane(self) -> TritLane {
        match self {
            Trit::Negative => TritLane::Low,
            Trit::Neutral => TritLane::Middle,
            Trit::Positive => TritLane::High,
        }
    }

    pub fn from_lane_bits(bits: u32) -> Result<Self, TritError> {
        TritLane::from_bits(bits)?.to_trit()
    }

    pub fn from_storage_char(ch: char) -> Result<Self, TritError> {
        match ch {
            '0' => Ok(Trit::Negative),
            '1' => Ok(Trit::Neutral),
            '2' => Ok(Trit::Positive),
            other => Err(TritError::InvalidDigit(other)),
        }
    }
}

impl TritLane {
    pub fn bits(self) -> u32 {
        match self {
            TritLane::Low => 0b00,
            TritLane::Middle => 0b01,
            TritLane::High => 0b10,
            TritLane::BoundaryOrMisc => TRIT_LANE_BOUNDARY_MISC,
        }
    }

    pub fn from_bits(bits: u32) -> Result<Self, TritError> {
        match bits {
            0b00 => Ok(TritLane::Low),
            0b01 => Ok(TritLane::Middle),
            0b10 => Ok(TritLane::High),
            0b11 => Ok(TritLane::BoundaryOrMisc),
            other => Err(TritError::InvalidLaneBits(other)),
        }
    }

    pub fn to_trit(self) -> Result<Trit, TritError> {
        match self {
            TritLane::Low => Ok(Trit::Negative),
            TritLane::Middle => Ok(Trit::Neutral),
            TritLane::High => Ok(Trit::Positive),
            TritLane::BoundaryOrMisc => Err(TritError::BoundaryOrMiscLane),
        }
    }

    pub fn is_boundary_or_misc(self) -> bool {
        matches!(self, TritLane::BoundaryOrMisc)
    }
}

impl fmt::Display for Trit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Trit::Negative => "-",
            Trit::Neutral => "0",
            Trit::Positive => "+",
        })
    }
}

impl fmt::Display for TritError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TritError::InvalidDigit(ch) => write!(f, "invalid trit digit '{ch}'"),
            TritError::InvalidBalancedValue(value) => {
                write!(f, "invalid balanced trit value {value}")
            }
            TritError::InvalidStorageDigit(digit) => {
                write!(f, "invalid trit storage digit {digit}")
            }
            TritError::InvalidLaneBits(bits) => write!(f, "invalid trit lane bits {bits:b}"),
            TritError::BoundaryOrMiscLane => {
                write!(f, "boundary/misc lane is not a value trit")
            }
        }
    }
}

impl Error for TritError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_digits_round_trip() {
        for trit in [Trit::Negative, Trit::Neutral, Trit::Positive] {
            let digit = trit.storage_digit();
            assert_eq!(Trit::from_storage_digit(digit), Ok(trit));
            assert_eq!(Trit::from_storage_char(trit.storage_char()), Ok(trit));
        }
    }

    #[test]
    fn lane_bits_reserve_all_ones_for_markers() {
        assert_eq!(Trit::Negative.lane_bits(), 0b00);
        assert_eq!(Trit::Neutral.lane_bits(), 0b01);
        assert_eq!(Trit::Positive.lane_bits(), 0b10);
        assert_eq!(
            TritLane::from_bits(TRIT_LANE_MARKER),
            Ok(TritLane::BoundaryOrMisc)
        );
        assert!(Trit::from_lane_bits(TRIT_LANE_MARKER).is_err());
    }
}
