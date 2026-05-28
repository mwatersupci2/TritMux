use crate::notes::{NoteId, NoteStore, NoteStoreError};
use crate::observe::{self, ObserveError};
use std::env;
use std::error::Error;
use std::fmt;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const DEFAULT_COLUMNS: usize = 120;
const DEFAULT_ROWS: usize = 36;
const MIN_COLUMNS: usize = 78;
const MIN_ROWS: usize = 20;
const COMMAND_ROWS: usize = 4;

#[derive(Debug)]
pub enum TuiError {
    Io(io::Error),
    Store(NoteStoreError),
    Observe(ObserveError),
    MissingSelection,
    InvalidNoteId(String),
    InvalidLineNumber(String),
    InvalidCommand(String),
}

#[derive(Debug, Clone)]
struct TuiState {
    selected: Option<NoteId>,
    source_file: Option<PathBuf>,
    compaction_step: usize,
    status: String,
}

#[derive(Debug, Clone, Copy)]
struct TerminalSize {
    columns: usize,
    rows: usize,
}

pub fn run(store: &mut NoteStore, artifact_path: &Path) -> Result<(), TuiError> {
    let mut state = TuiState {
        selected: first_note_id(store).or_else(|| Some(store.add("untitled", ""))),
        source_file: None,
        compaction_step: 0,
        status: "TUI ready".to_string(),
    };

    loop {
        state.compaction_step = state.compaction_step.saturating_add(1);
        let size = terminal_size();
        print!("{}", render_app(store, artifact_path, &state, size)?);
        print!("cmd> ");
        io::stdout().flush().map_err(TuiError::Io)?;

        let mut input = String::new();
        io::stdin().read_line(&mut input).map_err(TuiError::Io)?;
        let input = input.trim_end_matches(['\r', '\n']);
        if input.trim().is_empty() {
            continue;
        }

        if handle_command(store, artifact_path, &mut state, input)? {
            break;
        }
    }

    Ok(())
}

fn handle_command(
    store: &mut NoteStore,
    artifact_path: &Path,
    state: &mut TuiState,
    input: &str,
) -> Result<bool, TuiError> {
    let mut parts = input.trim().splitn(2, char::is_whitespace);
    let command = parts.next().unwrap_or_default();
    let rest = parts.next().unwrap_or_default().trim();

    match command {
        "q" | "quit" | "exit" => {
            if store.flush_if_dirty(artifact_path).map_err(TuiError::Store)? {
                state.status = format!("saved {}", artifact_path.display());
            }
            Ok(true)
        }
        "q!" | "quit!" | "exit!" => Ok(true),
        "new" => {
            let title = if rest.is_empty() { "untitled" } else { rest };
            let id = store.add(title, "");
            state.selected = Some(id);
            state.source_file = None;
            state.status = format!("created note {id}");
            Ok(false)
        }
        "select" | "open-note" => {
            let id = parse_note_id(rest)?;
            if store.note(id).is_none() {
                return Err(TuiError::Store(NoteStoreError::NotFound(id)));
            }
            state.selected = Some(id);
            state.status = format!("selected note {id}");
            Ok(false)
        }
        "title" => {
            let id = selected_id(state)?;
            let (_, body) = note_text(store, id)?;
            store.replace(id, rest, &body).map_err(TuiError::Store)?;
            state.status = format!("renamed note {id}");
            Ok(false)
        }
        "append" | "a" => {
            let id = selected_id(state)?;
            let (title, body) = note_text(store, id)?;
            let next = append_line(&body, rest);
            store.replace(id, &title, &next).map_err(TuiError::Store)?;
            state.status = format!("appended line to note {id}");
            Ok(false)
        }
        "set" => {
            let (line, text) = parse_line_text(rest)?;
            let id = selected_id(state)?;
            let (title, body) = note_text(store, id)?;
            let next = set_line(&body, line, text)?;
            store.replace(id, &title, &next).map_err(TuiError::Store)?;
            state.status = format!("set line {line}");
            Ok(false)
        }
        "del" | "delete-line" => {
            let line = parse_line_number(rest)?;
            let id = selected_id(state)?;
            let (title, body) = note_text(store, id)?;
            let next = delete_line(&body, line)?;
            store.replace(id, &title, &next).map_err(TuiError::Store)?;
            state.status = format!("deleted line {line}");
            Ok(false)
        }
        "clear" => {
            let id = selected_id(state)?;
            let (title, _) = note_text(store, id)?;
            store.replace(id, &title, "").map_err(TuiError::Store)?;
            state.status = format!("cleared note {id}");
            Ok(false)
        }
        "body" | "replace" => {
            let id = selected_id(state)?;
            let (title, _) = note_text(store, id)?;
            println!("enter body, finish with a single '.' line");
            let body = read_body()?;
            store.replace(id, &title, &body).map_err(TuiError::Store)?;
            state.status = format!("replaced note {id} body");
            Ok(false)
        }
        "open" | "import" => {
            let file = parse_path(rest)?;
            let text = fs::read_to_string(&file).map_err(TuiError::Io)?;
            let title = file
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("imported text");
            let id = store.add(title, &text);
            state.selected = Some(id);
            state.source_file = Some(file.clone());
            state.status = format!("opened {} into note {id}", file.display());
            Ok(false)
        }
        "write" | "export" => {
            let id = selected_id(state)?;
            let path = if rest.is_empty() {
                state
                    .source_file
                    .clone()
                    .ok_or_else(|| TuiError::InvalidCommand("write needs a path".to_string()))?
            } else {
                parse_path(rest)?
            };
            let (_, body) = note_text(store, id)?;
            fs::write(&path, body).map_err(TuiError::Io)?;
            state.source_file = Some(path.clone());
            state.status = format!("wrote {}", path.display());
            Ok(false)
        }
        "save" | "flush" => {
            store.save(artifact_path).map_err(TuiError::Store)?;
            state.status = format!("saved {}", artifact_path.display());
            Ok(false)
        }
        "watch" | "tick" => {
            state.compaction_step = state.compaction_step.saturating_add(1);
            state.status = format!("advanced compactor frame {}", state.compaction_step);
            Ok(false)
        }
        "help" | "?" => {
            state.status = "commands: new/select/title/append/set/del/body/open/write/save/watch/quit".to_string();
            Ok(false)
        }
        other => Err(TuiError::InvalidCommand(other.to_string())),
    }
}

