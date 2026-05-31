use crate::notes::{NoteId, NoteStore};
use crate::trit::block::{U320_WORDS, U640_WORDS};
use crate::trit::compact::{LockWindow, WindowLockError};
use crate::trit::core::Trit;
use crate::trit::payload::TritPayload;
use crate::trit::tryte::STryte;
use crate::trit::zip::{
    encode_u640_base3, grid9x9_count_for_u640, u640_count_for_strytes,
    zip_strytes_to_u640, ZipError, U640S_PER_GRID9X9,
};
use std::error::Error;
use std::fmt;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const PREVIEW_STRYTES: usize = 8;
const PREVIEW_BLOCKS: usize = 3;
const PREVIEW_WORDS: usize = 4;
const PREVIEW_BASE3_CHARS: usize = 64;
const WINDOW_MAP_LIMIT: usize = 72;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TritCounts {
    pub negative: usize,
    pub neutral: usize,
    pub positive: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct STryteObservation {
    pub index: usize,
    pub word: u32,
    pub marker_bits_set: bool,
    pub trits: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct U320Observation {
    pub index: usize,
    pub ptryte_count: usize,
    pub preview_words: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct U640Observation {
    pub index: usize,
    pub preview_u64_pairs: Vec<u64>,
    pub base3_digit_len: usize,
    pub base3_preview: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionWindowObservation {
    pub active_index: usize,
    pub locked_start: usize,
    pub locked_end_exclusive: usize,
    pub settle_start: usize,
    pub settle_end_exclusive: usize,
    pub active_guard_start: usize,
    pub active_guard_end_exclusive: usize,
    pub map: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayloadObservation {
    pub label: String,
    pub trits: usize,
    pub strytes: usize,
    pub ptrytes: usize,
    pub u320_blocks: usize,
    pub u320_words: usize,
    pub u640_blocks: usize,
    pub u640_words: usize,
    pub u64_pairs: usize,
    pub grids_9x9: usize,
    pub zipped_base3_lines: usize,
    pub trit_counts: TritCounts,
    pub stryte_preview: Vec<STryteObservation>,
    pub u320_preview: Vec<U320Observation>,
    pub u640_preview: Vec<U640Observation>,
    pub selected_window: Option<CompactionWindowObservation>,
    pub compaction_steps: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreObservation {
    pub notes: usize,
    pub dirty: bool,
    pub total_trits: usize,
    pub total_strytes: usize,
    pub total_u640_blocks: usize,
    pub total_grids_9x9: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskLineObservation {
    pub index: usize,
    pub len: usize,
    pub zeros: usize,
    pub ones: usize,
    pub twos: usize,
    pub preview: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskObservation {
    pub path: PathBuf,
    pub exists: bool,
    pub bytes: u64,
    pub lines: usize,
    pub non_empty_lines: usize,
    pub trinary_only: bool,
    pub zeros: usize,
    pub ones: usize,
    pub twos: usize,
    pub invalid_chars: usize,
    pub line_preview: Vec<DiskLineObservation>,
}

#[derive(Debug)]
pub enum ObserveError {
    Io(io::Error),
    NoteNotFound(NoteId),
    Window(WindowLockError),
    Zip(ZipError),
}

impl PayloadObservation {
    pub fn from_payload(
        label: impl Into<String>,
        payload: &TritPayload,
        active_step: Option<usize>,
    ) -> Result<Self, ObserveError> {
        let label = label.into();
        let compaction = payload.guarded_compaction().map_err(ObserveError::Window)?;
        let u640_blocks = zip_strytes_to_u640(payload.strytes()).map_err(ObserveError::Zip)?;
        let u320_blocks = compaction.blocks();
        let grids_9x9 = grid9x9_count_for_u640(u640_blocks.len());
        let selected_window = select_compaction_window(
            compaction.windows(),
            payload.len_strytes(),
            active_step,
        );

        Ok(Self {
            label,
            trits: payload.len_trits(),
            strytes: payload.len_strytes(),
            ptrytes: payload.len_strytes(),
            u320_blocks: u320_blocks.len(),
            u320_words: u320_blocks.len() * U320_WORDS,
            u640_blocks: u640_blocks.len(),
            u640_words: u640_blocks.len() * U640_WORDS,
            u64_pairs: u640_blocks.len() * (U640_WORDS / 2),
            grids_9x9,
            zipped_base3_lines: grids_9x9 * U640S_PER_GRID9X9,
            trit_counts: count_trits(payload),
            stryte_preview: payload
                .strytes()
                .iter()
                .take(PREVIEW_STRYTES)
                .enumerate()
                .map(|(index, stryte)| observe_stryte(index, *stryte))
                .collect(),
            u320_preview: u320_blocks
                .iter()
                .take(PREVIEW_BLOCKS)
                .enumerate()
                .map(|(index, block)| U320Observation {
                    index,
                    ptryte_count: block.ptryte_count(),
                    preview_words: block
                        .words()
                        .iter()
                        .take(PREVIEW_WORDS)
                        .copied()
                        .collect(),
                })
                .collect(),
            u640_preview: u640_blocks
                .iter()
                .take(PREVIEW_BLOCKS)
                .enumerate()
                .map(|(index, block)| {
                    let base3 = encode_u640_base3(block);
                    U640Observation {
                        index,
                        preview_u64_pairs: u64_pairs_from_u32_words(block.words())
                            .into_iter()
                            .take(PREVIEW_WORDS)
                            .collect(),
                        base3_digit_len: base3.len(),
                        base3_preview: preview_text(&base3, PREVIEW_BASE3_CHARS),
                    }
                })
                .collect(),
            selected_window,
            compaction_steps: compaction.windows().len(),
        })
    }
}

impl StoreObservation {
    pub fn from_store(store: &NoteStore) -> Self {
        let mut observation = Self {
            notes: store.notes().len(),
            dirty: store.is_dirty(),
            total_trits: 0,
            total_strytes: 0,
            total_u640_blocks: 0,
            total_grids_9x9: 0,
        };

        for note in store.notes() {
            for payload in [note.title_payload(), note.body_payload()] {
                observation.total_trits += payload.len_trits();
                observation.total_strytes += payload.len_strytes();
                let u640 = u640_count_for_strytes(payload.len_strytes());
                observation.total_u640_blocks += u640;
                observation.total_grids_9x9 += grid9x9_count_for_u640(u640);
            }
        }

        observation
    }
}

impl DiskObservation {
    pub fn from_path(path: &Path) -> Result<Self, ObserveError> {
        if !path.exists() {
            return Ok(Self {
                path: path.to_path_buf(),
                exists: false,
                bytes: 0,
                lines: 0,
                non_empty_lines: 0,
                trinary_only: true,
                zeros: 0,
                ones: 0,
                twos: 0,
                invalid_chars: 0,
                line_preview: Vec::new(),
            });
        }

        let bytes = fs::metadata(path).map_err(ObserveError::Io)?.len();
        let raw = fs::read_to_string(path).map_err(ObserveError::Io)?;
        let mut zeros = 0usize;
        let mut ones = 0usize;
        let mut twos = 0usize;
        let mut invalid_chars = 0usize;

        for ch in raw.chars() {
            match ch {
                '0' => zeros += 1,
                '1' => ones += 1,
                '2' => twos += 1,
                '\n' | '\r' => {}
                _ => invalid_chars += 1,
            }
        }

        let lines: Vec<&str> = raw.lines().collect();
        let line_preview = lines
            .iter()
            .take(8)
            .enumerate()
            .map(|(index, line)| observe_disk_line(index, line))
            .collect();

        Ok(Self {
            path: path.to_path_buf(),
            exists: true,
            bytes,
            lines: lines.len(),
            non_empty_lines: lines.iter().filter(|line| !line.trim().is_empty()).count(),
            trinary_only: invalid_chars == 0,
            zeros,
            ones,
            twos,
            invalid_chars,
            line_preview,
        })
    }
}

pub fn render_overview(store: &NoteStore, path: &Path) -> Result<String, ObserveError> {
    let mut out = String::new();
    render_header(&mut out, &StoreObservation::from_store(store), path)?;
    writeln!(&mut out)?;

    if store.notes().is_empty() {
        writeln!(&mut out, "RAM notes: none")?;
    } else {
        writeln!(&mut out, "RAM notes:")?;
        for note in store.notes() {
            let title = note.title_text().unwrap_or_else(|_| "<invalid utf8>".to_string());
            let title_u640 = u640_count_for_strytes(note.title_payload().len_strytes());
            let body_u640 = u640_count_for_strytes(note.body_payload().len_strytes());
            writeln!(
                &mut out,
                "  #{:<4} {:<24} title {:>4} trits/{:>3} sTrytes/{:>2} U640 | body {:>5} trits/{:>4} sTrytes/{:>2} U640",
                note.id(),
                preview_text(if title.is_empty() { "(untitled)" } else { &title }, 24),
                note.title_payload().len_trits(),
                note.title_payload().len_strytes(),
                title_u640,
                note.body_payload().len_trits(),
                note.body_payload().len_strytes(),
                body_u640
            )?;
        }
    }

    Ok(out)
}

pub fn render_note(
    store: &NoteStore,
    path: &Path,
    id: NoteId,
    active_step: Option<usize>,
) -> Result<String, ObserveError> {
    let note = store.note(id).ok_or(ObserveError::NoteNotFound(id))?;
    let mut out = String::new();
    render_header(&mut out, &StoreObservation::from_store(store), path)?;
    writeln!(&mut out)?;
    writeln!(&mut out, "selected note: #{id}")?;

    let title = PayloadObservation::from_payload("title RAM payload", note.title_payload(), active_step)?;
    let body = PayloadObservation::from_payload("body RAM payload", note.body_payload(), active_step)?;
    render_payload(&mut out, &title)?;
    render_payload(&mut out, &body)?;
    Ok(out)
}

fn render_header(
    out: &mut String,
    store: &StoreObservation,
    path: &Path,
) -> Result<(), ObserveError> {
    let disk = DiskObservation::from_path(path)?;
    writeln!(out, "TritMux observability applet")?;
    writeln!(
        out,
        "RAM  notes={} dirty={} trits={} sTrytes={} pTrytes={} U640={} 9x9_grids={}",
        store.notes,
        store.dirty,
        store.total_trits,
        store.total_strytes,
        store.total_strytes,
        store.total_u640_blocks,
        store.total_grids_9x9
    )?;
    writeln!(
        out,
        "DISK path={} exists={} bytes={} lines={} non_empty={} trinary_only={} digits[0/1/2]={}/{}/{} invalid={}",
        disk.path.display(),
        disk.exists,
        disk.bytes,
        disk.lines,
        disk.non_empty_lines,
        disk.trinary_only,
        disk.zeros,
        disk.ones,
        disk.twos,
        disk.invalid_chars
    )?;
    if !disk.line_preview.is_empty() {
        writeln!(out, "DISK base3 line preview:")?;
        for line in &disk.line_preview {
            writeln!(
                out,
                "  L{:<3} len={:<5} digits[0/1/2]={}/{}/{} {}",
                line.index,
                line.len,
                line.zeros,
                line.ones,
                line.twos,
                line.preview
            )?;
        }
    }
    Ok(())
}

fn render_payload(out: &mut String, observation: &PayloadObservation) -> fmt::Result {
    writeln!(out)?;
    writeln!(out, "[{}]", observation.label)?;
    writeln!(
        out,
        "  trits={} lanes[-/0/+]={}/{}/{} sTrytes={} pTrytes={} U320={} ({} u32 words) U640={} ({} u32 words, {} u64 pairs) 9x9_grids={} zipped_base3_lines={}",
        observation.trits,
        observation.trit_counts.negative,
        observation.trit_counts.neutral,
        observation.trit_counts.positive,
        observation.strytes,
        observation.ptrytes,
        observation.u320_blocks,
        observation.u320_words,
        observation.u640_blocks,
        observation.u640_words,
        observation.u64_pairs,
        observation.grids_9x9,
        observation.zipped_base3_lines
    )?;

    if observation.stryte_preview.is_empty() {
        writeln!(out, "  sTrytes: none")?;
    } else {
        writeln!(out, "  sTryte RAM preview:")?;
        for stryte in &observation.stryte_preview {
            writeln!(
                out,
                "    sTryte#{:<3} u32=0x{:08x} trits={}",
                stryte.index,
                stryte.word,
                stryte.trits
            )?;
        }
    }

    if !observation.u320_preview.is_empty() {
        writeln!(out, "  pTryte/U320 preview:")?;
        for block in &observation.u320_preview {
            writeln!(
                out,
                "    U320#{:<3} pTrytes={:<2} u32[0..{}]={}",
                block.index,
                block.ptryte_count,
                block.preview_words.len(),
                format_u32_words(&block.preview_words)
            )?;
        }
    }

    if !observation.u640_preview.is_empty() {
        writeln!(out, "  U640 + zipped base3 preview:")?;
        for block in &observation.u640_preview {
            writeln!(
                out,
                "    U640#{:<3} u64[0..{}]={} base3_digits={} {}",
                block.index,
                block.preview_u64_pairs.len(),
                format_u64_words(&block.preview_u64_pairs),
                block.base3_digit_len,
                block.base3_preview
            )?;
        }
    }

    writeln!(out, "  compactor: {} step(s)", observation.compaction_steps)?;
    if let Some(window) = &observation.selected_window {
        writeln!(
            out,
            "    active sTryte={} locked=[{}..{}) settle=[{}..{}) convert/lookahead=[{}..{})",
            window.active_index,
            window.locked_start,
            window.locked_end_exclusive,
            window.settle_start,
            window.settle_end_exclusive,
            window.active_guard_start,
            window.active_guard_end_exclusive
        )?;
        writeln!(out, "    map {}", window.map)?;
    } else {
        writeln!(out, "    no lock windows for an empty payload")?;
    }
    Ok(())
}

fn observe_stryte(index: usize, stryte: STryte) -> STryteObservation {
    let word = stryte.word();
    let mut lanes = Vec::new();
    for i in (0..10).rev() {
        let bits = (word >> (i * 2)) & 0b11;
        lanes.push(format!("{:02b}", bits));
    }
    let trits_str = format!("[{}]", lanes.join("|"));

    STryteObservation {
        index,
        word,
        marker_bits_set: stryte.has_u32_marker(),
        trits: trits_str,
    }
}

fn observe_disk_line(index: usize, line: &str) -> DiskLineObservation {
    let mut zeros = 0usize;
    let mut ones = 0usize;
    let mut twos = 0usize;
    for ch in line.chars() {
        match ch {
            '0' => zeros += 1,
            '1' => ones += 1,
            '2' => twos += 1,
            _ => {}
        }
    }

    DiskLineObservation {
        index,
        len: line.len(),
        zeros,
        ones,
        twos,
        preview: preview_text(line, PREVIEW_BASE3_CHARS),
    }
}

fn select_compaction_window(
    windows: &[LockWindow],
    total_strytes: usize,
    active_step: Option<usize>,
) -> Option<CompactionWindowObservation> {
    if windows.is_empty() {
        return None;
    }

    let index = active_step.unwrap_or(0) % windows.len();
    let window = windows[index];
    Some(observe_window(window, total_strytes))
}

fn observe_window(window: LockWindow, total_strytes: usize) -> CompactionWindowObservation {
    let locked = window.range();
    let settle = window.settle_range();
    let active_guard = window.active_guard_range();
    CompactionWindowObservation {
        active_index: window.active_index(),
        locked_start: locked.start,
        locked_end_exclusive: locked.end,
        settle_start: settle.start,
        settle_end_exclusive: settle.end,
        active_guard_start: active_guard.start,
        active_guard_end_exclusive: active_guard.end,
        map: render_window_map(total_strytes, window),
    }
}

fn render_window_map(total_strytes: usize, window: LockWindow) -> String {
    let visible = total_strytes.min(WINDOW_MAP_LIMIT);
    let mut map = String::new();
    for index in 0..visible {
        let marker = if index == window.active_index() {
            'A'
        } else if window.settle_range().contains(&index) {
            'S'
        } else if window.active_guard_range().contains(&index) {
            'L'
        } else {
            '.'
        };
        map.push(marker);
    }
    if total_strytes > visible {
        map.push_str("...");
    }
    map.push_str("  A=active S=settle L=lookahead .=free");
    map
}

fn count_trits(payload: &TritPayload) -> TritCounts {
    let mut counts = TritCounts::default();
    for trit in payload.trits() {
        match trit {
            Trit::Negative => counts.negative += 1,
            Trit::Neutral => counts.neutral += 1,
            Trit::Positive => counts.positive += 1,
        }
    }
    counts
}

fn u64_pairs_from_u32_words<const N: usize>(words: &[u32; N]) -> Vec<u64> {
    words
        .chunks_exact(2)
        .map(|pair| pair[0] as u64 | ((pair[1] as u64) << 32))
        .collect()
}

fn format_u32_words(words: &[u32]) -> String {
    words
        .iter()
        .map(|word| format!("0x{word:08x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn format_u64_words(words: &[u64]) -> String {
    words
        .iter()
        .map(|word| format!("0x{word:016x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn preview_text(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }

    let mut preview: String = text.chars().take(max_chars.saturating_sub(3)).collect();
    preview.push_str("...");
    preview
}

impl fmt::Display for ObserveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ObserveError::Io(err) => write!(f, "{err}"),
            ObserveError::NoteNotFound(id) => write!(f, "note {id} was not found"),
            ObserveError::Window(err) => write!(f, "{err}"),
            ObserveError::Zip(err) => write!(f, "{err}"),
        }
    }
}

impl Error for ObserveError {}

impl From<fmt::Error> for ObserveError {
    fn from(_value: fmt::Error) -> Self {
        ObserveError::Io(io::Error::new(
            io::ErrorKind::Other,
            "failed to render observability view",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::NoteStore;
    use std::env;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn payload_observation_reports_full_pipeline_counts() {
        let payload = TritPayload::from_utf8("observe trits");
        let observation =
            PayloadObservation::from_payload("sample", &payload, Some(2)).unwrap();

        assert_eq!(observation.trits, payload.len_trits());
        assert_eq!(observation.strytes, payload.len_strytes());
        assert_eq!(observation.ptrytes, payload.len_strytes());
        assert_eq!(
            observation.u640_blocks,
            u640_count_for_strytes(payload.len_strytes())
        );
        assert_eq!(observation.u64_pairs, observation.u640_blocks * 10);
        assert!(observation.zipped_base3_lines >= observation.u640_blocks);
        assert!(observation.selected_window.is_some());
    }

    #[test]
    fn disk_observation_counts_trinary_digits() {
        let path = temp_path("observe-disk");
        fs::write(&path, "012\n120\n").unwrap();

        let observation = DiskObservation::from_path(&path).unwrap();
        assert!(observation.exists);
        assert!(observation.trinary_only);
        assert_eq!(observation.lines, 2);
        assert_eq!(observation.zeros, 2);
        assert_eq!(observation.ones, 2);
        assert_eq!(observation.twos, 2);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn note_dashboard_renders_compaction_window() {
        let path = temp_path("observe-render");
        let mut store = NoteStore::new();
        let id = store.add("title", "body");

        let rendered = render_note(&store, &path, id, Some(1)).unwrap();
        assert!(rendered.contains("TritMux observability applet"));
        assert!(rendered.contains("sTryte RAM preview"));
        assert!(rendered.contains("compactor"));
        assert!(rendered.contains("A=active"));
    }

    fn temp_path(label: &str) -> PathBuf {
        env::temp_dir().join(format!(
            "tritmux-{label}-{}.trit",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }
}
