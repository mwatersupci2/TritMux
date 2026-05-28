use crate::memory::{MemoryError, MemoryRegionId, Residency, TrinaryMemoryManager};
use crate::trit::payload::{PayloadError, TritPayload};
use crate::trit::scalar::{ScalarError, TritScalar};
use crate::trit::zip::{
    decode_grid9x9_base3_lines, decode_usize_base3, encode_grid9x9_base3_lines,
    encode_usize_base3, grid9x9_count_for_u640, stryte_count_for_trits,
    u640_count_for_strytes, unzip_u640_grids_to_strytes, ZipError, U640S_PER_GRID9X9,
};
use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io;
use std::path::Path;

const MAGIC: &[u8] = b"TRITMUX_KV_ZIP_V1";

#[derive(Debug, Clone, Eq)]
pub struct KvKey {
    scalar_bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KvValueState {
    Hot,
    Cold,
}

#[derive(Debug, Clone)]
enum ValueSlot {
    Hot(TritScalar),
    HotRegion { region_id: MemoryRegionId },
    Cold(ColdValue),
}

#[derive(Debug, Clone)]
struct ColdValue {
    trit_len: usize,
    grid_lines: Vec<String>,
}

#[derive(Debug, Default, Clone)]
pub struct KvStore {
    entries: HashMap<KvKey, ValueSlot>,
    dirty: bool,
}

#[derive(Debug)]
pub enum KvError {
    Io(io::Error),
    InvalidMagic,
    MissingLine(&'static str),
    Scalar(ScalarError),
    Payload(PayloadError),
    Zip(ZipError),
    Memory(MemoryError),
    MemoryManagerRequired,
}

impl KvKey {
    pub fn from_scalar(scalar: &TritScalar) -> Result<Self, KvError> {
        Ok(Self {
            scalar_bytes: scalar.to_bytes().map_err(KvError::Scalar)?,
        })
    }

    pub fn to_scalar(&self) -> Result<TritScalar, KvError> {
        TritScalar::from_bytes(&self.scalar_bytes).map_err(KvError::Scalar)
    }

    pub fn encoded_bytes(&self) -> &[u8] {
        &self.scalar_bytes
    }
}

impl PartialEq for KvKey {
    fn eq(&self, other: &Self) -> bool {
        self.scalar_bytes == other.scalar_bytes
    }
}

impl Hash for KvKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.scalar_bytes.hash(state);
    }
}

