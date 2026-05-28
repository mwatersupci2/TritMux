use crate::trit::core::{Trit, TritLane};
use crate::trit::payload::{PayloadError, TritPayload};
use std::error::Error;
use std::fmt;
use std::string::FromUtf8Error;

const MAGIC: &[u8] = b"TRIT_SCALAR_V1";

const TAG_NULL: u8 = 0x00;
const TAG_BOOL: u8 = 0x01;
const TAG_TRIT: u8 = 0x02;
const TAG_TRIT_LANE: u8 = 0x03;
const TAG_TEXT: u8 = 0x10;
const TAG_BYTES: u8 = 0x11;
const TAG_I64: u8 = 0x20;
const TAG_U64: u8 = 0x21;
const TAG_I128: u8 = 0x22;
const TAG_U128: u8 = 0x23;
const TAG_F32: u8 = 0x30;
const TAG_F64: u8 = 0x31;
const TAG_BIG_INT: u8 = 0x40;
const TAG_BIG_UINT: u8 = 0x41;
const TAG_DECIMAL: u8 = 0x50;
const TAG_FRACTION: u8 = 0x51;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigUIntScalar {
    magnitude_be: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigIntScalar {
    negative: bool,
    magnitude_be: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecimalScalar {
    negative: bool,
    coefficient: BigUIntScalar,
    scale: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FractionScalar {
    numerator: BigIntScalar,
    denominator: BigUIntScalar,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TritScalar {
    Null,
    Bool(bool),
    Trit(Trit),
    TritLane(TritLane),
    Text(String),
    Bytes(Vec<u8>),
    I64(i64),
    U64(u64),
    I128(i128),
    U128(u128),
    F32(f32),
    F64(f64),
    BigInt(BigIntScalar),
    BigUInt(BigUIntScalar),
    Decimal(DecimalScalar),
    Fraction(FractionScalar),
}

#[derive(Debug)]
pub enum ScalarError {
    Payload(PayloadError),
    InvalidMagic,
    MissingField(&'static str),
    InvalidTag(u8),
    InvalidBool(u8),
    InvalidLane(u8),
    InvalidSign(u8),
    ZeroDenominator,
    LengthOverflow,
    TrailingBytes,
    Utf8(FromUtf8Error),
}

impl BigUIntScalar {
    pub fn new(magnitude_be: Vec<u8>) -> Self {
        Self {
            magnitude_be: normalize_magnitude(magnitude_be),
        }
    }

    pub fn zero() -> Self {
        Self {
            magnitude_be: Vec::new(),
        }
    }

    pub fn from_u128(value: u128) -> Self {
        if value == 0 {
            return Self::zero();
        }

        Self::new(value.to_be_bytes().to_vec())
    }

    pub fn magnitude_be(&self) -> &[u8] {
        &self.magnitude_be
    }

    pub fn is_zero(&self) -> bool {
        self.magnitude_be.is_empty()
    }
}

impl BigIntScalar {
    pub fn new(negative: bool, magnitude_be: Vec<u8>) -> Self {
        let magnitude_be = normalize_magnitude(magnitude_be);
        Self {
            negative: negative && !magnitude_be.is_empty(),
            magnitude_be,
        }
    }

    pub fn zero() -> Self {
        Self {
            negative: false,
            magnitude_be: Vec::new(),
        }
    }

    pub fn from_i128(value: i128) -> Self {
        if value == 0 {
            return Self::zero();
        }

        let negative = value < 0;
        let magnitude = value.unsigned_abs();
        Self::new(negative, magnitude.to_be_bytes().to_vec())
    }

    pub fn negative(&self) -> bool {
        self.negative
    }

    pub fn magnitude_be(&self) -> &[u8] {
        &self.magnitude_be
    }

    pub fn is_zero(&self) -> bool {
        self.magnitude_be.is_empty()
    }
}

impl DecimalScalar {
    pub fn new(negative: bool, coefficient: BigUIntScalar, scale: i32) -> Self {
        Self {
            negative: negative && !coefficient.is_zero(),
            coefficient,
            scale,
        }
    }

    pub fn negative(&self) -> bool {
        self.negative
    }

    pub fn coefficient(&self) -> &BigUIntScalar {
        &self.coefficient
    }

    pub fn scale(&self) -> i32 {
        self.scale
    }
}

impl FractionScalar {
    pub fn new(
        numerator: BigIntScalar,
        denominator: BigUIntScalar,
    ) -> Result<Self, ScalarError> {
        if denominator.is_zero() {
            return Err(ScalarError::ZeroDenominator);
        }
        Ok(Self {
            numerator,
            denominator,
        })
    }

    pub fn numerator(&self) -> &BigIntScalar {
        &self.numerator
    }

    pub fn denominator(&self) -> &BigUIntScalar {
        &self.denominator
    }
}

impl TritScalar {
    pub fn to_payload(&self) -> Result<TritPayload, ScalarError> {
        Ok(TritPayload::from_bytes(&self.to_bytes()?))
    }

    pub fn from_payload(payload: &TritPayload) -> Result<Self, ScalarError> {
        let bytes = payload.to_bytes().map_err(ScalarError::Payload)?;
        Self::from_bytes(&bytes)
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, ScalarError> {
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);

        match self {
            TritScalar::Null => out.push(TAG_NULL),
            TritScalar::Bool(value) => {
                out.push(TAG_BOOL);
                out.push(u8::from(*value));
            }
            TritScalar::Trit(value) => {
                out.push(TAG_TRIT);
                out.push(value.storage_digit());
            }
            TritScalar::TritLane(value) => {
                out.push(TAG_TRIT_LANE);
                out.push(value.bits() as u8);
            }
            TritScalar::Text(value) => {
                out.push(TAG_TEXT);
                write_bytes(&mut out, value.as_bytes())?;
            }
            TritScalar::Bytes(value) => {
                out.push(TAG_BYTES);
                write_bytes(&mut out, value)?;
            }
            TritScalar::I64(value) => {
                out.push(TAG_I64);
                out.extend_from_slice(&value.to_le_bytes());
            }
            TritScalar::U64(value) => {
                out.push(TAG_U64);
                out.extend_from_slice(&value.to_le_bytes());
            }
            TritScalar::I128(value) => {
                out.push(TAG_I128);
                out.extend_from_slice(&value.to_le_bytes());
            }
            TritScalar::U128(value) => {
                out.push(TAG_U128);
                out.extend_from_slice(&value.to_le_bytes());
            }
            TritScalar::F32(value) => {
                out.push(TAG_F32);
                out.extend_from_slice(&value.to_bits().to_le_bytes());
            }
            TritScalar::F64(value) => {
                out.push(TAG_F64);
                out.extend_from_slice(&value.to_bits().to_le_bytes());
            }
            TritScalar::BigInt(value) => {
                out.push(TAG_BIG_INT);
                write_big_int(&mut out, value)?;
            }
            TritScalar::BigUInt(value) => {
                out.push(TAG_BIG_UINT);
                write_big_uint(&mut out, value)?;
            }
            TritScalar::Decimal(value) => {
                out.push(TAG_DECIMAL);
                out.push(sign_byte(value.negative));
                out.extend_from_slice(&value.scale.to_le_bytes());
                write_big_uint(&mut out, &value.coefficient)?;
            }
            TritScalar::Fraction(value) => {
                out.push(TAG_FRACTION);
                write_big_int(&mut out, &value.numerator)?;
                write_big_uint(&mut out, &value.denominator)?;
            }
        }

        Ok(out)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ScalarError> {
        let mut reader = ByteReader::new(bytes);
        let magic = reader.take(MAGIC.len(), "magic")?;
        if magic != MAGIC {
            return Err(ScalarError::InvalidMagic);
        }

        let tag = reader.u8("tag")?;
        let value = match tag {
            TAG_NULL => TritScalar::Null,
            TAG_BOOL => TritScalar::Bool(read_bool(reader.u8("bool")?)?),
            TAG_TRIT => {
                let digit = reader.u8("trit")?;
                TritScalar::Trit(Trit::from_storage_digit(digit).map_err(|_| {
                    ScalarError::InvalidLane(digit)
                })?)
            }
            TAG_TRIT_LANE => {
                let bits = reader.u8("trit lane")?;
                let lane = TritLane::from_bits(bits as u32)
                    .map_err(|_| ScalarError::InvalidLane(bits))?;
                TritScalar::TritLane(lane)
            }
            TAG_TEXT => {
                let bytes = reader.bytes("text")?.to_vec();
                TritScalar::Text(String::from_utf8(bytes).map_err(ScalarError::Utf8)?)
            }
            TAG_BYTES => TritScalar::Bytes(reader.bytes("bytes")?.to_vec()),
            TAG_I64 => TritScalar::I64(i64::from_le_bytes(reader.array("i64")?)),
            TAG_U64 => TritScalar::U64(u64::from_le_bytes(reader.array("u64")?)),
            TAG_I128 => TritScalar::I128(i128::from_le_bytes(reader.array("i128")?)),
            TAG_U128 => TritScalar::U128(u128::from_le_bytes(reader.array("u128")?)),
            TAG_F32 => TritScalar::F32(f32::from_bits(u32::from_le_bytes(reader.array("f32")?))),
            TAG_F64 => TritScalar::F64(f64::from_bits(u64::from_le_bytes(reader.array("f64")?))),
            TAG_BIG_INT => TritScalar::BigInt(read_big_int(&mut reader)?),
            TAG_BIG_UINT => TritScalar::BigUInt(read_big_uint(&mut reader)?),
            TAG_DECIMAL => {
                let negative = read_sign(reader.u8("decimal sign")?)?;
                let scale = i32::from_le_bytes(reader.array("decimal scale")?);
                let coefficient = read_big_uint(&mut reader)?;
                TritScalar::Decimal(DecimalScalar::new(negative, coefficient, scale))
            }
            TAG_FRACTION => {
                let numerator = read_big_int(&mut reader)?;
                let denominator = read_big_uint(&mut reader)?;
                TritScalar::Fraction(FractionScalar::new(numerator, denominator)?)
            }
            other => return Err(ScalarError::InvalidTag(other)),
        };

        reader.finish()?;
        Ok(value)
    }
}

fn normalize_magnitude(mut magnitude: Vec<u8>) -> Vec<u8> {
    let first_non_zero = magnitude.iter().position(|byte| *byte != 0);
    match first_non_zero {
        Some(index) => {
            if index > 0 {
                magnitude.drain(0..index);
            }
            magnitude
        }
        None => Vec::new(),
    }
}

fn write_big_int(out: &mut Vec<u8>, value: &BigIntScalar) -> Result<(), ScalarError> {
    out.push(sign_byte(value.negative));
    write_bytes(out, value.magnitude_be())
}

fn read_big_int(reader: &mut ByteReader<'_>) -> Result<BigIntScalar, ScalarError> {
    let negative = read_sign(reader.u8("big int sign")?)?;
    let magnitude = reader.bytes("big int magnitude")?.to_vec();
    Ok(BigIntScalar::new(negative, magnitude))
}

fn write_big_uint(out: &mut Vec<u8>, value: &BigUIntScalar) -> Result<(), ScalarError> {
    write_bytes(out, value.magnitude_be())
}

fn read_big_uint(reader: &mut ByteReader<'_>) -> Result<BigUIntScalar, ScalarError> {
    Ok(BigUIntScalar::new(
        reader.bytes("big uint magnitude")?.to_vec(),
    ))
}

fn write_bytes(out: &mut Vec<u8>, bytes: &[u8]) -> Result<(), ScalarError> {
    let len = u32::try_from(bytes.len()).map_err(|_| ScalarError::LengthOverflow)?;
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(bytes);
    Ok(())
}

fn sign_byte(negative: bool) -> u8 {
    u8::from(negative)
}

fn read_bool(value: u8) -> Result<bool, ScalarError> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        other => Err(ScalarError::InvalidBool(other)),
    }
}

fn read_sign(value: u8) -> Result<bool, ScalarError> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        other => Err(ScalarError::InvalidSign(other)),
    }
}

struct ByteReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> ByteReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn u8(&mut self, name: &'static str) -> Result<u8, ScalarError> {
        Ok(self.take(1, name)?[0])
    }

    fn bytes(&mut self, name: &'static str) -> Result<&'a [u8], ScalarError> {
        let len = u32::from_le_bytes(self.array(name)?) as usize;
        self.take(len, name)
    }

    fn array<const N: usize>(&mut self, name: &'static str) -> Result<[u8; N], ScalarError> {
        let bytes = self.take(N, name)?;
        let mut out = [0u8; N];
        out.copy_from_slice(bytes);
        Ok(out)
    }

    fn take(&mut self, len: usize, name: &'static str) -> Result<&'a [u8], ScalarError> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or(ScalarError::LengthOverflow)?;
        if end > self.bytes.len() {
            return Err(ScalarError::MissingField(name));
        }
        let bytes = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(bytes)
    }

    fn finish(&self) -> Result<(), ScalarError> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(ScalarError::TrailingBytes)
        }
    }
}