fn render_app(
    store: &NoteStore,
    artifact_path: &Path,
    state: &TuiState,
    size: TerminalSize,
) -> Result<String, TuiError> {
    let columns = size.columns.max(MIN_COLUMNS);
    let rows = size.rows.max(MIN_ROWS);
    let content_rows = rows.saturating_sub(COMMAND_ROWS).max(12);
    let divider_width = 1usize;
    let left_width = (columns * 47 / 100).max(38);
    let right_width = columns.saturating_sub(left_width + divider_width).max(30);
    let left_lines = editor_lines(store, state, artifact_path, content_rows.saturating_sub(2))?;
    let right_lines = observation_lines(store, artifact_path, state)?;

    let mut out = String::new();
    write!(&mut out, "\x1b[2J\x1b[H")?;
    let left_frame = frame_lines(
        "Editor",
        &left_lines,
        left_width,
        content_rows,
    );
    let right_frame = frame_lines(
        "Trinary Observability",
        &right_lines,
        right_width,
        content_rows,
    );

    for row in 0..content_rows {
        let left = left_frame.get(row).map(String::as_str).unwrap_or("");
        let right = right_frame.get(row).map(String::as_str).unwrap_or("");
        writeln!(&mut out, "{left}|{right}")?;
    }

    writeln!(
        &mut out,
        "{}",
        "-".repeat(columns.min(left_width + right_width + divider_width))
    )?;
    writeln!(&mut out, "{}", command_hint(columns))?;
    writeln!(&mut out, "status: {}", crop(&state.status, columns.saturating_sub(8)))?;
    Ok(out)
}

fn editor_lines(
    store: &NoteStore,
    state: &TuiState,
    artifact_path: &Path,
    max_body_rows: usize,
) -> Result<Vec<String>, TuiError> {
    let mut lines = Vec::new();
    lines.push(format!(
        "artifact: {}{}",
        artifact_path.display(),
        if store.is_dirty() { " *dirty" } else { "" }
    ));
    if let Some(file) = &state.source_file {
        lines.push(format!("text file: {}", file.display()));
    } else {
        lines.push("text file: <none; use open/write>".to_string());
    }
    lines.push(format!("notes in RAM: {}", store.notes().len()));

    let Some(id) = state.selected else {
        lines.push("no selected note".to_string());
        return Ok(lines);
    };
    let note = store.note(id).ok_or(NoteStoreError::NotFound(id)).map_err(TuiError::Store)?;
    let title = note.title_text().map_err(NoteStoreError::Payload).map_err(TuiError::Store)?;
    let body = note.body_text().map_err(NoteStoreError::Payload).map_err(TuiError::Store)?;
    lines.push(format!("selected: #{id} {title}"));
    lines.push(format!(
        "payload: title {} trits/{} sTrytes | body {} trits/{} sTrytes",
        note.title_payload().len_trits(),
        note.title_payload().len_strytes(),
        note.body_payload().len_trits(),
        note.body_payload().len_strytes()
    ));
    lines.push(String::new());
    lines.push("body:".to_string());

    let body_lines: Vec<&str> = if body.is_empty() {
        Vec::new()
    } else {
        body.lines().collect()
    };
    if body_lines.is_empty() {
        lines.push("  1 | ".to_string());
    } else {
        for (index, line) in body_lines.iter().take(max_body_rows).enumerate() {
            lines.push(format!("{:>3} | {}", index + 1, line));
        }
        if body_lines.len() > max_body_rows {
            lines.push(format!("... {} more line(s)", body_lines.len() - max_body_rows));
        }
    }

    Ok(lines)
}