impl KvStore {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            dirty: false,
        }
    }

    pub fn load(path: &Path) -> Result<Self, KvError> {
        if !path.exists() {
            return Ok(Self::new());
        }

        let artifact = fs::read_to_string(path).map_err(KvError::Io)?;
        Self::from_artifact(&artifact)
    }

    pub fn from_artifact(artifact: &str) -> Result<Self, KvError> {
        let mut lines = artifact.lines().filter(|line| !line.trim().is_empty());
        let Some(magic_line) = lines.next() else {
            return Ok(Self::new());
        };

        let magic_payload =
            TritPayload::from_storage_digits(magic_line.trim()).map_err(KvError::Payload)?;
        if magic_payload.to_bytes().map_err(KvError::Payload)? != MAGIC {
            return Err(KvError::InvalidMagic);
        }

        let count =
            decode_usize_base3(next_line(&mut lines, "entry count")?).map_err(KvError::Zip)?;
        let mut entries = HashMap::with_capacity(count);

        for _ in 0..count {
            let key = read_key(&mut lines)?;
            let value = read_cold_value(&mut lines)?;
            entries.insert(key, ValueSlot::Cold(value));
        }

        Ok(Self {
            entries,
            dirty: false,
        })
    }

    pub fn flush(&mut self, path: &Path) -> Result<(), KvError> {
        if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            fs::create_dir_all(parent).map_err(KvError::Io)?;
        }

        let artifact = self.to_artifact()?;
        fs::write(path, artifact).map_err(KvError::Io)?;
        self.dirty = false;
        Ok(())
    }

    pub fn flush_managed(
        &mut self,
        path: &Path,
        memory: &TrinaryMemoryManager,
    ) -> Result<(), KvError> {
        if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            fs::create_dir_all(parent).map_err(KvError::Io)?;
        }

        let artifact = self.to_artifact_managed(memory)?;
        fs::write(path, artifact).map_err(KvError::Io)?;
        self.dirty = false;
        Ok(())
    }

    pub fn to_artifact(&self) -> Result<String, KvError> {
        let mut keys: Vec<&KvKey> = self.entries.keys().collect();
        keys.sort_by(|left, right| left.encoded_bytes().cmp(right.encoded_bytes()));

        let mut artifact = String::new();
        artifact.push_str(&TritPayload::from_bytes(MAGIC).as_storage_digits());
        artifact.push('\n');
        artifact.push_str(&encode_usize_base3(keys.len()));
        artifact.push('\n');

        for key in keys {
            write_payload(&mut artifact, &TritPayload::from_bytes(key.encoded_bytes()))?;
            match self
                .entries
                .get(key)
                .expect("key came from this map's key set")
            {
                ValueSlot::Hot(value) => {
                    write_payload(&mut artifact, &value.to_payload().map_err(KvError::Scalar)?)?
                }
                ValueSlot::HotRegion { .. } => return Err(KvError::MemoryManagerRequired),
                ValueSlot::Cold(value) => write_cold_value(&mut artifact, value),
            }
        }

        Ok(artifact)
    }

    pub fn to_artifact_managed(&self, memory: &TrinaryMemoryManager) -> Result<String, KvError> {
        let mut keys: Vec<&KvKey> = self.entries.keys().collect();
        keys.sort_by(|left, right| left.encoded_bytes().cmp(right.encoded_bytes()));

        let mut artifact = String::new();
        artifact.push_str(&TritPayload::from_bytes(MAGIC).as_storage_digits());
        artifact.push('\n');
        artifact.push_str(&encode_usize_base3(keys.len()));
        artifact.push('\n');

        for key in keys {
            write_payload(&mut artifact, &TritPayload::from_bytes(key.encoded_bytes()))?;
            match self
                .entries
                .get(key)
                .expect("key came from this map's key set")
            {
                ValueSlot::Hot(value) => {
                    write_payload(&mut artifact, &value.to_payload().map_err(KvError::Scalar)?)?
                }
                ValueSlot::HotRegion { region_id } => {
                    let payload = memory
                        .region(*region_id)
                        .map_err(KvError::Memory)?
                        .to_payload()
                        .map_err(KvError::Memory)?;
                    write_payload(&mut artifact, &payload)?;
                }
                ValueSlot::Cold(value) => write_cold_value(&mut artifact, value),
            }
        }

        Ok(artifact)
    }

    pub fn put(&mut self, key: TritScalar, value: TritScalar) -> Result<(), KvError> {
        let key = KvKey::from_scalar(&key)?;
        self.entries.insert(key, ValueSlot::Hot(value));
        self.dirty = true;
        Ok(())
    }

    pub fn put_managed(
        &mut self,
        key: TritScalar,
        value: TritScalar,
        memory: &mut TrinaryMemoryManager,
        residency: Residency,
    ) -> Result<MemoryRegionId, KvError> {
        let key = KvKey::from_scalar(&key)?;
        let payload = value.to_payload().map_err(KvError::Scalar)?;
        let region_id = memory.allocate_heap_payload("kv hot value", &payload, residency);
        self.entries.insert(key, ValueSlot::HotRegion { region_id });
        self.dirty = true;
        Ok(region_id)
    }

    pub fn get(&mut self, key: &TritScalar) -> Result<Option<TritScalar>, KvError> {
        let key = KvKey::from_scalar(key)?;
        let Some(slot) = self.entries.get_mut(&key) else {
            return Ok(None);
        };

        if let ValueSlot::Cold(value) = slot {
            let scalar = value.decode_scalar()?;
            *slot = ValueSlot::Hot(scalar);
        }

        match slot {
            ValueSlot::Hot(value) => Ok(Some(value.clone())),
            ValueSlot::HotRegion { .. } => Err(KvError::MemoryManagerRequired),
            ValueSlot::Cold(_) => unreachable!("cold slot is promoted before returning"),
        }
    }

    pub fn get_managed(
        &mut self,
        key: &TritScalar,
        memory: &mut TrinaryMemoryManager,
    ) -> Result<Option<TritScalar>, KvError> {
        let key = KvKey::from_scalar(key)?;
        let Some(slot) = self.entries.get_mut(&key) else {
            return Ok(None);
        };

        match slot {
            ValueSlot::Hot(value) => Ok(Some(value.clone())),
            ValueSlot::HotRegion { region_id } => decode_scalar_from_memory(memory, *region_id).map(Some),
            ValueSlot::Cold(value) => {
                let payload = value.decode_payload()?;
                let scalar = TritScalar::from_payload(&payload).map_err(KvError::Scalar)?;
                let region_id = memory.allocate_heap_payload("kv promoted value", &payload, Residency::Hot);
                *slot = ValueSlot::HotRegion { region_id };
                Ok(Some(scalar))
            }
        }
    }

    pub fn delete(&mut self, key: &TritScalar) -> Result<bool, KvError> {
        let key = KvKey::from_scalar(key)?;
        let removed = self.entries.remove(&key).is_some();
        if removed {
            self.dirty = true;
        }
        Ok(removed)
    }

    pub fn contains_key(&self, key: &TritScalar) -> Result<bool, KvError> {
        let key = KvKey::from_scalar(key)?;
        Ok(self.entries.contains_key(&key))
    }

    pub fn key_state(&self, key: &TritScalar) -> Result<Option<KvValueState>, KvError> {
        let key = KvKey::from_scalar(key)?;
        Ok(self.entries.get(&key).map(|slot| match slot {
            ValueSlot::Hot(_) | ValueSlot::HotRegion { .. } => KvValueState::Hot,
            ValueSlot::Cold(_) => KvValueState::Cold,
        }))
    }

    pub fn keys(&self) -> Result<Vec<TritScalar>, KvError> {
        let mut keys: Vec<&KvKey> = self.entries.keys().collect();
        keys.sort_by(|left, right| left.encoded_bytes().cmp(right.encoded_bytes()));
        keys.into_iter().map(KvKey::to_scalar).collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn hot_len(&self) -> usize {
        self.entries
            .values()
            .filter(|slot| matches!(slot, ValueSlot::Hot(_) | ValueSlot::HotRegion { .. }))
            .count()
    }

    pub fn cold_len(&self) -> usize {
        self.entries
            .values()
            .filter(|slot| matches!(slot, ValueSlot::Cold(_)))
            .count()
    }
}

