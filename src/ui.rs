use crate::notes::{NoteId, NoteStore, NoteStoreError};
use crate::observe::{self, ObserveError};
use crate::trit::zip::{grid9x9_count_for_u640, u640_count_for_strytes};
use crate::tui::{self, TuiError};
use std::error::Error;
use std::fmt;
use std::io::{self, Write};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

#[derive(Debug)]
pub enum UiError {
    Io(io::Error),
    Store(NoteStoreError),
    Observe(ObserveError),
    Tui(TuiError),
    InvalidNoteId(String),
    InvalidFrameCount(String),
}

pub fn run(path: PathBuf) -> Result<(), UiError> {
    let mut store = NoteStore::load(&path).map_err(UiError::Store)?;

    println!("TritMux note shell");
    println!("artifact: {}", path.display());
    println!("type 'help' for commands");

    loop {
        print_prompt(if store.is_dirty() { "tritmux*" } else { "tritmux" })?;
        let input = read_line()?;
        let trimmed = input.trim();
        if trimmed.is_empty() {
            continue;
        }

        let mut parts = trimmed.splitn(2, char::is_whitespace);
        let command = parts.next().unwrap_or_default();
        let rest = parts.next().unwrap_or_default().trim();

        match command {
            "new" | "add" => add_note(&mut store)?,
            "list" | "ls" => list_notes(&store)?,
            "show" | "open" => show_note(&store, parse_id(rest)?)?,
            "edit" => edit_note(&mut store, parse_id(rest)?)?,
            "delete" | "del" | "rm" => delete_note(&mut store, parse_id(rest)?)?,
            "save" | "flush" => {
                store.save(&path).map_err(UiError::Store)?;
                println!("saved {}", path.display());
            }
            "observe" | "obs" => observe_notes(&store, &path, rest)?,
            "watch" => watch_observation(&store, &path, rest)?,
            "tui" => tui::run(&mut store, &path).map_err(UiError::Tui)?,
            "path" => println!("{}", path.display()),
            "help" | "?" => print_help(),
            "quit" | "exit" => {
                if store.flush_if_dirty(&path).map_err(UiError::Store)? {
                    println!("saved {}", path.display());
                }
                break;
            }
            "quit!" | "exit!" => break,
            unknown => {
                println!("unknown command '{unknown}'");
                print_help();
            }
        }
    }

    Ok(())
}

fn observe_notes(store: &NoteStore, path: &PathBuf, rest: &str) -> Result<(), UiError> {
    let rendered = match rest {
        "" | "all" | "disk" => observe::render_overview(store, path).map_err(UiError::Observe)?,
        other => observe::render_note(store, path, parse_id(other)?, None)
            .map_err(UiError::Observe)?,
    };
    println!("{rendered}");
    Ok(())
}

fn watch_observation(store: &NoteStore, path: &PathBuf, rest: &str) -> Result<(), UiError> {
    let (id, frames) = parse_watch_args(rest)?;
    for frame in 0..frames {
        print!("\x1b[2J\x1b[H");
        println!(
            "watching note {id} frame {}/{}; Ctrl-C exits the process",
            frame + 1,
            frames
        );
        let rendered = observe::render_note(store, path, id, Some(frame))
            .map_err(UiError::Observe)?;
        println!("{rendered}");
        io::stdout().flush().map_err(UiError::Io)?;
        if frame + 1 < frames {
            thread::sleep(Duration::from_millis(250));
        }
    }
    Ok(())
}

fn add_note(store: &mut NoteStore) -> Result<(), UiError> {
    let title = prompt_line("title")?;
    println!("body, finish with a single '.' line");
    let body = read_body()?;
    let id = store.add(title.trim(), &body);
    println!("created note {id}");
    Ok(())
}

fn list_notes(store: &NoteStore) -> Result<(), UiError> {
    if store.notes().is_empty() {
        println!("no notes");
        return Ok(());
    }

    for note in store.notes() {
        let title = note.title_text().map_err(NoteStoreError::Payload)?;
        let body = note.body_text().map_err(NoteStoreError::Payload)?;
        let summary = summarize(&body, 56);
        println!(
            "{:>4}  {}  [{} title trits/{} sTrytes, {} body trits/{} sTrytes]",
            note.id(),
            if title.is_empty() { "(untitled)" } else { &title },
            note.title_payload().len_trits(),
            note.title_payload().len_strytes(),
            note.body_payload().len_trits(),
            note.body_payload().len_strytes()
        );
        if !summary.is_empty() {
            println!("      {summary}");
        }
    }

    Ok(())
}

fn show_note(store: &NoteStore, id: NoteId) -> Result<(), UiError> {
    let note = store.note(id).ok_or(NoteStoreError::NotFound(id))?;
    let title = note.title_text().map_err(NoteStoreError::Payload)?;
    let body = note.body_text().map_err(NoteStoreError::Payload)?;
    println!("#{id} {title}");
    println!(
        "title: {} trits, {} sTrytes, {} U640, {} 9x9 grid(s)",
        note.title_payload().len_trits(),
        note.title_payload().len_strytes(),
        u640_count_for_strytes(note.title_payload().len_strytes()),
        grid9x9_count_for_u640(u640_count_for_strytes(note.title_payload().len_strytes()))
    );
    println!(
        "body: {} trits, {} sTrytes, {} U640, {} 9x9 grid(s)",
        note.body_payload().len_trits(),
        note.body_payload().len_strytes(),
        u640_count_for_strytes(note.body_payload().len_strytes()),
        grid9x9_count_for_u640(u640_count_for_strytes(note.body_payload().len_strytes()))
    );
    println!();
    println!("{body}");
    Ok(())
}