fn observation_lines(
    store: &NoteStore,
    artifact_path: &Path,
    state: &TuiState,
) -> Result<Vec<String>, TuiError> {
    let rendered = match state.selected {
        Some(id) if store.note(id).is_some() => {
            observe::render_note(store, artifact_path, id, Some(state.compaction_step))
                .map_err(TuiError::Observe)?
        }
        _ => observe::render_overview(store, artifact_path).map_err(TuiError::Observe)?,
    };
    Ok(rendered.lines().map(str::to_string).collect())
}

fn frame_lines(title: &str, lines: &[String], width: usize, height: usize) -> Vec<String> {
    let width = width.max(10);
    let inner = width.saturating_sub(2);
    let mut frame = Vec::with_capacity(height);
    let title = crop(title, inner.saturating_sub(2));
    let title_segment = format!(" {title} ");
    let top_fill = inner.saturating_sub(title_segment.len());
    frame.push(format!("+{title_segment}{}+", "-".repeat(top_fill)));

    let body_rows = height.saturating_sub(2);
    for row in 0..body_rows {
        let text = lines.get(row).map(String::as_str).unwrap_or("");
        let cropped = crop(text, inner);
        frame.push(format!("|{cropped:<inner$}|"));
    }

    frame.push(format!("+{}+", "-".repeat(inner)));
    frame
}

fn command_hint(columns: usize) -> String {
    crop(
        "new <title> | select <id> | title <text> | append <text> | set <line> <text> | del <line> | body | open <file.md> | write [file.md] | save | watch | quit",
        columns,
    )
}

fn terminal_size() -> TerminalSize {
    let columns = env::var("COLUMNS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(DEFAULT_COLUMNS);
    let rows = env::var("LINES")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(DEFAULT_ROWS);
    TerminalSize { columns, rows }
}

fn first_note_id(store: &NoteStore) -> Option<NoteId> {
    store.notes().first().map(|note| note.id())
}

fn selected_id(state: &TuiState) -> Result<NoteId, TuiError> {
    state.selected.ok_or(TuiError::MissingSelection)
}

fn note_text(store: &NoteStore, id: NoteId) -> Result<(String, String), TuiError> {
    let note = store.note(id).ok_or(NoteStoreError::NotFound(id)).map_err(TuiError::Store)?;
    let title = note.title_text().map_err(NoteStoreError::Payload).map_err(TuiError::Store)?;
    let body = note.body_text().map_err(NoteStoreError::Payload).map_err(TuiError::Store)?;
    Ok((title, body))
}

fn append_line(body: &str, text: &str) -> String {
    if body.is_empty() {
        text.to_string()
    } else {
        format!("{body}\n{text}")
    }
}

fn set_line(body: &str, line: usize, text: &str) -> Result<String, TuiError> {
    if line == 0 {
        return Err(TuiError::InvalidLineNumber(line.to_string()));
    }
    let mut lines: Vec<String> = if body.is_empty() {
        vec![String::new()]
    } else {
        body.lines().map(str::to_string).collect()
    };
    if line > lines.len() {
        return Err(TuiError::InvalidLineNumber(line.to_string()));
    }
    lines[line - 1] = text.to_string();
    Ok(lines.join("\n"))
}

fn delete_line(body: &str, line: usize) -> Result<String, TuiError> {
    if line == 0 {
        return Err(TuiError::InvalidLineNumber(line.to_string()));
    }
    let mut lines: Vec<String> = body.lines().map(str::to_string).collect();
    if line > lines.len() {
        return Err(TuiError::InvalidLineNumber(line.to_string()));
    }
    lines.remove(line - 1);
    Ok(lines.join("\n"))
}

fn parse_note_id(text: &str) -> Result<NoteId, TuiError> {
    if text.is_empty() {
        return Err(TuiError::InvalidNoteId("<missing>".to_string()));
    }
    text.parse::<NoteId>()
        .map_err(|_| TuiError::InvalidNoteId(text.to_string()))
}

fn parse_line_number(text: &str) -> Result<usize, TuiError> {
    if text.is_empty() {
        return Err(TuiError::InvalidLineNumber("<missing>".to_string()));
    }
    text.parse::<usize>()
        .ok()
        .filter(|line| *line > 0)
        .ok_or_else(|| TuiError::InvalidLineNumber(text.to_string()))
}

fn parse_line_text(text: &str) -> Result<(usize, &str), TuiError> {
    let mut parts = text.splitn(2, char::is_whitespace);
    let line = parse_line_number(parts.next().unwrap_or_default())?;
    let value = parts.next().unwrap_or_default();
    Ok((line, value))
}

fn parse_path(text: &str) -> Result<PathBuf, TuiError> {
    if text.is_empty() {
        return Err(TuiError::InvalidCommand("missing path".to_string()));
    }
    Ok(PathBuf::from(text))
}

fn read_body() -> Result<String, TuiError> {
    let mut lines = Vec::new();
    loop {
        print!("body> ");
        io::stdout().flush().map_err(TuiError::Io)?;
        let mut input = String::new();
        io::stdin().read_line(&mut input).map_err(TuiError::Io)?;
        let line = input.trim_end_matches(['\r', '\n']).to_string();
        if line == "." {
            break;
        }
        lines.push(line);
    }
    Ok(lines.join("\n"))
}

fn crop(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let mut out = String::new();
    for ch in text.chars().take(width) {
        if matches!(ch, '\n' | '\r' | '\t') {
            out.push(' ');
        } else {
            out.push(ch);
        }
    }
    out
}

impl fmt::Display for TuiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TuiError::Io(err) => write!(f, "{err}"),
            TuiError::Store(err) => write!(f, "{err}"),
            TuiError::Observe(err) => write!(f, "{err}"),
            TuiError::MissingSelection => write!(f, "no note is selected"),
            TuiError::InvalidNoteId(value) => write!(f, "invalid note id '{value}'"),
            TuiError::InvalidLineNumber(value) => write!(f, "invalid line number '{value}'"),
            TuiError::InvalidCommand(value) => write!(f, "invalid TUI command '{value}'"),
        }
    }
}