fn decode_scalar_from_memory(
    memory: &TrinaryMemoryManager,
    region_id: MemoryRegionId,
) -> Result<TritScalar, KvError> {
    let payload = memory
        .region(region_id)
        .map_err(KvError::Memory)?
        .to_payload()
        .map_err(KvError::Memory)?;
    TritScalar::from_payload(&payload).map_err(KvError::Scalar)
}

impl ColdValue {
    fn decode_payload(&self) -> Result<TritPayload, KvError> {
        let stryte_count = stryte_count_for_trits(self.trit_len);
        let grid_count = expected_grid_count(self.trit_len);
        let mut grids = Vec::with_capacity(grid_count);

        for chunk in self.grid_lines.chunks(U640S_PER_GRID9X9) {
            let refs: Vec<&str> = chunk.iter().map(String::as_str).collect();
            grids.push(decode_grid9x9_base3_lines(&refs).map_err(KvError::Zip)?);
        }

        let strytes =
            unzip_u640_grids_to_strytes(&grids, stryte_count).map_err(KvError::Zip)?;
        TritPayload::from_strytes(strytes, self.trit_len).map_err(KvError::Payload)
    }

    fn decode_scalar(&self) -> Result<TritScalar, KvError> {
        TritScalar::from_payload(&self.decode_payload()?).map_err(KvError::Scalar)
    }
}

fn read_key<'a, I>(lines: &mut I) -> Result<KvKey, KvError>
where
    I: Iterator<Item = &'a str>,
{
    let payload = read_payload(lines)?;
    let scalar = TritScalar::from_payload(&payload).map_err(KvError::Scalar)?;
    KvKey::from_scalar(&scalar)
}