fn edit_note(store: &mut NoteStore, id: NoteId) -> Result<(), UiError> {
    let note = store.note(id).ok_or(NoteStoreError::NotFound(id))?;
    let old_title = note.title_text().map_err(NoteStoreError::Payload)?;
    let old_body = note.body_text().map_err(NoteStoreError::Payload)?;

    println!("current title: {old_title}");
    let title = prompt_line("new title, blank keeps current")?;
    let final_title = if title.trim().is_empty() {
        old_title
    } else {
        title.trim().to_string()
    };

    println!("replace body? y/N");
    print_prompt("replace")?;
    let answer = read_line()?;
    let final_body = if matches!(answer.trim(), "y" | "Y" | "yes" | "YES") {
        println!("new body, finish with a single '.' line");
        read_body()?
    } else {
        old_body
    };

    store
        .replace(id, &final_title, &final_body)
        .map_err(UiError::Store)?;
    println!("updated note {id}");
    Ok(())
}

fn delete_note(store: &mut NoteStore, id: NoteId) -> Result<(), UiError> {
    let note = store.note(id).ok_or(NoteStoreError::NotFound(id))?;
    let title = note.title_text().map_err(NoteStoreError::Payload)?;
    println!("delete note {id} '{title}'? y/N");
    print_prompt("delete")?;
    let answer = read_line()?;
    if matches!(answer.trim(), "y" | "Y" | "yes" | "YES") {
        store.delete(id).map_err(UiError::Store)?;
        println!("deleted note {id}");
    } else {
        println!("delete cancelled");
    }
    Ok(())
}

fn parse_id(text: &str) -> Result<NoteId, UiError> {
    if text.is_empty() {
        return Err(UiError::InvalidNoteId("<missing>".to_string()));
    }

    text.parse::<NoteId>()
        .map_err(|_| UiError::InvalidNoteId(text.to_string()))
}

fn parse_watch_args(text: &str) -> Result<(NoteId, usize), UiError> {
    let mut parts = text.split_whitespace();
    let id_text = parts.next().unwrap_or_default();
    let id = parse_id(id_text)?;
    let frames = match parts.next() {
        Some(value) => value
            .parse::<usize>()
            .ok()
            .filter(|frames| *frames > 0)
            .ok_or_else(|| UiError::InvalidFrameCount(value.to_string()))?,
        None => 24,
    };
    Ok((id, frames))
}

fn prompt_line(label: &str) -> Result<String, UiError> {
    print_prompt(label)?;
    read_line()
}

fn print_prompt(label: &str) -> Result<(), UiError> {
    print!("{label}> ");
    io::stdout().flush().map_err(UiError::Io)
}

fn read_line() -> Result<String, UiError> {
    let mut input = String::new();
    io::stdin().read_line(&mut input).map_err(UiError::Io)?;
    Ok(input.trim_end_matches(['\r', '\n']).to_string())
}

fn read_body() -> Result<String, UiError> {
    let mut lines = Vec::new();
    loop {
        print_prompt("body")?;
        let line = read_line()?;
        if line == "." {
            break;
        }
        lines.push(line);
    }
    Ok(lines.join("\n"))
}

fn summarize(text: &str, max_chars: usize) -> String {
    let mut normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= max_chars {
        return normalized;
    }

    normalized = normalized.chars().take(max_chars.saturating_sub(3)).collect();
    normalized.push_str("...");
    normalized
}

fn print_help() {
    println!("commands:");
    println!("  new | add       create a note");
    println!("  list | ls       list notes");
    println!("  show <id>       show a note");
    println!("  edit <id>       edit a note");
    println!("  delete <id>     delete a note");
    println!("  save | flush    write dirty notes to the trinary artifact");
    println!("  observe [id]    inspect RAM, disk, compaction, U320/U640/base3 state");
    println!("  watch <id> [n]  animate the compactor lock window for n frames");
    println!("  tui             open split editor/observability frontend");
    println!("  path            print the artifact path");
    println!("  quit | exit     save if dirty, then exit");
    println!("  quit! | exit!   exit without saving");
}

impl fmt::Display for UiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UiError::Io(err) => write!(f, "{err}"),
            UiError::Store(err) => write!(f, "{err}"),
            UiError::Observe(err) => write!(f, "{err}"),
            UiError::Tui(err) => write!(f, "{err}"),
            UiError::InvalidNoteId(value) => write!(f, "invalid note id '{value}'"),
            UiError::InvalidFrameCount(value) => write!(f, "invalid frame count '{value}'"),
        }
    }
}

impl Error for UiError {}

impl From<NoteStoreError> for UiError {
    fn from(value: NoteStoreError) -> Self {
        UiError::Store(value)
    }
}