impl Error for TuiError {}

impl From<fmt::Error> for TuiError {
    fn from(_value: fmt::Error) -> Self {
        TuiError::Io(io::Error::new(
            io::ErrorKind::Other,
            "failed to render TUI",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn split_pane_render_contains_editor_and_observability() {
        let mut store = NoteStore::new();
        let id = store.add("doc.md", "# Heading\nbody");
        let state = TuiState {
            selected: Some(id),
            source_file: Some(PathBuf::from("doc.md")),
            compaction_step: 1,
            status: "testing".to_string(),
        };

        let rendered = render_app(
            &store,
            &PathBuf::from("notes.trit"),
            &state,
            TerminalSize {
                columns: 120,
                rows: 32,
            },
        )
        .unwrap();

        assert!(rendered.contains("Editor"));
        assert!(rendered.contains("Trinary Observability"));
        assert!(rendered.contains("# Heading"));
        assert!(rendered.contains("sTryte"));
        assert!(rendered.contains("cmd>") == false);
    }

    #[test]
    fn line_edit_commands_update_selected_note() {
        let mut store = NoteStore::new();
        let id = store.add("note", "first");
        let mut state = TuiState {
            selected: Some(id),
            source_file: None,
            compaction_step: 0,
            status: String::new(),
        };
        let path = temp_path("tui-edit");

        handle_command(&mut store, &path, &mut state, "append second").unwrap();
        handle_command(&mut store, &path, &mut state, "set 1 changed").unwrap();
        handle_command(&mut store, &path, &mut state, "del 2").unwrap();

        assert_eq!(store.note(id).unwrap().body_text().unwrap(), "changed");
    }

    #[test]
    fn open_and_write_plain_text_file() {
        let input = temp_path("tui-input.md");
        let output = temp_path("tui-output.md");
        fs::write(&input, "# Title\nbody").unwrap();
        let mut store = NoteStore::new();
        let mut state = TuiState {
            selected: None,
            source_file: None,
            compaction_step: 0,
            status: String::new(),
        };
        let artifact = temp_path("tui-artifact.trit");

        handle_command(
            &mut store,
            &artifact,
            &mut state,
            &format!("open {}", input.display()),
        )
        .unwrap();
        handle_command(
            &mut store,
            &artifact,
            &mut state,
            &format!("write {}", output.display()),
        )
        .unwrap();

        assert_eq!(fs::read_to_string(&output).unwrap(), "# Title\nbody");
        let _ = fs::remove_file(input);
        let _ = fs::remove_file(output);
        let _ = fs::remove_file(artifact);
    }

    fn temp_path(label: &str) -> PathBuf {
        env::temp_dir().join(format!(
            "tritmux-{label}-{}.tmp",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }
}
