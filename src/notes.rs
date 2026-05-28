use crate::trit::payload::{PayloadError, TritPayload};
use crate::trit::storage_artifact::{self, ArtifactError};
use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

pub type NoteId = u64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    id: NoteId,
    title: TritPayload,
    body: TritPayload,
}

#[derive(Debug, Default)]
pub struct NoteStore {
    notes: Vec<Note>,
    next_id: NoteId,
    dirty: bool,
}

#[derive(Debug)]
pub enum NoteStoreError {
    Io(io::Error),
    Payload(PayloadError),
    Artifact(ArtifactError),
    NotFound(NoteId),
}

impl Note {
    pub fn new(id: NoteId, title: &str, body: &str) -> Self {
        Self {
            id,
            title: TritPayload::from_utf8(title),
            body: TritPayload::from_utf8(body),
        }
    }

    pub fn from_payloads(id: NoteId, title: TritPayload, body: TritPayload) -> Self {
        Self { id, title, body }
    }

    pub fn id(&self) -> NoteId {
        self.id
    }

    pub fn title_text(&self) -> Result<String, PayloadError> {
        self.title.to_utf8_string()
    }

    pub fn body_text(&self) -> Result<String, PayloadError> {
        self.body.to_utf8_string()
    }

    pub fn title_bytes(&self) -> Result<Vec<u8>, PayloadError> {
        self.title.to_bytes()
    }

    pub fn body_bytes(&self) -> Result<Vec<u8>, PayloadError> {
        self.body.to_bytes()
    }

    pub fn title_payload(&self) -> &TritPayload {
        &self.title
    }

    pub fn body_payload(&self) -> &TritPayload {
        &self.body
    }

    pub fn replace_text(&mut self, title: &str, body: &str) {
        self.title = TritPayload::from_utf8(title);
        self.body = TritPayload::from_utf8(body);
    }
}

impl NoteStore {
    pub fn new() -> Self {
        Self {
            notes: Vec::new(),
            next_id: 1,
            dirty: false,
        }
    }

    pub fn from_notes(notes: Vec<Note>) -> Self {
        let next_id = notes.iter().map(Note::id).max().unwrap_or(0).saturating_add(1);
        Self {
            notes,
            next_id,
            dirty: false,
        }
    }

    pub fn load(path: &Path) -> Result<Self, NoteStoreError> {
        if !path.exists() {
            return Ok(Self::new());
        }

        let artifact = fs::read_to_string(path).map_err(NoteStoreError::Io)?;
        let notes = storage_artifact::decode_notes(&artifact).map_err(NoteStoreError::Artifact)?;
        Ok(Self::from_notes(notes))
    }

    pub fn save(&mut self, path: &Path) -> Result<(), NoteStoreError> {
        if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            fs::create_dir_all(parent).map_err(NoteStoreError::Io)?;
        }

        let artifact =
            storage_artifact::encode_notes(&self.notes).map_err(NoteStoreError::Artifact)?;
        fs::write(path, artifact).map_err(NoteStoreError::Io)?;
        self.dirty = false;
        Ok(())
    }

    pub fn flush_if_dirty(&mut self, path: &Path) -> Result<bool, NoteStoreError> {
        if self.dirty {
            self.save(path)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn add(&mut self, title: &str, body: &str) -> NoteId {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.notes.push(Note::new(id, title, body));
        self.dirty = true;
        id
    }

    pub fn replace(&mut self, id: NoteId, title: &str, body: &str) -> Result<(), NoteStoreError> {
        let note = self.note_mut(id).ok_or(NoteStoreError::NotFound(id))?;
        note.replace_text(title, body);
        self.dirty = true;
        Ok(())
    }

    pub fn delete(&mut self, id: NoteId) -> Result<(), NoteStoreError> {
        let Some(index) = self.notes.iter().position(|note| note.id == id) else {
            return Err(NoteStoreError::NotFound(id));
        };
        self.notes.remove(index);
        self.dirty = true;
        Ok(())
    }

    pub fn notes(&self) -> &[Note] {
        &self.notes
    }

    pub fn note(&self, id: NoteId) -> Option<&Note> {
        self.notes.iter().find(|note| note.id == id)
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn note_mut(&mut self, id: NoteId) -> Option<&mut Note> {
        self.notes.iter_mut().find(|note| note.id == id)
    }
}

impl fmt::Display for NoteStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NoteStoreError::Io(err) => write!(f, "{err}"),
            NoteStoreError::Payload(err) => write!(f, "{err}"),
            NoteStoreError::Artifact(err) => write!(f, "{err}"),
            NoteStoreError::NotFound(id) => write!(f, "note {id} was not found"),
        }
    }
}

impl Error for NoteStoreError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn store_saves_and_loads_trinary_artifact() {
        let mut store = NoteStore::new();
        let id = store.add("alpha", "first body");

        let path = env::temp_dir().join(format!(
            "tritmux-test-{}.trit",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        store.save(&path).unwrap();
        let raw = fs::read_to_string(&path).unwrap();
        assert!(raw.chars().all(|ch| matches!(ch, '0' | '1' | '2' | '\n')));

        let loaded = NoteStore::load(&path).unwrap();
        let note = loaded.note(id).unwrap();
        assert_eq!(note.title_text().unwrap(), "alpha");
        assert_eq!(note.body_text().unwrap(), "first body");

        let _ = fs::remove_file(path);
    }
}
