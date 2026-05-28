use crate::notes::{Note, NoteId};
use crate::trit::payload::{PayloadError, TritPayload};
use crate::trit::zip::{
    decode_grid9x9_base3_lines, decode_usize_base3, encode_grid9x9_base3_lines,
    encode_usize_base3, grid9x9_count_for_u640, stryte_count_for_trits,
    u640_count_for_strytes, unzip_u640_grids_to_strytes, ZipError, U640S_PER_GRID9X9,
};
use std::error::Error;
use std::fmt;

const MAGIC_V1: &[u8] = b"TRITMUX_NOTES_V1";
const MAGIC_V2_ZIP: &[u8] = b"TRITMUX_NOTES_ZIP_V2";

#[derive(Debug)]
pub enum ArtifactError {
    MissingMagic,
    InvalidMagic,
    MissingLine(&'static str),
    Payload(PayloadError),
    Zip(ZipError),
    ShortRecord,
    LengthOverflow,
    TrailingBytes,
}

pub fn encode_notes(notes: &[Note]) -> Result<String, ArtifactError> {
    let mut artifact = String::new();
    artifact.push_str(&TritPayload::from_bytes(MAGIC_V2_ZIP).as_storage_digits());
    artifact.push('\n');
    artifact.push_str(&encode_usize_base3(notes.len()));
    artifact.push('\n');

    for note in notes {
        let record = encode_note_record(note)?;
        let grids = record.zipped_u640_grids().map_err(ArtifactError::Zip)?;
        artifact.push_str(&encode_usize_base3(record.len_trits()));
        artifact.push('\n');
        artifact.push_str(&encode_usize_base3(grids.len()));
        artifact.push('\n');

        for grid in &grids {
            for line in encode_grid9x9_base3_lines(grid) {
                artifact.push_str(&line);
                artifact.push('\n');
            }
        }
    }

    Ok(artifact)
}

pub fn decode_notes(artifact: &str) -> Result<Vec<Note>, ArtifactError> {
    let mut lines = artifact.lines().filter(|line| !line.trim().is_empty());
    let Some(magic_line) = lines.next() else {
        return Ok(Vec::new());
    };

    let magic_payload =
        TritPayload::from_storage_digits(magic_line.trim()).map_err(ArtifactError::Payload)?;
    let magic = magic_payload.to_bytes().map_err(ArtifactError::Payload)?;

    if magic == MAGIC_V2_ZIP {
        decode_notes_v2(&mut lines)
    } else if magic == MAGIC_V1 {
        decode_notes_v1_lines(lines)
    } else {
        Err(ArtifactError::InvalidMagic)
    }
}

fn decode_notes_v2<'a, I>(lines: &mut I) -> Result<Vec<Note>, ArtifactError>
where
    I: Iterator<Item = &'a str>,
{
    let note_count_line = next_line(lines, "note count")?;
    let note_count = decode_usize_base3(note_count_line).map_err(ArtifactError::Zip)?;
    let mut notes = Vec::with_capacity(note_count);

    for _ in 0..note_count {
        let trit_len = decode_usize_base3(next_line(lines, "record trit length")?)
            .map_err(ArtifactError::Zip)?;
        let grid_count =
            decode_usize_base3(next_line(lines, "record 9x9 U640 grid count")?)
                .map_err(ArtifactError::Zip)?;

        let stryte_count = stryte_count_for_trits(trit_len);
        let expected_grids = grid9x9_count_for_u640(u640_count_for_strytes(stryte_count));
        if grid_count != expected_grids {
            return Err(ArtifactError::Zip(ZipError::InsufficientBlocks {
                needed: expected_grids,
                available: grid_count,
            }));
        }

        let mut grids = Vec::with_capacity(grid_count);
        for _ in 0..grid_count {
            let mut grid_lines = Vec::with_capacity(U640S_PER_GRID9X9);
            for _ in 0..U640S_PER_GRID9X9 {
                grid_lines.push(next_line(lines, "9x9 U640 base3 block")?);
            }
            grids.push(decode_grid9x9_base3_lines(&grid_lines).map_err(ArtifactError::Zip)?);
        }

        let strytes =
            unzip_u640_grids_to_strytes(&grids, stryte_count).map_err(ArtifactError::Zip)?;
        let payload =
            TritPayload::from_strytes(strytes, trit_len).map_err(ArtifactError::Payload)?;
        notes.push(decode_note_record(&payload)?);
    }

    Ok(notes)
}

fn decode_notes_v1_lines<'a, I>(lines: I) -> Result<Vec<Note>, ArtifactError>
where
    I: Iterator<Item = &'a str>,
{
    let mut notes = Vec::new();
    for line in lines {
        let payload =
            TritPayload::from_storage_digits(line.trim()).map_err(ArtifactError::Payload)?;
        notes.push(decode_note_record(&payload)?);
    }

    Ok(notes)
}

fn next_line<'a, I>(lines: &mut I, name: &'static str) -> Result<&'a str, ArtifactError>
where
    I: Iterator<Item = &'a str>,
{
    lines.next().map(str::trim).ok_or(ArtifactError::MissingLine(name))
}