fn read_cold_value<'a, I>(lines: &mut I) -> Result<ColdValue, KvError>
where
    I: Iterator<Item = &'a str>,
{
    let trit_len =
        decode_usize_base3(next_line(lines, "value trit length")?).map_err(KvError::Zip)?;
    let grid_count =
        decode_usize_base3(next_line(lines, "value 9x9 U640 grid count")?).map_err(KvError::Zip)?;
    let expected = expected_grid_count(trit_len);
    if grid_count != expected {
        return Err(KvError::Zip(ZipError::InsufficientBlocks {
            needed: expected,
            available: grid_count,
        }));
    }

    let mut grid_lines = Vec::with_capacity(grid_count * U640S_PER_GRID9X9);
    for _ in 0..grid_count * U640S_PER_GRID9X9 {
        grid_lines.push(next_line(lines, "value 9x9 U640 base3 block")?.to_string());
    }

    Ok(ColdValue {
        trit_len,
        grid_lines,
    })
}

fn read_payload<'a, I>(lines: &mut I) -> Result<TritPayload, KvError>
where
    I: Iterator<Item = &'a str>,
{
    let cold = read_cold_value(lines)?;
    cold.decode_payload()
}

fn write_payload(out: &mut String, payload: &TritPayload) -> Result<(), KvError> {
    let grids = payload.zipped_u640_grids().map_err(KvError::Zip)?;
    out.push_str(&encode_usize_base3(payload.len_trits()));
    out.push('\n');
    out.push_str(&encode_usize_base3(grids.len()));
    out.push('\n');

    for grid in &grids {
        for line in encode_grid9x9_base3_lines(grid) {
            out.push_str(&line);
            out.push('\n');
        }
    }

    Ok(())
}

fn write_cold_value(out: &mut String, value: &ColdValue) {
    out.push_str(&encode_usize_base3(value.trit_len));
    out.push('\n');
    out.push_str(&encode_usize_base3(expected_grid_count(value.trit_len)));
    out.push('\n');
    for line in &value.grid_lines {
        out.push_str(line);
        out.push('\n');
    }
}

fn expected_grid_count(trit_len: usize) -> usize {
    let strytes = stryte_count_for_trits(trit_len);
    grid9x9_count_for_u640(u640_count_for_strytes(strytes))
}

fn next_line<'a, I>(lines: &mut I, name: &'static str) -> Result<&'a str, KvError>
where
    I: Iterator<Item = &'a str>,
{
    lines.next().map(str::trim).ok_or(KvError::MissingLine(name))
}

impl fmt::Display for KvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KvError::Io(err) => write!(f, "{err}"),
            KvError::InvalidMagic => write!(f, "invalid trinary KV artifact magic"),
            KvError::MissingLine(name) => write!(f, "missing trinary KV line: {name}"),
            KvError::Scalar(err) => write!(f, "{err}"),
            KvError::Payload(err) => write!(f, "{err}"),
            KvError::Zip(err) => write!(f, "{err}"),
            KvError::Memory(err) => write!(f, "{err}"),
            KvError::MemoryManagerRequired => {
                write!(f, "memory-backed KV values require a memory manager")
            }
        }
    }
}