impl fmt::Display for ScalarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScalarError::Payload(err) => write!(f, "{err}"),
            ScalarError::InvalidMagic => write!(f, "invalid trit scalar magic"),
            ScalarError::MissingField(name) => write!(f, "missing scalar field: {name}"),
            ScalarError::InvalidTag(tag) => write!(f, "invalid scalar tag 0x{tag:02x}"),
            ScalarError::InvalidBool(value) => write!(f, "invalid bool value {value}"),
            ScalarError::InvalidLane(value) => write!(f, "invalid trit lane value {value}"),
            ScalarError::InvalidSign(value) => write!(f, "invalid sign value {value}"),
            ScalarError::ZeroDenominator => write!(f, "fraction denominator cannot be zero"),
            ScalarError::LengthOverflow => write!(f, "scalar length overflow"),
            ScalarError::TrailingBytes => write!(f, "scalar payload has trailing bytes"),
            ScalarError::Utf8(err) => write!(f, "invalid UTF-8 scalar text: {err}"),
        }
    }
}

impl Error for ScalarError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(value: TritScalar) {
        let payload = value.to_payload().unwrap();
        assert!(payload.len_strytes() > 0);
        assert_eq!(TritScalar::from_payload(&payload).unwrap(), value);
    }

    #[test]
    fn scalar_round_trips_basic_programmer_types() {
        round_trip(TritScalar::Null);
        round_trip(TritScalar::Bool(true));
        round_trip(TritScalar::Trit(Trit::Positive));
        round_trip(TritScalar::TritLane(TritLane::BoundaryOrMisc));
        round_trip(TritScalar::Text("hello 🌎🙂".to_string()));
        round_trip(TritScalar::Bytes(vec![0, 1, 2, 255]));
    }

    #[test]
    fn scalar_round_trips_numbers() {
        round_trip(TritScalar::I64(-42));
        round_trip(TritScalar::U64(42));
        round_trip(TritScalar::I128(i128::MIN + 123));
        round_trip(TritScalar::U128(u128::MAX - 123));
        round_trip(TritScalar::F32(-3.5));
        round_trip(TritScalar::F64(std::f64::consts::PI));
    }

    #[test]
    fn scalar_round_trips_big_decimal_and_fraction_types() {
        let big_uint = BigUIntScalar::new(vec![0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
        let big_int = BigIntScalar::new(true, vec![0, 255, 254, 253, 252]);
        let decimal = DecimalScalar::new(true, BigUIntScalar::new(vec![1, 35, 69, 103]), 6);
        let fraction = FractionScalar::new(
            BigIntScalar::new(true, vec![12, 34, 56]),
            BigUIntScalar::new(vec![7, 89]),
        )
        .unwrap();

        round_trip(TritScalar::BigUInt(big_uint));
        round_trip(TritScalar::BigInt(big_int));
        round_trip(TritScalar::Decimal(decimal));
        round_trip(TritScalar::Fraction(fraction));
    }

    #[test]
    fn zero_denominator_is_rejected() {
        assert!(matches!(
            FractionScalar::new(BigIntScalar::zero(), BigUIntScalar::zero()),
            Err(ScalarError::ZeroDenominator)
        ));
    }

    #[test]
    fn scalar_payload_uses_existing_zip_pipeline() {
        let scalar = TritScalar::Text("cold typed value 🧊".to_string());
        let payload = scalar.to_payload().unwrap();
        assert!(!payload.zipped_u640_grids().unwrap().is_empty());
        assert_eq!(TritScalar::from_payload(&payload).unwrap(), scalar);
    }
}