fn encode_note_record(note: &Note) -> Result<TritPayload, ArtifactError> {
    let title = note.title_bytes().map_err(ArtifactError::Payload)?;
    let body = note.body_bytes().map_err(ArtifactError::Payload)?;

    let title_len = u32::try_from(title.len()).map_err(|_| ArtifactError::LengthOverflow)?;
    let body_len = u32::try_from(body.len()).map_err(|_| ArtifactError::LengthOverflow)?;

    let mut bytes = Vec::with_capacity(16 + title.len() + body.len());
    bytes.extend_from_slice(&note.id().to_le_bytes());
    bytes.extend_from_slice(&title_len.to_le_bytes());
    bytes.extend_from_slice(&body_len.to_le_bytes());
    bytes.extend_from_slice(&title);
    bytes.extend_from_slice(&body);
    Ok(TritPayload::from_bytes(&bytes))
}

fn decode_note_record(payload: &TritPayload) -> Result<Note, ArtifactError> {
    let bytes = payload.to_bytes().map_err(ArtifactError::Payload)?;
    if bytes.len() < 16 {
        return Err(ArtifactError::ShortRecord);
    }

    let mut id_bytes = [0u8; 8];
    id_bytes.copy_from_slice(&bytes[0..8]);
    let id = NoteId::from_le_bytes(id_bytes);

    let mut title_len_bytes = [0u8; 4];
    title_len_bytes.copy_from_slice(&bytes[8..12]);
    let title_len = u32::from_le_bytes(title_len_bytes) as usize;

    let mut body_len_bytes = [0u8; 4];
    body_len_bytes.copy_from_slice(&bytes[12..16]);
    let body_len = u32::from_le_bytes(body_len_bytes) as usize;

    let title_start = 16usize;
    let body_start = title_start
        .checked_add(title_len)
        .ok_or(ArtifactError::LengthOverflow)?;
    let end = body_start
        .checked_add(body_len)
        .ok_or(ArtifactError::LengthOverflow)?;

    if end > bytes.len() {
        return Err(ArtifactError::ShortRecord);
    }
    if end != bytes.len() {
        return Err(ArtifactError::TrailingBytes);
    }

    Ok(Note::from_payloads(
        id,
        TritPayload::from_bytes(&bytes[title_start..body_start]),
        TritPayload::from_bytes(&bytes[body_start..end]),
    ))
}

impl fmt::Display for ArtifactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArtifactError::MissingMagic => write!(f, "missing trinary artifact magic"),
            ArtifactError::InvalidMagic => write!(f, "invalid trinary artifact magic"),
            ArtifactError::MissingLine(name) => write!(f, "missing trinary artifact line: {name}"),
            ArtifactError::Payload(err) => write!(f, "{err}"),
            ArtifactError::Zip(err) => write!(f, "{err}"),
            ArtifactError::ShortRecord => write!(f, "short trinary note record"),
            ArtifactError::LengthOverflow => write!(f, "trinary note record length overflow"),
            ArtifactError::TrailingBytes => write!(f, "trinary note record has trailing bytes"),
        }
    }
}

impl Error for ArtifactError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_artifact_uses_only_trinary_digits_and_newlines() {
        let notes = vec![Note::new(1, "first", "body")];
        let artifact = encode_notes(&notes).unwrap();
        assert!(artifact
            .chars()
            .all(|ch| matches!(ch, '0' | '1' | '2' | '\n')));

        let decoded = decode_notes(&artifact).unwrap();
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].title_text().unwrap(), "first");
        assert_eq!(decoded[0].body_text().unwrap(), "body");
    }

    #[test]
    fn note_artifact_stores_fixed_9_by_9_u640_grid_lines() {
        let notes = vec![Note::new(1, "grid", "one payload")];
        let artifact = encode_notes(&notes).unwrap();
        let lines: Vec<&str> = artifact.lines().collect();

        assert_eq!(lines.len(), 1 + 1 + 1 + 1 + U640S_PER_GRID9X9);
        assert_eq!(decode_usize_base3(lines[1]).unwrap(), 1);
        assert_eq!(decode_usize_base3(lines[3]).unwrap(), 1);
    }
}