impl Error for KvError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trit::scalar::{BigIntScalar, BigUIntScalar, DecimalScalar, FractionScalar};
    use std::env;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn typed_keys_distinguish_text_from_integer() {
        let mut store = KvStore::new();
        store
            .put(
                TritScalar::Text("1".to_string()),
                TritScalar::Text("text-key".to_string()),
            )
            .unwrap();
        store
            .put(TritScalar::I64(1), TritScalar::Text("int-key".to_string()))
            .unwrap();

        assert_eq!(
            store.get(&TritScalar::Text("1".to_string())).unwrap(),
            Some(TritScalar::Text("text-key".to_string()))
        );
        assert_eq!(
            store.get(&TritScalar::I64(1)).unwrap(),
            Some(TritScalar::Text("int-key".to_string()))
        );
    }

    #[test]
    fn load_indexes_keys_but_keeps_values_cold_until_get() {
        let mut store = KvStore::new();
        let key = TritScalar::Text("rare".to_string());
        let value = TritScalar::Text("cold value".to_string());
        store.put(key.clone(), value.clone()).unwrap();

        let artifact = store.to_artifact().unwrap();
        assert!(artifact
            .chars()
            .all(|ch| matches!(ch, '0' | '1' | '2' | '\n')));

        let mut loaded = KvStore::from_artifact(&artifact).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded.hot_len(), 0);
        assert_eq!(loaded.cold_len(), 1);
        assert_eq!(loaded.key_state(&key).unwrap(), Some(KvValueState::Cold));

        assert_eq!(loaded.get(&key).unwrap(), Some(value));
        assert_eq!(loaded.hot_len(), 1);
        assert_eq!(loaded.cold_len(), 0);
        assert_eq!(loaded.key_state(&key).unwrap(), Some(KvValueState::Hot));
    }

    #[test]
    fn flush_and_load_file_round_trip_typed_values() {
        let mut store = KvStore::new();
        let fraction = TritScalar::Fraction(
            FractionScalar::new(
                BigIntScalar::new(true, vec![12, 34, 56]),
                BigUIntScalar::new(vec![7, 89]),
            )
            .unwrap(),
        );
        let decimal = TritScalar::Decimal(DecimalScalar::new(
            true,
            BigUIntScalar::new(vec![1, 35, 69, 103]),
            6,
        ));

        store
            .put(TritScalar::Text("fraction".to_string()), fraction.clone())
            .unwrap();
        store.put(TritScalar::U64(42), decimal.clone()).unwrap();

        let path = env::temp_dir().join(format!(
            "tritmux-kv-test-{}.trit",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        store.flush(&path).unwrap();
        let raw = fs::read_to_string(&path).unwrap();
        assert!(raw.chars().all(|ch| matches!(ch, '0' | '1' | '2' | '\n')));

        let mut loaded = KvStore::load(&path).unwrap();
        assert_eq!(
            loaded
                .get(&TritScalar::Text("fraction".to_string()))
                .unwrap(),
            Some(fraction)
        );
        assert_eq!(loaded.get(&TritScalar::U64(42)).unwrap(), Some(decimal));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn delete_cold_value_without_unzipping_it() {
        let mut store = KvStore::new();
        let key = TritScalar::Text("drop".to_string());
        store
            .put(key.clone(), TritScalar::Text("delete me".to_string()))
            .unwrap();

        let artifact = store.to_artifact().unwrap();
        let mut loaded = KvStore::from_artifact(&artifact).unwrap();
        assert_eq!(loaded.key_state(&key).unwrap(), Some(KvValueState::Cold));
        assert!(loaded.delete(&key).unwrap());
        assert_eq!(loaded.hot_len(), 0);
        assert_eq!(loaded.cold_len(), 0);

        let reloaded = KvStore::from_artifact(&loaded.to_artifact().unwrap()).unwrap();
        assert!(!reloaded.contains_key(&key).unwrap());
    }

    #[test]
    fn managed_get_promotes_cold_value_into_memory_region() {
        let mut store = KvStore::new();
        let key = TritScalar::Text("managed".to_string());
        let value = TritScalar::Text("memory backed".to_string());
        store.put(key.clone(), value.clone()).unwrap();

        let artifact = store.to_artifact().unwrap();
        let mut loaded = KvStore::from_artifact(&artifact).unwrap();
        let mut memory = TrinaryMemoryManager::new();

        assert_eq!(loaded.key_state(&key).unwrap(), Some(KvValueState::Cold));
        assert_eq!(loaded.get_managed(&key, &mut memory).unwrap(), Some(value));
        assert_eq!(loaded.key_state(&key).unwrap(), Some(KvValueState::Hot));
        assert_eq!(memory.stats().heap_regions, 1);
    }

    #[test]
    fn managed_hot_values_flush_through_memory_manager() {
        let mut store = KvStore::new();
        let mut memory = TrinaryMemoryManager::new();
        let key = TritScalar::Text("hot".to_string());
        let value = TritScalar::U128(123456789);

        let region = store
            .put_managed(key.clone(), value.clone(), &mut memory, Residency::Hot)
            .unwrap();
        assert_eq!(memory.region(region).unwrap().residency(), Residency::Hot);
        assert!(matches!(store.to_artifact(), Err(KvError::MemoryManagerRequired)));

        let artifact = store.to_artifact_managed(&memory).unwrap();
        assert!(artifact
            .chars()
            .all(|ch| matches!(ch, '0' | '1' | '2' | '\n')));

        let mut loaded = KvStore::from_artifact(&artifact).unwrap();
        let mut loaded_memory = TrinaryMemoryManager::new();
        assert_eq!(loaded.get_managed(&key, &mut loaded_memory).unwrap(), Some(value));
    }
}
