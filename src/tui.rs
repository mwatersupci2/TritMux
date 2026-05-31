use crate::notes::{NoteId, NoteStore, NoteStoreError};
use crate::observe::{self, ObserveError};
use crate::trit::core::Trit;
use crate::trit::tryte::{STryte, TRITS_PER_TRYTE};
use std::env;
use std::error::Error;
use std::fmt;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Instant;

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
    active_tab: AppTab,
    selected: Option<NoteId>,
    source_file: Option<PathBuf>,
    compaction_step: usize,
    status: String,
    lines: Vec<String>,
    cursor_row: usize,
    cursor_col: usize,
    command_mode: bool,
    command_input: String,
    /// Optional (row, col_start, col_end) for word selection (Option 6 double-tap)
    selection: Option<(usize, usize, usize)>,
    /// First visible line of the editor body (viewport scroll offset)
    scroll_top: usize,
    /// First visible line of the observability pane
    obs_scroll_top: usize,
    calculator: CalculatorState,
}

#[derive(Debug, Clone)]
struct CalculatorState {
    entry: String,
    accumulator: Option<f64>,
    pending_op: Option<CalcOp>,
    last_expression: String,
    last_result: f64,
    mode: CalcMode,
    fresh_entry: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CalcMode {
    SplitTryte,
    STryte,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CalcOp {
    Add,
    Subtract,
    Multiply,
    Divide,
}

impl CalculatorState {
    fn new() -> Self {
        Self {
            entry: "0".to_string(),
            accumulator: None,
            pending_op: None,
            last_expression: String::new(),
            last_result: 0.0,
            mode: CalcMode::SplitTryte,
            fresh_entry: true,
        }
    }

    fn display_value(&self) -> f64 {
        parse_calc_entry(&self.entry).unwrap_or(self.last_result)
    }

    fn mode_label(&self) -> &'static str {
        match self.mode {
            CalcMode::SplitTryte => "SplitTryte",
            CalcMode::STryte => "sTryte",
        }
    }
}

impl CalcOp {
    fn symbol(self) -> &'static str {
        match self {
            CalcOp::Add => "+",
            CalcOp::Subtract => "-",
            CalcOp::Multiply => "*",
            CalcOp::Divide => "/",
        }
    }

    fn apply(self, lhs: f64, rhs: f64) -> Option<f64> {
        match self {
            CalcOp::Add => Some(lhs + rhs),
            CalcOp::Subtract => Some(lhs - rhs),
            CalcOp::Multiply => Some(lhs * rhs),
            CalcOp::Divide if rhs != 0.0 => Some(lhs / rhs),
            CalcOp::Divide => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppTab {
    MainMenu,
    Reader,
    Editor,
    Calculator,
    Observability,
}

impl AppTab {
    const ALL: [AppTab; 5] = [
        AppTab::MainMenu,
        AppTab::Reader,
        AppTab::Editor,
        AppTab::Calculator,
        AppTab::Observability,
    ];

    fn number(self) -> usize {
        match self {
            AppTab::MainMenu => 1,
            AppTab::Reader => 2,
            AppTab::Editor => 3,
            AppTab::Calculator => 4,
            AppTab::Observability => 5,
        }
    }

    fn title(self) -> &'static str {
        match self {
            AppTab::MainMenu => "TUI Main Menu",
            AppTab::Reader => "Notepad Reader",
            AppTab::Editor => "Notepad Editor",
            AppTab::Calculator => "Calculator",
            AppTab::Observability => "Trinary Memory Observability",
        }
    }

    fn tab_label(self) -> &'static str {
        match self {
            AppTab::MainMenu => "Main",
            AppTab::Reader => "Reader",
            AppTab::Editor => "Editor",
            AppTab::Calculator => "Calc",
            AppTab::Observability => "Observability",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        match name {
            "1" | "main" | "menu" => Some(AppTab::MainMenu),
            "2" | "reader" | "read" => Some(AppTab::Reader),
            "3" | "editor" | "edit" | "notepad" => Some(AppTab::Editor),
            "4" | "calculator" | "calc" => Some(AppTab::Calculator),
            "5" | "observability" | "observe" | "obs" | "memory" => Some(AppTab::Observability),
            _ => None,
        }
    }

    fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }
}

#[derive(Debug, Clone, Copy)]
struct TerminalSize {
    columns: usize,
    rows: usize,
}

#[cfg(windows)]
mod win32 {
    use std::io;
    use std::ffi::c_void;

    pub type HANDLE = *mut c_void;
    pub type DWORD = u32;
    pub type BOOL = i32;

    pub const STD_INPUT_HANDLE: DWORD = -10i32 as DWORD;
    pub const STD_OUTPUT_HANDLE: DWORD = -11i32 as DWORD;

    pub const ENABLE_LINE_INPUT: DWORD = 0x0002;
    pub const ENABLE_ECHO_INPUT: DWORD = 0x0004;
    pub const ENABLE_PROCESSED_INPUT: DWORD = 0x0001;
    pub const ENABLE_VIRTUAL_TERMINAL_INPUT: DWORD = 0x0200;
    
    pub const ENABLE_PROCESSED_OUTPUT: DWORD = 0x0001;
    pub const ENABLE_VIRTUAL_TERMINAL_PROCESSING: DWORD = 0x0004;

    #[link(name = "kernel32")]
    extern "system" {
        pub fn GetStdHandle(nStdHandle: DWORD) -> HANDLE;
        pub fn GetConsoleMode(hConsoleHandle: HANDLE, lpMode: *mut DWORD) -> BOOL;
        pub fn SetConsoleMode(hConsoleHandle: HANDLE, dwMode: DWORD) -> BOOL;
    }

    pub struct RawModeGuard {
        h_in: HANDLE,
        h_out: HANDLE,
        orig_in_mode: DWORD,
        orig_out_mode: DWORD,
    }

    impl RawModeGuard {
        pub fn new() -> Result<Self, io::Error> {
            unsafe {
                let h_in = GetStdHandle(STD_INPUT_HANDLE);
                let h_out = GetStdHandle(STD_OUTPUT_HANDLE);
                if h_in == std::ptr::null_mut() || h_out == std::ptr::null_mut() {
                    return Err(io::Error::new(io::ErrorKind::Other, "Failed to get Win32 std handle"));
                }
                
                let mut orig_in_mode = 0;
                let mut orig_out_mode = 0;
                if GetConsoleMode(h_in, &mut orig_in_mode) == 0 {
                    return Err(io::Error::last_os_error());
                }
                if GetConsoleMode(h_out, &mut orig_out_mode) == 0 {
                    return Err(io::Error::last_os_error());
                }
                
                // Set input to raw
                let raw_in_mode = (orig_in_mode 
                    & !(ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT | ENABLE_PROCESSED_INPUT))
                    | ENABLE_VIRTUAL_TERMINAL_INPUT;
                if SetConsoleMode(h_in, raw_in_mode) == 0 {
                    return Err(io::Error::last_os_error());
                }
                
                // Enable virtual terminal processing for colors and buffer clearing
                let raw_out_mode = orig_out_mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING | ENABLE_PROCESSED_OUTPUT;
                let _ = SetConsoleMode(h_out, raw_out_mode);

                Ok(Self { h_in, h_out, orig_in_mode, orig_out_mode })
            }
        }

        pub fn suspend(&self) -> Result<(), io::Error> {
            unsafe {
                if SetConsoleMode(self.h_in, self.orig_in_mode) == 0 {
                    return Err(io::Error::last_os_error());
                }
                if SetConsoleMode(self.h_out, self.orig_out_mode) == 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            }
        }

        pub fn resume(&self) -> Result<(), io::Error> {
            unsafe {
                let raw_in_mode = (self.orig_in_mode 
                    & !(ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT | ENABLE_PROCESSED_INPUT))
                    | ENABLE_VIRTUAL_TERMINAL_INPUT;
                if SetConsoleMode(self.h_in, raw_in_mode) == 0 {
                    return Err(io::Error::last_os_error());
                }
                let raw_out_mode = self.orig_out_mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING | ENABLE_PROCESSED_OUTPUT;
                let _ = SetConsoleMode(self.h_out, raw_out_mode);
                Ok(())
            }
        }
    }

    impl Drop for RawModeGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = SetConsoleMode(self.h_in, self.orig_in_mode);
                let _ = SetConsoleMode(self.h_out, self.orig_out_mode);
            }
        }
    }
}

#[cfg(not(windows))]
mod fallback {
    use std::io;
    use std::os::raw::{c_int, c_ulong};

    use super::TerminalSize;

    const STDIN_FILENO: c_int = 0;
    const TCSANOW: c_int = 0;
    const TIOCGWINSZ: c_ulong = 0x5413;
    const NCCS: usize = 32;

    const BRKINT: u32 = 0o000002;
    const ICRNL: u32 = 0o000400;
    const INPCK: u32 = 0o000020;
    const ISTRIP: u32 = 0o000040;
    const IXON: u32 = 0o002000;

    const OPOST: u32 = 0o000001;

    const CSIZE: u32 = 0o000060;
    const CS8: u32 = 0o000060;

    const ECHO: u32 = 0o000010;
    const ICANON: u32 = 0o000002;
    const IEXTEN: u32 = 0o100000;
    const ISIG: u32 = 0o000001;

    const VTIME: usize = 5;
    const VMIN: usize = 6;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Termios {
        c_iflag: u32,
        c_oflag: u32,
        c_cflag: u32,
        c_lflag: u32,
        c_line: u8,
        c_cc: [u8; NCCS],
        c_ispeed: u32,
        c_ospeed: u32,
    }

    #[repr(C)]
    struct Winsize {
        ws_row: u16,
        ws_col: u16,
        ws_xpixel: u16,
        ws_ypixel: u16,
    }

    extern "C" {
        fn tcgetattr(fd: c_int, termios: *mut Termios) -> c_int;
        fn tcsetattr(fd: c_int, optional_actions: c_int, termios: *const Termios) -> c_int;
        fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
    }

    pub struct RawModeGuard {
        original_mode: Termios,
    }

    impl RawModeGuard {
        pub fn new() -> Result<Self, io::Error> {
            let original_mode = read_terminal_mode()?;
            set_raw_mode(original_mode)?;
            Ok(Self { original_mode })
        }

        pub fn suspend(&self) -> Result<(), io::Error> {
            restore_terminal_mode(self.original_mode)
        }

        pub fn resume(&self) -> Result<(), io::Error> {
            set_raw_mode(self.original_mode)
        }
    }

    impl Drop for RawModeGuard {
        fn drop(&mut self) {
            let _ = restore_terminal_mode(self.original_mode);
        }
    }

    fn read_terminal_mode() -> Result<Termios, io::Error> {
        let mut mode = Termios {
            c_iflag: 0,
            c_oflag: 0,
            c_cflag: 0,
            c_lflag: 0,
            c_line: 0,
            c_cc: [0; NCCS],
            c_ispeed: 0,
            c_ospeed: 0,
        };

        let rc = unsafe { tcgetattr(STDIN_FILENO, &mut mode) };
        if rc == 0 {
            Ok(mode)
        } else {
            Err(io::Error::last_os_error())
        }
    }

    fn set_raw_mode(original_mode: Termios) -> Result<(), io::Error> {
        let mut raw = original_mode;
        raw.c_iflag &= !(BRKINT | ICRNL | INPCK | ISTRIP | IXON);
        raw.c_oflag &= !OPOST;
        raw.c_cflag = (raw.c_cflag & !CSIZE) | CS8;
        raw.c_lflag &= !(ECHO | ICANON | IEXTEN | ISIG);
        raw.c_cc[VMIN] = 1;
        raw.c_cc[VTIME] = 0;
        apply_terminal_mode(raw)
    }

    fn restore_terminal_mode(mode: Termios) -> Result<(), io::Error> {
        apply_terminal_mode(mode)
    }

    fn apply_terminal_mode(mode: Termios) -> Result<(), io::Error> {
        let rc = unsafe { tcsetattr(STDIN_FILENO, TCSANOW, &mode) };
        if rc == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    pub fn terminal_size() -> Option<TerminalSize> {
        let mut size = Winsize {
            ws_row: 0,
            ws_col: 0,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let rc = unsafe { ioctl(STDIN_FILENO, TIOCGWINSZ, &mut size) };
        if rc == 0 && size.ws_col > 0 && size.ws_row > 0 {
            Some(TerminalSize {
                columns: size.ws_col as usize,
                rows: size.ws_row as usize,
            })
        } else {
            None
        }
    }
}

/// A parsed mouse event from SGR mouse reporting (ESC[<btn;col;rowM/m)
#[derive(Debug, Clone)]
struct MouseEvent {
    /// 0=left, 1=middle, 2=right, 64=wheel-up, 65=wheel-down
    button: u8,
    col: usize,   // 1-based
    row: usize,   // 1-based
    pressed: bool,
}

#[derive(Debug)]
enum Key {
    Char(char),
    Up,
    Down,
    Left,
    Right,
    Backspace,
    Delete,
    Enter,
    Tab,
    Esc,
    PageUp,
    PageDown,
    Home,
    End,
    Ctrl(char),
    Mouse(MouseEvent),
    Unknown,
}

/// Event dispatched to the main render loop from background threads.
enum Event {
    Input(Key),
    Tick,
}

/// Parse one stdin read into all key events it contains.
fn parse_keys(buf: &[u8]) -> Vec<Key> {
    let mut keys = Vec::new();
    let mut i = 0;

    while i < buf.len() {
        let (key, consumed) = parse_key_at(&buf[i..]);
        keys.push(key);
        i += consumed.max(1);
    }

    keys
}

fn parse_key_at(buf: &[u8]) -> (Key, usize) {
    if buf.is_empty() {
        return (Key::Unknown, 0);
    }
    match buf[0] {
        27 => {
            if buf.len() >= 3 && buf[1] == b'[' {
                // SGR mouse: ESC [ < ... M/m
                if buf[2] == b'<' {
                    if let Some(end) = buf[3..].iter().position(|b| *b == b'M' || *b == b'm') {
                        let sequence_end = 3 + end + 1;
                        let tail = &buf[3..sequence_end];
                        if let Some(sgr) = parse_sgr_mouse(tail) {
                            return (Key::Mouse(sgr), sequence_end);
                        }
                        return (Key::Unknown, sequence_end);
                    }
                    return (Key::Unknown, buf.len());
                }
                match buf[2] {
                    b'A' => (Key::Up, 3),
                    b'B' => (Key::Down, 3),
                    b'C' => (Key::Right, 3),
                    b'D' => (Key::Left, 3),
                    b'H' => (Key::Home, 3),
                    b'F' => (Key::End, 3),
                    b'1' if buf.len() >= 4 && buf[3] == b'~' => (Key::Home, 4),
                    b'3' if buf.len() >= 4 && buf[3] == b'~' => (Key::Delete, 4),
                    b'4' if buf.len() >= 4 && buf[3] == b'~' => (Key::End, 4),
                    b'5' if buf.len() >= 4 && buf[3] == b'~' => (Key::PageUp, 4),
                    b'6' if buf.len() >= 4 && buf[3] == b'~' => (Key::PageDown, 4),
                    _ => (Key::Unknown, 3),
                }
            } else if buf.len() == 1 {
                (Key::Esc, 1)
            } else {
                (Key::Unknown, 1)
            }
        }
        8 | 127 => (Key::Backspace, 1),
        b'\r' | b'\n' => (Key::Enter, 1),
        b'\t' => (Key::Tab, 1),
        1..=26 => {
            let ch = (b'a' + buf[0] - 1) as char;
            (Key::Ctrl(ch), 1)
        }
        _ => {
            if let Ok(s) = std::str::from_utf8(buf) {
                if let Some(ch) = s.chars().next() {
                    return (Key::Char(ch), ch.len_utf8());
                }
            }
            (Key::Unknown, 1)
        }
    }
}

/// Parse SGR mouse sequence tail after ESC[<  (bytes up to and including M or m).
fn parse_sgr_mouse(tail: &[u8]) -> Option<MouseEvent> {
    // Expect "btn;col;row" followed by M (press) or m (release)
    let s = std::str::from_utf8(tail).ok()?;
    let (nums, pressed) = if let Some(rest) = s.strip_suffix('M') {
        (rest, true)
    } else if let Some(rest) = s.strip_suffix('m') {
        (rest, false)
    } else {
        return None;
    };
    let mut parts = nums.splitn(3, ';');
    let button: u8 = parts.next()?.parse().ok()?;
    let col: usize = parts.next()?.parse().ok()?;
    let row: usize = parts.next()?.parse().ok()?;
    Some(MouseEvent { button, col, row, pressed })
}

// ─────────────────────────────────────────────────────────────────────────────
// Option 6 — Mobile & Touch Screen Pipeline (TKB-27 Bank:0)
// Termux on Android maps screen taps → SGR mouse clicks and swipes → scroll
// wheel events.  GestureTracker sits on top of the raw MouseEvent stream and
// recognises higher-level gestures that match TKB-27 Bank:0 entries.
// ─────────────────────────────────────────────────────────────────────────────

/// TKB-27 Bank:0 TOUCH_GESTURE symbols (subset implemented in software).
#[derive(Debug, Clone, PartialEq)]
enum Gesture {
    /// TOUCH_TAP (01)   — single-finger quick tap (plain left-click)
    Tap { col: usize, row: usize },
    /// TOUCH_DOUBLE_TAP (02) — two taps within DOUBLE_TAP_MS on same cell → select word
    DoubleTap { col: usize, row: usize },
    /// TOUCH_SWIPE_UP (04)  — rapid upward scroll burst
    SwipeUp,
    /// TOUCH_SWIPE_DOWN (05) — rapid downward scroll burst
    SwipeDown,
    /// TOUCH_SWIPE_LEFT (06) — rapid scroll burst while near right edge → prev tab
    SwipeLeft,
    /// TOUCH_SWIPE_RIGHT (07) — rapid scroll burst while near left edge → next tab
    SwipeRight,
    /// TOUCH_EDGE_SWIPE_L (21) — tap in leftmost 2 columns → page up
    EdgeSwipeLeft,
    /// TOUCH_EDGE_SWIPE_R (22) — tap in rightmost 2 columns → page down
    EdgeSwipeRight,
}

/// Timing constants (milliseconds).
const DOUBLE_TAP_MS: u128 = 400;
const SWIPE_BURST_MS: u128 = 150;
const SWIPE_MIN_EVENTS: usize = 3;

/// Tracks inter-event timing to classify raw mouse events into Gestures.
struct GestureTracker {
    last_click_time: Option<Instant>,
    last_click_pos: Option<(usize, usize)>, // (col, row)
    scroll_up_count: usize,
    scroll_down_count: usize,
    scroll_burst_start: Option<Instant>,
}

impl GestureTracker {
    fn new() -> Self {
        GestureTracker {
            last_click_time: None,
            last_click_pos: None,
            scroll_up_count: 0,
            scroll_down_count: 0,
            scroll_burst_start: None,
        }
    }

    /// Feed a raw MouseEvent; returns an optional high-level Gesture.
    fn feed(&mut self, m: &MouseEvent, terminal_cols: usize) -> Option<Gesture> {
        match m.button {
            // Left press
            0 if m.pressed => {
                let now = Instant::now();
                let pos = (m.col, m.row);

                // Double-tap detection
                if let (Some(t), Some(last)) = (self.last_click_time, self.last_click_pos) {
                    if now.duration_since(t).as_millis() <= DOUBLE_TAP_MS
                        && last == pos
                    {
                        self.last_click_time = None;
                        self.last_click_pos = None;
                        return Some(Gesture::DoubleTap { col: m.col, row: m.row });
                    }
                }

                // Edge-swipe (tap very near screen edges → page navigation)
                if m.col <= 2 {
                    self.last_click_time = Some(now);
                    self.last_click_pos = Some(pos);
                    return Some(Gesture::EdgeSwipeLeft);
                }
                if terminal_cols > 2 && m.col >= terminal_cols.saturating_sub(2) {
                    self.last_click_time = Some(now);
                    self.last_click_pos = Some(pos);
                    return Some(Gesture::EdgeSwipeRight);
                }

                self.last_click_time = Some(now);
                self.last_click_pos = Some(pos);
                Some(Gesture::Tap { col: m.col, row: m.row })
            }

            // Scroll wheel up (btn 64) — accumulate for swipe burst
            64 if m.pressed => {
                let now = Instant::now();
                let elapsed = self.scroll_burst_start
                    .map(|t| now.duration_since(t).as_millis())
                    .unwrap_or(u128::MAX);

                if elapsed > SWIPE_BURST_MS {
                    // Reset burst window
                    self.scroll_up_count = 0;
                    self.scroll_down_count = 0;
                    self.scroll_burst_start = Some(now);
                }

                self.scroll_up_count += 1;

                if self.scroll_up_count >= SWIPE_MIN_EVENTS {
                    self.scroll_up_count = 0;
                    self.scroll_burst_start = None;
                    // Near left edge → SwipeLeft (prev tab), else SwipeUp
                    return Some(if m.col <= 4 { Gesture::SwipeLeft } else { Gesture::SwipeUp });
                }
                None
            }

            // Scroll wheel down (btn 65) — accumulate for swipe burst
            65 if m.pressed => {
                let now = Instant::now();
                let elapsed = self.scroll_burst_start
                    .map(|t| now.duration_since(t).as_millis())
                    .unwrap_or(u128::MAX);

                if elapsed > SWIPE_BURST_MS {
                    self.scroll_up_count = 0;
                    self.scroll_down_count = 0;
                    self.scroll_burst_start = Some(now);
                }

                self.scroll_down_count += 1;

                if self.scroll_down_count >= SWIPE_MIN_EVENTS {
                    self.scroll_down_count = 0;
                    self.scroll_burst_start = None;
                    return Some(if m.col <= 4 { Gesture::SwipeRight } else { Gesture::SwipeDown });
                }
                None
            }

            _ => None,
        }
    }
}

/// Select the word boundaries at `col` in `line` (returns start..end, both inclusive char indices).
fn select_word_at(line: &str, col: usize) -> (usize, usize) {
    let chars: Vec<char> = line.chars().collect();
    let col = col.min(chars.len().saturating_sub(1));
    if chars.is_empty() {
        return (0, 0);
    }
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    // Expand left
    let mut start = col;
    while start > 0 && is_word(chars[start - 1]) {
        start -= 1;
    }
    // Expand right
    let mut end = col;
    while end + 1 < chars.len() && is_word(chars[end + 1]) {
        end += 1;
    }
    (start, end)
}

/// Dispatch a TKB-27 Bank:0 gesture into TUI state mutations.
/// Returns true if the TUI should redraw immediately.
fn dispatch_gesture(
    store: &NoteStore,
    state: &mut TuiState,
    gesture: &Gesture,
    size: TerminalSize,
) {
    match gesture {
        // TOUCH_TAP — standard click handled by handle_mouse; nothing extra needed
        Gesture::Tap { .. } => {}

        // TOUCH_DOUBLE_TAP — select word under touch point
        Gesture::DoubleTap { col, row } => {
            let left_width = (size.columns.max(MIN_COLUMNS) * 47 / 100).max(38);
            let content_rows = size.rows.max(MIN_ROWS).saturating_sub(COMMAND_ROWS).max(12);
            // Same coordinate mapping as handle_mouse body-click
            let editor_body_start = 3usize;
            let line_num_cols = 6usize;
            let header_lines = 6usize;

            if *row >= editor_body_start
                && *row < editor_body_start + content_rows
                && *col >= 1
                && *col <= left_width
            {
                let body_row = row - editor_body_start;
                if body_row >= header_lines {
                    let text_row = body_row - header_lines;
                    if text_row < state.lines.len() {
                        let inner_col = col.saturating_sub(1 + line_num_cols);
                        let (ws, we) = select_word_at(&state.lines[text_row], inner_col);
                        state.cursor_row = text_row;
                        state.cursor_col = we;
                        state.selection = Some((text_row, ws, we));
                        let word: String = state.lines[text_row]
                            .chars()
                            .skip(ws)
                            .take(we - ws + 1)
                            .collect();
                        state.status = format!("selected: \"{word}\" (col {ws}–{we})");
                    }
                }
            }
        }

        // TOUCH_SWIPE_UP — scroll cursor up several lines (like page-scroll)
        Gesture::SwipeUp => {
            let jump = (size.rows / 4).max(3);
            state.cursor_row = state.cursor_row.saturating_sub(jump);
            state.cursor_col = state.cursor_col.min(char_count(&state.lines[state.cursor_row]));
            state.selection = None;
            state.status = format!("swipe up → row {}", state.cursor_row + 1);
        }

        // TOUCH_SWIPE_DOWN — scroll cursor down several lines
        Gesture::SwipeDown => {
            let jump = (size.rows / 4).max(3);
            let max_row = state.lines.len().saturating_sub(1);
            state.cursor_row = (state.cursor_row + jump).min(max_row);
            state.cursor_col = state.cursor_col.min(char_count(&state.lines[state.cursor_row]));
            state.selection = None;
            state.status = format!("swipe down → row {}", state.cursor_row + 1);
        }

        // TOUCH_SWIPE_LEFT (near left edge) — cycle to previous tab
        Gesture::SwipeLeft => {
            let notes = store.notes();
            if !notes.is_empty() {
                let current_idx = state
                    .selected
                    .and_then(|id| notes.iter().position(|n| n.id() == id))
                    .unwrap_or(0);
                let prev_idx = if current_idx == 0 { notes.len() - 1 } else { current_idx - 1 };
                state.selected = Some(notes[prev_idx].id());
                state.cursor_row = 0;
                state.cursor_col = 0;
                state.selection = None;
                state.status = format!(
                    "swipe left → note #{} ({})",
                    notes[prev_idx].id(),
                    notes[prev_idx].title_text().unwrap_or_default()
                );
            }
        }

        // TOUCH_SWIPE_RIGHT (near left edge) — cycle to next tab
        Gesture::SwipeRight => {
            let notes = store.notes();
            if !notes.is_empty() {
                let current_idx = state
                    .selected
                    .and_then(|id| notes.iter().position(|n| n.id() == id))
                    .unwrap_or(0);
                let next_idx = (current_idx + 1) % notes.len();
                state.selected = Some(notes[next_idx].id());
                state.cursor_row = 0;
                state.cursor_col = 0;
                state.selection = None;
                state.status = format!(
                    "swipe right → note #{} ({})",
                    notes[next_idx].id(),
                    notes[next_idx].title_text().unwrap_or_default()
                );
            }
        }

        // TOUCH_EDGE_SWIPE_L — jump cursor up a full page
        Gesture::EdgeSwipeLeft => {
            let page = size.rows.saturating_sub(COMMAND_ROWS + 4).max(4);
            state.cursor_row = state.cursor_row.saturating_sub(page);
            state.cursor_col = state.cursor_col.min(char_count(&state.lines[state.cursor_row]));
            state.selection = None;
            state.status = format!("edge-left → page up (row {})", state.cursor_row + 1);
        }

        // TOUCH_EDGE_SWIPE_R — jump cursor down a full page
        Gesture::EdgeSwipeRight => {
            let page = size.rows.saturating_sub(COMMAND_ROWS + 4).max(4);
            let max_row = state.lines.len().saturating_sub(1);
            state.cursor_row = (state.cursor_row + page).min(max_row);
            state.cursor_col = state.cursor_col.min(char_count(&state.lines[state.cursor_row]));
            state.selection = None;
            state.status = format!("edge-right → page down (row {})", state.cursor_row + 1);
        }
    }
}

fn char_count(s: &str) -> usize {
    s.chars().count()
}

/// Number of note body lines visible in the editor pane for the given terminal size.
/// Accounts for: 2 frame borders + 6 header lines (tabs, artifact, text file, payload, blank, body:).
fn compute_editor_visible_rows(size: TerminalSize) -> usize {
    let rows = size.rows.max(MIN_ROWS);
    let content_rows = rows.saturating_sub(COMMAND_ROWS).max(12);
    let max_body_rows = content_rows.saturating_sub(2); // subtract top+bottom border
    max_body_rows.saturating_sub(6).max(1)              // subtract 6 header lines
}

/// Adjust `state.scroll_top` so the cursor is always inside the visible viewport.
fn clamp_scroll(state: &mut TuiState, visible_rows: usize) {
    if state.cursor_row < state.scroll_top {
        state.scroll_top = state.cursor_row;
    }
    if visible_rows > 0 && state.cursor_row >= state.scroll_top + visible_rows {
        state.scroll_top = state.cursor_row.saturating_sub(visible_rows - 1);
    }
    // Also ensure scroll_top doesn't exceed the last line
    if !state.lines.is_empty() {
        state.scroll_top = state.scroll_top.min(state.lines.len() - 1);
    }
}

fn insert_char(s: &mut String, idx: usize, c: char) {
    let mut chars: Vec<char> = s.chars().collect();
    if idx <= chars.len() {
        chars.insert(idx, c);
    }
    *s = chars.into_iter().collect();
}

fn remove_char(s: &mut String, idx: usize) {
    let mut chars: Vec<char> = s.chars().collect();
    if idx < chars.len() {
        chars.remove(idx);
    }
    *s = chars.into_iter().collect();
}

fn sync_to_store(store: &mut NoteStore, state: &TuiState) -> Result<(), TuiError> {
    if let Some(id) = state.selected {
        if let Some(note) = store.note(id) {
            let title = note.title_text().map_err(NoteStoreError::Payload).map_err(TuiError::Store)?;
            let body = state.lines.join("\n");
            store.replace(id, &title, &body).map_err(TuiError::Store)?;
        }
    }
    Ok(())
}

fn load_selected_note(store: &NoteStore, state: &mut TuiState) {
    if let Some(id) = state.selected {
        if let Some(note) = store.note(id) {
            if let Ok(body) = note.body_text() {
                state.lines = if body.is_empty() {
                    vec![String::new()]
                } else {
                    body.lines().map(String::from).collect()
                };
                state.cursor_row = 0;
                state.cursor_col = 0;
                return;
            }
        }
    }
    state.lines = vec![String::new()];
    state.cursor_row = 0;
    state.cursor_col = 0;
}

struct AlternateScreenGuard;
impl Drop for AlternateScreenGuard {
    fn drop(&mut self) {
        print!("\x1b[?1049l");
        let _ = io::stdout().flush();
    }
}

/// Enables SGR mouse reporting (click + scroll) on construction, disables on drop.
struct MouseReportGuard;
impl MouseReportGuard {
    fn new() -> Self {
        // Enable: normal mouse button events + SGR extended coordinates
        print!("\x1b[?1000h\x1b[?1006h");
        let _ = io::stdout().flush();
        MouseReportGuard
    }
}
impl Drop for MouseReportGuard {
    fn drop(&mut self) {
        print!("\x1b[?1000l\x1b[?1006l");
        let _ = io::stdout().flush();
    }
}

fn colorize_left_line(line: &str) -> String {
    if line.starts_with('+') {
        return format!("\x1b[38;2;96;96;96m{}\x1b[0m", line);
    }
    
    if line.starts_with('|') && line.ends_with('|') {
        let inner = &line[1..line.len()-1];
        let mut colorized_inner = String::new();
        
        if inner.contains("Editor") {
            colorized_inner.push_str(&format!("\x1b[1;38;2;0;255;255m{}\x1b[0m", inner.trim()));
        } else if inner.starts_with("TABS | ") {
            let tab_content = &inner[7..];
            let mut colorized_tab = String::new();
            let mut in_selected_tab = false;
            for ch in tab_content.chars() {
                match ch {
                    '[' => {
                        in_selected_tab = true;
                        colorized_tab.push_str("\x1b[1;38;2;0;255;255m[\x1b[0m\x1b[1;38;2;255;192;0m");
                    }
                    ']' => {
                        in_selected_tab = false;
                        colorized_tab.push_str("\x1b[0m\x1b[1;38;2;0;255;255m]\x1b[0m");
                    }
                    _ => {
                        if !in_selected_tab {
                            colorized_tab.push_str("\x1b[38;2;128;128;128m");
                            colorized_tab.push(ch);
                            colorized_tab.push_str("\x1b[0m");
                        } else {
                            colorized_tab.push(ch);
                        }
                    }
                }
            }
            colorized_inner.push_str(&colorized_tab);
        } else if inner.contains("artifact:") {
            let mut parts = inner.splitn(2, ':');
            let label = parts.next().unwrap();
            let val = parts.next().unwrap_or("");
            colorized_inner.push_str(&format!("\x1b[38;2;128;128;128m{}:\x1b[0m\x1b[38;2;255;255;255m{}\x1b[0m", label, val));
        } else if inner.contains("selected:") {
            let mut parts = inner.splitn(2, ':');
            let label = parts.next().unwrap();
            let val = parts.next().unwrap_or("");
            colorized_inner.push_str(&format!("\x1b[38;2;128;128;128m{}:\x1b[0m\x1b[1;38;2;255;192;0m{}\x1b[0m", label, val));
        } else if inner.contains(" | ") {
            let mut parts = inner.splitn(2, " | ");
            let line_num = parts.next().unwrap();
            let content = parts.next().unwrap_or("");
            
            let highlighted_content = if content.trim_start().starts_with('#') {
                format!("\x1b[1;38;2;0;192;255m{}\x1b[0m", content)
            } else if content.trim_start().starts_with('*') || content.trim_start().starts_with('-') {
                format!("\x1b[38;2;255;192;0m{}\x1b[0m", content)
            } else {
                format!("\x1b[38;2;255;255;255m{}\x1b[0m", content)
            };
            
            colorized_inner.push_str(&format!(
                "\x1b[38;2;128;128;128m{} |\x1b[0m {}",
                line_num, highlighted_content
            ));
        } else {
            colorized_inner.push_str("\x1b[38;2;192;192;192m");
            colorized_inner.push_str(inner);
            colorized_inner.push_str("\x1b[0m");
        }
        
        return format!("\x1b[38;2;96;96;96m|\x1b[0m{}\x1b[38;2;96;96;96m|\x1b[0m", colorized_inner);
    }
    
    line.to_string()
}

pub fn run(store: &mut NoteStore, artifact_path: &Path) -> Result<(), TuiError> {
    #[cfg(windows)]
    let guard = win32::RawModeGuard::new().map_err(TuiError::Io)?;
    #[cfg(not(windows))]
    let guard = fallback::RawModeGuard::new().map_err(TuiError::Io)?;

    print!("\x1b[?1049h\x1b[2J\x1b[H");
    io::stdout().flush().map_err(TuiError::Io)?;
    let _alt_guard = AlternateScreenGuard;
    // Option 5: enable SGR mouse reporting (buttons + scroll wheel)
    let _mouse_guard = MouseReportGuard::new();

    let mut state = TuiState {
        active_tab: AppTab::MainMenu,
        selected: first_note_id(store).or_else(|| Some(store.add("untitled", ""))),
        source_file: None,
        compaction_step: 0,
        status: "TUI ready. Press : for commands.".to_string(),
        lines: Vec::new(),
        cursor_row: 0,
        cursor_col: 0,
        command_mode: false,
        command_input: String::new(),
        selection: None,
        scroll_top: 0,
        obs_scroll_top: 0,
        calculator: CalculatorState::new(),
    };
    load_selected_note(store, &mut state);

    // ── Event channel: input thread + timer thread → main loop ──────────────
    let (tx, rx) = mpsc::channel::<Event>();

    // Option 4 – timer thread: sends Tick every 200 ms for compactor animation
    {
        let tx_tick = tx.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_millis(200));
                if tx_tick.send(Event::Tick).is_err() {
                    break;
                }
            }
        });
    }

    // Input thread: blocks on stdin, sends Key events
    {
        let tx_key = tx.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 64];
            loop {
                match io::stdin().read(&mut buf) {
                    Ok(n) if n > 0 => {
                        for key in parse_keys(&buf[..n]) {
                            if tx_key.send(Event::Input(key)).is_err() {
                                return;
                            }
                        }
                    }
                    _ => break,
                }
            }
        });
    }
    // drop our own clone so only threads hold senders
    drop(tx);

    // Option 6: gesture tracker (sits above raw mouse events)
    let mut gestures = GestureTracker::new();
    let mut needs_render = true;

    loop {
        if needs_render {
            let size = terminal_size();
            print!("{}", render_app(store, artifact_path, &state, size)?);
            print!("{}", position_cursor(&state, size));
            io::stdout().flush().map_err(TuiError::Io)?;
            needs_render = false;
        }

        // Wait for next event (blocks until input or tick)
        let event = match rx.recv() {
            Ok(e) => e,
            Err(_) => break, // both senders dropped → quit
        };

        match event {
            // ── Option 4: timer tick → advance compactor animation frame ──
            Event::Tick => {
                // Drain any extra ticks that piled up while we were rendering
                while let Ok(Event::Tick) = rx.try_recv() {}
                if state.active_tab == AppTab::Observability {
                    state.compaction_step = state.compaction_step.wrapping_add(1);
                    needs_render = true;
                }
                continue;
            }

            Event::Input(key) => {
                needs_render = true;
                if state.command_mode {
                    match key {
                        Key::Char(c) => {
                            state.command_input.push(c);
                        }
                        Key::Backspace => {
                            state.command_input.pop();
                        }
                        Key::Esc => {
                            state.command_mode = false;
                            state.command_input.clear();
                            state.status = "Editing".to_string();
                        }
                        Key::Enter => {
                            let cmd = state.command_input.trim().to_string();
                            state.command_mode = false;
                            state.command_input.clear();
                            if cmd.is_empty() {
                                continue;
                            }
                            let prev_selected = state.selected;

                            // Suspend raw mode temporarily for interactive prompts
                            let _ = guard.suspend();
                            let should_quit = handle_command(store, artifact_path, &mut state, &cmd);
                            let _ = guard.resume();

                            let should_quit = should_quit?;
                            if should_quit {
                                break;
                            }
                            if state.selected != prev_selected {
                                load_selected_note(store, &mut state);
                            }
                        }
                        _ => {}
                    }
                } else {
                    match key {
                        Key::Ctrl('q') => {
                            if store.flush_if_dirty(artifact_path).map_err(TuiError::Store)? {
                                state.status = format!("saved {}", artifact_path.display());
                            }
                            break;
                        }
                        Key::Ctrl('s') => {
                            if let Err(e) = sync_to_store(store, &state) {
                                state.status = format!("Sync error: {e}");
                            } else {
                                match store.save(artifact_path) {
                                    Ok(_) => state.status = format!("saved to {}", artifact_path.display()),
                                    Err(e) => state.status = format!("Save error: {e}"),
                                }
                            }
                        }
                        Key::Char(':') => {
                            state.command_mode = true;
                            state.command_input.clear();
                            state.status = "Command mode. Press ESC to return.".to_string();
                        }
                        Key::Tab => {
                            state.active_tab = state.active_tab.next();
                            state.status = format!("tab {}: {}", state.active_tab.number(), state.active_tab.title());
                        }
                        _ if state.active_tab == AppTab::Calculator => {
                            if handle_calculator_key(&mut state, &key) {
                                state.status = format!("calculator mode: {}", state.calculator.mode_label());
                            }
                        }
                        Key::Up => {
                            if state.cursor_row > 0 {
                                state.cursor_row -= 1;
                                state.cursor_col = state.cursor_col.min(char_count(&state.lines[state.cursor_row]));
                            }
                            let vr = compute_editor_visible_rows(terminal_size());
                            clamp_scroll(&mut state, vr);
                        }
                        Key::Down => {
                            if state.cursor_row + 1 < state.lines.len() {
                                state.cursor_row += 1;
                                state.cursor_col = state.cursor_col.min(char_count(&state.lines[state.cursor_row]));
                            }
                            let vr = compute_editor_visible_rows(terminal_size());
                            clamp_scroll(&mut state, vr);
                        }
                        Key::Left => {
                            if state.cursor_col > 0 {
                                state.cursor_col -= 1;
                            } else if state.cursor_row > 0 {
                                state.cursor_row -= 1;
                                state.cursor_col = char_count(&state.lines[state.cursor_row]);
                            }
                            let vr = compute_editor_visible_rows(terminal_size());
                            clamp_scroll(&mut state, vr);
                        }
                        Key::Right => {
                            let len = char_count(&state.lines[state.cursor_row]);
                            if state.cursor_col < len {
                                state.cursor_col += 1;
                            } else if state.cursor_row + 1 < state.lines.len() {
                                state.cursor_row += 1;
                                state.cursor_col = 0;
                            }
                            let vr = compute_editor_visible_rows(terminal_size());
                            clamp_scroll(&mut state, vr);
                        }
                        // TKB-27 KEY_PGUP / KEY_PGDN — scroll editor viewport by one page
                        Key::PageUp => {
                            let vr = compute_editor_visible_rows(terminal_size());
                            let page = vr.max(1);
                            state.cursor_row = state.cursor_row.saturating_sub(page);
                            state.cursor_col = state.cursor_col.min(char_count(&state.lines[state.cursor_row]));
                            clamp_scroll(&mut state, vr);
                            state.status = format!("PgUp → L{}", state.cursor_row + 1);
                        }
                        Key::PageDown => {
                            let vr = compute_editor_visible_rows(terminal_size());
                            let page = vr.max(1);
                            let max_row = state.lines.len().saturating_sub(1);
                            state.cursor_row = (state.cursor_row + page).min(max_row);
                            state.cursor_col = state.cursor_col.min(char_count(&state.lines[state.cursor_row]));
                            clamp_scroll(&mut state, vr);
                            state.status = format!("PgDn → L{}", state.cursor_row + 1);
                        }
                        // TKB-27 KEY_HOME / KEY_END — jump to line start/end
                        Key::Home => {
                            state.cursor_col = 0;
                        }
                        Key::End => {
                            state.cursor_col = char_count(&state.lines[state.cursor_row]);
                        }
                        // [ / ] — scroll observability pane up/down independently
                        Key::Char('[') => {
                            state.obs_scroll_top = state.obs_scroll_top.saturating_sub(3);
                            state.status = "obs ↑".to_string();
                        }
                        Key::Char(']') => {
                            state.obs_scroll_top = state.obs_scroll_top.saturating_add(3);
                            state.status = "obs ↓".to_string();
                        }
                        Key::Backspace => {
                            if state.active_tab != AppTab::Editor {
                                continue;
                            }
                            if state.cursor_col > 0 {
                                remove_char(&mut state.lines[state.cursor_row], state.cursor_col - 1);
                                state.cursor_col -= 1;
                                let _ = sync_to_store(store, &state);
                                state.compaction_step = state.compaction_step.saturating_add(1);
                            } else if state.cursor_row > 0 {
                                let prev_len = char_count(&state.lines[state.cursor_row - 1]);
                                let current_line = state.lines.remove(state.cursor_row);
                                state.cursor_row -= 1;
                                state.lines[state.cursor_row].push_str(&current_line);
                                state.cursor_col = prev_len;
                                let _ = sync_to_store(store, &state);
                                state.compaction_step = state.compaction_step.saturating_add(1);
                            }
                        }
                        Key::Delete => {
                            if state.active_tab != AppTab::Editor {
                                continue;
                            }
                            let len = char_count(&state.lines[state.cursor_row]);
                            if state.cursor_col < len {
                                remove_char(&mut state.lines[state.cursor_row], state.cursor_col);
                                let _ = sync_to_store(store, &state);
                                state.compaction_step = state.compaction_step.saturating_add(1);
                            } else if state.cursor_row + 1 < state.lines.len() {
                                let next_line = state.lines.remove(state.cursor_row + 1);
                                state.lines[state.cursor_row].push_str(&next_line);
                                let _ = sync_to_store(store, &state);
                                state.compaction_step = state.compaction_step.saturating_add(1);
                            }
                        }
                        Key::Enter => {
                            if state.active_tab != AppTab::Editor {
                                continue;
                            }
                            let current_line = &state.lines[state.cursor_row];
                            let left: String = current_line.chars().take(state.cursor_col).collect();
                            let right: String = current_line.chars().skip(state.cursor_col).collect();
                            state.lines[state.cursor_row] = left;
                            state.lines.insert(state.cursor_row + 1, right);
                            state.cursor_row += 1;
                            state.cursor_col = 0;
                            let vr = compute_editor_visible_rows(terminal_size());
                            clamp_scroll(&mut state, vr);
                            let _ = sync_to_store(store, &state);
                            state.compaction_step = state.compaction_step.saturating_add(1);
                        }
                        Key::Char(c) => {
                            if state.active_tab != AppTab::Editor {
                                continue;
                            }
                            insert_char(&mut state.lines[state.cursor_row], state.cursor_col, c);
                            state.cursor_col += 1;
                            let _ = sync_to_store(store, &state);
                            state.compaction_step = state.compaction_step.saturating_add(1);
                        }
                        // ── Options 5 & 6: mouse events + gesture layer ──
                        Key::Mouse(m) => {
                            let size = terminal_size();
                            // Feed through TKB-27 gesture recogniser first
                            if let Some(gesture) = gestures.feed(&m, size.columns) {
                                dispatch_gesture(store, &mut state, &gesture, size);
                                // For plain Tap, also run the normal click handler
                                if matches!(gesture, Gesture::Tap { .. }) {
                                    handle_mouse(store, &mut state, &m, size);
                                }
                            } else {
                                // Single scroll events not yet a swipe burst → direct handler
                                handle_mouse(store, &mut state, &m, size);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    Ok(())
}

fn handle_calculator_key(state: &mut TuiState, key: &Key) -> bool {
    match key {
        Key::Char(ch) if ch.is_ascii_digit() => {
            push_calc_digit(&mut state.calculator, *ch);
            true
        }
        Key::Char('.') => {
            if state.calculator.fresh_entry {
                state.calculator.entry = "0".to_string();
                state.calculator.fresh_entry = false;
            }
            if !state.calculator.entry.contains('.') {
                state.calculator.entry.push('.');
            }
            true
        }
        Key::Char('+') => {
            push_calc_operator(&mut state.calculator, CalcOp::Add);
            true
        }
        Key::Char('-') => {
            if state.calculator.entry == "0" || state.calculator.fresh_entry {
                state.calculator.entry = "-".to_string();
                state.calculator.fresh_entry = false;
            } else {
                push_calc_operator(&mut state.calculator, CalcOp::Subtract);
            }
            true
        }
        Key::Char('*') | Key::Char('x') | Key::Char('X') => {
            push_calc_operator(&mut state.calculator, CalcOp::Multiply);
            true
        }
        Key::Char('/') => {
            push_calc_operator(&mut state.calculator, CalcOp::Divide);
            true
        }
        Key::Char('=') | Key::Enter => {
            finish_calc(&mut state.calculator);
            true
        }
        Key::Backspace => {
            if state.calculator.fresh_entry || state.calculator.entry.chars().count() <= 1 {
                state.calculator.entry = "0".to_string();
                state.calculator.fresh_entry = true;
            } else {
                state.calculator.entry.pop();
                if state.calculator.entry == "-" {
                    state.calculator.entry = "0".to_string();
                    state.calculator.fresh_entry = true;
                }
            }
            true
        }
        Key::Char('c') | Key::Char('C') | Key::Esc => {
            state.calculator = CalculatorState::new();
            true
        }
        Key::Char('m') | Key::Char('M') => {
            state.calculator.mode = match state.calculator.mode {
                CalcMode::SplitTryte => CalcMode::STryte,
                CalcMode::STryte => CalcMode::SplitTryte,
            };
            true
        }
        _ => false,
    }
}

fn push_calc_digit(calc: &mut CalculatorState, ch: char) {
    if calc.fresh_entry || calc.entry == "0" {
        calc.entry.clear();
        calc.fresh_entry = false;
    }
    calc.entry.push(ch);
}

fn push_calc_operator(calc: &mut CalculatorState, op: CalcOp) {
    let rhs = parse_calc_entry(&calc.entry).unwrap_or(0.0);
    if let (Some(lhs), Some(pending)) = (calc.accumulator, calc.pending_op) {
        if let Some(result) = pending.apply(lhs, rhs) {
            calc.last_expression = format_calc_expression(lhs, pending, rhs);
            calc.last_result = result;
            calc.accumulator = Some(result);
            calc.entry = format_calc_value(result);
        } else {
            calc.last_expression = format!("{} / {}", format_calc_value(lhs), format_calc_value(rhs));
            calc.entry = "ERR".to_string();
            calc.accumulator = None;
            calc.pending_op = None;
            calc.fresh_entry = true;
            return;
        }
    } else {
        calc.accumulator = Some(rhs);
    }
    calc.pending_op = Some(op);
    calc.fresh_entry = true;
}

fn finish_calc(calc: &mut CalculatorState) {
    let Some(op) = calc.pending_op else {
        calc.last_result = parse_calc_entry(&calc.entry).unwrap_or(calc.last_result);
        return;
    };
    let lhs = calc.accumulator.unwrap_or(0.0);
    let rhs = parse_calc_entry(&calc.entry).unwrap_or(0.0);
    if let Some(result) = op.apply(lhs, rhs) {
        calc.last_expression = format_calc_expression(lhs, op, rhs);
        calc.last_result = result;
        calc.entry = format_calc_value(result);
        calc.accumulator = Some(result);
        calc.pending_op = None;
        calc.fresh_entry = true;
    } else {
        calc.last_expression = format!("{} / {}", format_calc_value(lhs), format_calc_value(rhs));
        calc.entry = "ERR".to_string();
        calc.pending_op = None;
        calc.accumulator = None;
        calc.fresh_entry = true;
    }
}

fn parse_calc_entry(entry: &str) -> Option<f64> {
    entry.parse::<f64>().ok().filter(|value| value.is_finite())
}

fn format_calc_expression(lhs: f64, op: CalcOp, rhs: f64) -> String {
    format!("{} {} {}", format_calc_value(lhs), op.symbol(), format_calc_value(rhs))
}

fn format_calc_value(value: f64) -> String {
    if value.is_finite() && value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        let mut text = format!("{value:.8}");
        while text.contains('.') && text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
        text
    }
}

/// Handle Option 5 mouse events: left-click tab bar to switch notes,
/// scroll wheel to move cursor up/down in the editor.
fn handle_mouse(_store: &NoteStore, state: &mut TuiState, m: &MouseEvent, size: TerminalSize) {
    let columns = size.columns.max(MIN_COLUMNS);
    let rows = size.rows.max(MIN_ROWS);
    let content_rows = rows.saturating_sub(COMMAND_ROWS).max(12);

    let tab_row = 2usize;

    match m.button {
        // Left click
        0 if m.pressed => {
            if m.row == tab_row {
                let click_col = m.col.saturating_sub(2);
                let mut tab_cursor = 0usize;
                for tab in AppTab::ALL {
                    let label = format!("Tab{} {}", tab.number(), tab.title());
                    let tab_len = label.chars().count() + 2;
                    if click_col >= tab_cursor && click_col < tab_cursor + tab_len {
                        state.active_tab = tab;
                        state.status = format!("tab {}: {}", tab.number(), tab.title());
                        return;
                    }
                    tab_cursor += tab_len + 3;
                }
            } else if state.active_tab == AppTab::Editor {
                // Click in editor body area (rows 3..content_rows, col 1..left_width)
                let editor_body_start = 10usize;
                // offset 5 for line-number prefix "  N | " (varies, use 6)
                let line_num_cols = 6usize;
                if m.row >= editor_body_start
                    && m.row < editor_body_start + content_rows
                    && m.col >= 1
                    && m.col <= columns
                {
                    let viewport_row = m.row - editor_body_start;
                    let new_row = state.scroll_top + viewport_row;
                    if new_row < state.lines.len() {
                        state.cursor_row = new_row;
                        let inner_col = m.col.saturating_sub(1 + line_num_cols);
                        let line_len = char_count(&state.lines[new_row]);
                        state.cursor_col = inner_col.min(line_len);
                        state.status = format!("mouse: row {} col {}", new_row + 1, state.cursor_col + 1);
                    }
                }
            }
        }
        // Scroll wheel up (button 64) — scroll viewport up (TKB-27 MOUSE_SCROLL_UP)
        64 if m.pressed => {
            state.scroll_top = state.scroll_top.saturating_sub(1);
            // Keep cursor visible inside the new viewport
            let vr = compute_editor_visible_rows(size);
            if state.cursor_row >= state.scroll_top + vr {
                state.cursor_row = state.scroll_top + vr.saturating_sub(1);
                state.cursor_row = state.cursor_row.min(state.lines.len().saturating_sub(1));
                state.cursor_col = state.cursor_col.min(char_count(&state.lines[state.cursor_row]));
            }
        }
        // Scroll wheel down (button 65) — scroll viewport down (TKB-27 MOUSE_SCROLL_DOWN)
        65 if m.pressed => {
            let total = state.lines.len();
            let vr = compute_editor_visible_rows(size);
            let max_scroll = total.saturating_sub(vr);
            state.scroll_top = (state.scroll_top + 1).min(max_scroll);
            // Keep cursor visible
            if state.cursor_row < state.scroll_top {
                state.cursor_row = state.scroll_top;
                state.cursor_col = state.cursor_col.min(char_count(&state.lines[state.cursor_row]));
            }
        }
        _ => {}
    }
}

fn position_cursor(state: &TuiState, size: TerminalSize) -> String {
    let rows = size.rows.max(MIN_ROWS);
    if state.command_mode {
        format!("\x1b[{};{}H", rows, 6 + state.command_input.chars().count())
    } else if state.active_tab == AppTab::Editor {
        let visible_row = state.cursor_row.saturating_sub(state.scroll_top);
        format!("\x1b[{};{}H", 10 + visible_row, 8 + state.cursor_col)
    } else {
        format!("\x1b[{};1H", rows)
    }
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

    let result = match command {
        "q" | "quit" | "exit" => {
            if store.flush_if_dirty(artifact_path).map_err(TuiError::Store)? {
                state.status = format!("saved {}", artifact_path.display());
            }
            return Ok(true);
        }
        "q!" | "quit!" | "exit!" => return Ok(true),
        "new" => {
            let title = if rest.is_empty() { "untitled" } else { rest };
            let id = store.add(title, "");
            state.selected = Some(id);
            state.source_file = None;
            state.status = format!("created note {id}");
            false
        }
        "select" | "open-note" => {
            let id = parse_note_id(rest)?;
            if store.note(id).is_none() {
                return Err(TuiError::Store(NoteStoreError::NotFound(id)));
            }
            state.selected = Some(id);
            state.status = format!("selected note {id}");
            false
        }
        "title" => {
            let id = selected_id(state)?;
            let (_, body) = note_text(store, id)?;
            store.replace(id, rest, &body).map_err(TuiError::Store)?;
            state.status = format!("renamed note {id}");
            false
        }
        "append" | "a" => {
            let id = selected_id(state)?;
            let (title, body) = note_text(store, id)?;
            let next = append_line(&body, rest);
            store.replace(id, &title, &next).map_err(TuiError::Store)?;
            state.status = format!("appended line to note {id}");
            false
        }
        "set" => {
            let (line, text) = parse_line_text(rest)?;
            let id = selected_id(state)?;
            let (title, body) = note_text(store, id)?;
            let next = set_line(&body, line, text)?;
            store.replace(id, &title, &next).map_err(TuiError::Store)?;
            state.status = format!("set line {line}");
            false
        }
        "del" | "delete-line" => {
            let line = parse_line_number(rest)?;
            let id = selected_id(state)?;
            let (title, body) = note_text(store, id)?;
            let next = delete_line(&body, line)?;
            store.replace(id, &title, &next).map_err(TuiError::Store)?;
            state.status = format!("deleted line {line}");
            false
        }
        "clear" => {
            let id = selected_id(state)?;
            let (title, _) = note_text(store, id)?;
            store.replace(id, &title, "").map_err(TuiError::Store)?;
            state.status = format!("cleared note {id}");
            false
        }
        "body" | "replace" => {
            let id = selected_id(state)?;
            let (title, _) = note_text(store, id)?;
            println!("enter body, finish with a single '.' line");
            let body = read_body()?;
            store.replace(id, &title, &body).map_err(TuiError::Store)?;
            state.status = format!("replaced note {id} body");
            false
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
            false
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
            false
        }
        "save" | "flush" => {
            store.save(artifact_path).map_err(TuiError::Store)?;
            state.status = format!("saved {}", artifact_path.display());
            false
        }
        "watch" | "tick" => {
            state.compaction_step = state.compaction_step.saturating_add(1);
            state.status = format!("advanced compactor frame {}", state.compaction_step);
            false
        }
        "tab" => {
            let tab = AppTab::from_name(rest)
                .ok_or_else(|| TuiError::InvalidCommand("tab needs 1-5, main, reader, editor, calc, or obs".to_string()))?;
            state.active_tab = tab;
            state.status = format!("tab {}: {}", tab.number(), tab.title());
            false
        }
        "help" | "?" => {
            state.status = "commands: tab/new/select/title/append/set/del/body/open/write/save/watch/quit".to_string();
            false
        }
        other => return Err(TuiError::InvalidCommand(other.to_string())),
    };

    load_selected_note(store, state);
    Ok(result)
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
    let page_rows = content_rows.saturating_sub(2);
    let page_lines = page_lines(store, artifact_path, state, page_rows, columns)?;

    let mut out = String::new();
    write!(&mut out, "\x1b[H")?;
    let frame = frame_lines(state.active_tab.title(), &page_lines, columns, content_rows);

    for row in 0..content_rows {
        let line = frame.get(row).map(String::as_str).unwrap_or("");
        let color_line = colorize_left_line(line);
        writeln!(&mut out, "{color_line}\x1b[K")?;
    }

    writeln!(
        &mut out,
        "\x1b[38;2;96;96;96m{}\x1b[0m\x1b[K",
        "-".repeat(columns)
    )?;
    writeln!(&mut out, "{}\x1b[K", command_hint(columns))?;
    writeln!(&mut out, "\x1b[38;2;128;128;128mstatus: \x1b[0m\x1b[1;38;2;255;255;255m{}\x1b[0m\x1b[K", crop(&state.status, columns.saturating_sub(8)))?;
    
    if state.command_mode {
        write!(&mut out, "\x1b[1;38;2;0;255;255mcmd> \x1b[0m\x1b[38;2;255;255;255m{}\x1b[K", crop(&state.command_input, columns.saturating_sub(5)))?;
    } else {
        let footer = crop("Press : for command mode | Ctrl+S: Save | Ctrl+Q: Quit", columns);
        write!(&mut out, "\x1b[38;2;128;128;128m{}\x1b[0m\x1b[K", footer)?;
    }
    Ok(out)
}

fn page_lines(
    store: &NoteStore,
    artifact_path: &Path,
    state: &TuiState,
    max_rows: usize,
    columns: usize,
) -> Result<Vec<String>, TuiError> {
    let mut lines = vec![app_tab_bar(state.active_tab), String::new()];
    let mut body = match state.active_tab {
        AppTab::MainMenu => main_menu_lines(store, state, artifact_path),
        AppTab::Reader => reader_lines(store, state, artifact_path, max_rows.saturating_sub(2))?,
        AppTab::Editor => editor_lines(store, state, artifact_path, max_rows.saturating_sub(2))?,
        AppTab::Calculator => calculator_lines(&state.calculator, max_rows.saturating_sub(2), columns),
        AppTab::Observability => observation_lines(store, artifact_path, state, max_rows.saturating_sub(2))?,
    };
    lines.append(&mut body);
    Ok(lines)
}

fn app_tab_bar(active: AppTab) -> String {
    AppTab::ALL
        .iter()
        .map(|tab| {
            let label = format!("{} {}", tab.number(), tab.tab_label());
            if *tab == active {
                format!("[{label}]")
            } else {
                format!(" {label} ")
            }
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

fn main_menu_lines(store: &NoteStore, state: &TuiState, artifact_path: &Path) -> Vec<String> {
    let selected = state
        .selected
        .and_then(|id| store.note(id).map(|note| (id, note.title_text().unwrap_or_default())));
    let selected = selected
        .map(|(id, title)| format!("#{id} {}", if title.is_empty() { "untitled".to_string() } else { title }))
        .unwrap_or_else(|| "<none>".to_string());

    vec![
        "Main Menu".to_string(),
        format!("artifact: {}", artifact_path.display()),
        format!("notes in RAM: {}", store.notes().len()),
        format!("selected note: {selected}"),
        String::new(),
        "Tabs".to_string(),
        "  Tab 1: TUI Main Menu".to_string(),
        "  Tab 2: Notepad reader".to_string(),
        "  Tab 3: Notepad editor".to_string(),
        "  Tab 4: Calculator".to_string(),
        "  Tab 5: Trinary Memory observability readout".to_string(),
        String::new(),
        "Use Tab to cycle tabs, or command mode: tab 1..5".to_string(),
    ]
}

fn reader_lines(
    store: &NoteStore,
    state: &TuiState,
    artifact_path: &Path,
    max_body_rows: usize,
) -> Result<Vec<String>, TuiError> {
    let mut lines = note_header_lines(store, state, artifact_path)?;
    lines.push(String::new());
    lines.push("read-only body:".to_string());

    let total_lines = state.lines.len();
    let available = max_body_rows.saturating_sub(lines.len() + 1).max(1);
    let scroll = state.scroll_top.min(total_lines.saturating_sub(1));
    let end = (scroll + available).min(total_lines);

    if state.lines.is_empty() {
        lines.push("  1 | ".to_string());
    } else {
        for (offset, line) in state.lines[scroll..end].iter().enumerate() {
            lines.push(format!("{:>3} | {}", scroll + offset + 1, line));
        }
    }

    if total_lines > available {
        lines.push(format!("\u{2195} reader {}-{}/{}", scroll + 1, end, total_lines));
    }

    Ok(lines)
}

fn editor_lines(
    store: &NoteStore,
    state: &TuiState,
    artifact_path: &Path,
    max_body_rows: usize,
) -> Result<Vec<String>, TuiError> {
    let mut lines = note_header_lines(store, state, artifact_path)?;
    lines.push(String::new());

    // Visible body rows = available page rows minus note metadata and body label.
    let header_count = lines.len();
    let body_label_row = 1usize;
    let available = max_body_rows.saturating_sub(header_count + body_label_row + 1); // +1 gutter

    let total_lines = state.lines.len();
    let scroll = state.scroll_top.min(if total_lines > 0 { total_lines - 1 } else { 0 });
    let end = (scroll + available).min(total_lines);

    // Scroll-position gutter indicator (replaces "body:" label when scrolled)
    if scroll == 0 {
        lines.push("body:".to_string());
    } else {
        lines.push(format!(
            "body: \u{2195} L{}–{}/{}",
            scroll + 1, end, total_lines
        ));
    }

    if state.lines.is_empty() {
        lines.push("  1 | ".to_string());
    } else {
        for (offset, line) in state.lines[scroll..end].iter().enumerate() {
            let line_num = scroll + offset + 1;
            lines.push(format!("{:>3} | {}", line_num, line));
        }
    }

    // Bottom scroll gutter: show position info when content is clipped
    if total_lines > available {
        let pct = if total_lines > 0 { (end * 100) / total_lines } else { 100 };
        lines.push(format!(
            "\u{2195} {}/{} lines ({}%) | PgUp/PgDn scroll | [/] obs",
            end, total_lines, pct
        ));
    }

    Ok(lines)
}

fn note_header_lines(
    store: &NoteStore,
    state: &TuiState,
    artifact_path: &Path,
) -> Result<Vec<String>, TuiError> {
    let mut lines = Vec::new();
    lines.push(format!("artifact: {}", artifact_path.display()));
    if let Some(file) = &state.source_file {
        lines.push(format!("text file: {}", file.display()));
    } else {
        lines.push("text file: <none; use open/write>".to_string());
    }

    let Some(id) = state.selected else {
        lines.push("no selected note".to_string());
        return Ok(lines);
    };
    let note = store.note(id).ok_or(NoteStoreError::NotFound(id)).map_err(TuiError::Store)?;
    let title = note.title_text().unwrap_or_else(|_| "untitled".to_string());
    let body = note.body_text().unwrap_or_default();
    lines.push(format!(
        "selected: #{} {}{}",
        id,
        if title.is_empty() { "untitled" } else { &title },
        if store.is_dirty() { " *dirty" } else { "" }
    ));
    lines.push(format!(
        "note: title {} chars | body {} lines",
        title.chars().count(),
        body.lines().count().max(1)
    ));
    Ok(lines)
}

fn calculator_lines(calc: &CalculatorState, max_rows: usize, columns: usize) -> Vec<String> {
    let inner_width = columns.saturating_sub(2).max(76);
    let right_width = (inner_width / 4).max(24);
    let left_width = inner_width.saturating_sub(right_width + 3).max(40);
    let left = calculator_left_lines(calc, left_width);
    let right = calculator_trinary_lines(calc, right_width);
    let rows = max_rows.max(left.len()).max(right.len());
    let mut out = Vec::with_capacity(rows);

    for row in 0..rows {
        let l = crop(left.get(row).map(String::as_str).unwrap_or(""), left_width);
        let r = crop(right.get(row).map(String::as_str).unwrap_or(""), right_width);
        out.push(format!("{l:<left_width$} | {r:<right_width$}"));
    }

    out
}

fn calculator_left_lines(calc: &CalculatorState, width: usize) -> Vec<String> {
    let display = crop(&calc.entry, width.saturating_sub(4));
    vec![
        "Calculator".to_string(),
        format!("{display:>width$}"),
        String::new(),
        "+---------+---------+---------+---------+".to_string(),
        "|    7    |    8    |    9    |    /    |".to_string(),
        "|    4    |    5    |    6    |    *    |".to_string(),
        "|    1    |    2    |    3    |    -    |".to_string(),
        "|    0    |    .    |    =    |    +    |".to_string(),
        "+---------+---------+---------+---------+".to_string(),
        String::new(),
        "Keys: 0-9 . + - * / Enter".to_string(),
        "C clears, Backspace edits, M toggles mode".to_string(),
        format!("mode: {}", calc.mode_label()),
    ]
}

fn calculator_trinary_lines(calc: &CalculatorState, width: usize) -> Vec<String> {
    let value = calc.display_value();
    let integer = value.round() as i128;
    let balanced = balanced_trinary_digits(integer);
    let storage = balanced_storage_digits(&balanced);
    let mut lines = vec![
        "Balanced Trinary".to_string(),
        format!("mode: {}", calc.mode_label()),
        format!("expr: {}", if calc.last_expression.is_empty() { "<entry>" } else { &calc.last_expression }),
        format!("dec: {}", format_calc_value(value)),
    ];

    if value.fract() != 0.0 {
        lines.push(format!("rounded: {}", format_calc_value(integer as f64)));
    }
    lines.push(format!("bt: {}", crop(&balanced, width.saturating_sub(4))));
    lines.push(format!("storage 0/1/2: {}", crop(&storage, width.saturating_sub(15))));
    lines.push(String::new());

    match calc.mode {
        CalcMode::SplitTryte => {
            lines.push("SplitTryte groups".to_string());
            for (index, group) in splittryte_groups(&balanced).iter().enumerate() {
                lines.push(format!("{index:>2}: {group}"));
            }
        }
        CalcMode::STryte => {
            lines.push("sTryte words".to_string());
            for (index, line) in stryte_lines(&balanced).iter().enumerate() {
                lines.push(format!("{index:>2}: {line}"));
            }
        }
    }

    lines
}

fn balanced_trinary_digits(mut value: i128) -> String {
    if value == 0 {
        return "0".to_string();
    }

    let mut digits = Vec::new();
    while value != 0 {
        let mut remainder = value % 3;
        value /= 3;
        if remainder == 2 {
            remainder = -1;
            value += 1;
        } else if remainder == -2 {
            remainder = 1;
            value -= 1;
        }
        digits.push(match remainder {
            -1 => '-',
            0 => '0',
            1 => '+',
            _ => '0',
        });
    }
    digits.iter().rev().collect()
}

fn balanced_storage_digits(balanced: &str) -> String {
    balanced
        .chars()
        .map(|ch| match ch {
            '-' => '0',
            '0' => '1',
            '+' => '2',
            _ => '1',
        })
        .collect()
}

fn splittryte_groups(balanced: &str) -> Vec<String> {
    balanced
        .chars()
        .collect::<Vec<_>>()
        .rchunks(8)
        .rev()
        .map(|chunk| {
            let group: String = chunk.iter().collect();
            let padded = format!("{group:0>8}");
            format!("[11|0|{}|{}|{}|{}]", &padded[0..1], &padded[1..4], &padded[4..5], &padded[5..8])
        })
        .collect()
}

fn stryte_lines(balanced: &str) -> Vec<String> {
    balanced
        .chars()
        .collect::<Vec<_>>()
        .rchunks(TRITS_PER_TRYTE)
        .rev()
        .map(|chunk| {
            let mut trits = [Trit::Neutral; TRITS_PER_TRYTE];
            let offset = TRITS_PER_TRYTE.saturating_sub(chunk.len());
            for (index, ch) in chunk.iter().enumerate() {
                trits[offset + index] = match ch {
                    '-' => Trit::Negative,
                    '0' => Trit::Neutral,
                    '+' => Trit::Positive,
                    _ => Trit::Neutral,
                };
            }
            let stryte = STryte::from_trits(trits);
            format!("{} 0x{:08x}", stryte.as_storage_digits(), stryte.word())
        })
        .collect()
}

fn observation_lines(
    store: &NoteStore,
    artifact_path: &Path,
    state: &TuiState,
    max_obs_rows: usize,
) -> Result<Vec<String>, TuiError> {
    let rendered = match state.selected {
        Some(id) if store.note(id).is_some() => {
            observe::render_note(store, artifact_path, id, Some(state.compaction_step))
                .map_err(TuiError::Observe)?
        }
        _ => observe::render_overview(store, artifact_path).map_err(TuiError::Observe)?,
    };
    let all_lines: Vec<String> = rendered.lines().map(str::to_string).collect();
    let total = all_lines.len();
    let scroll = state.obs_scroll_top.min(if total > 0 { total - 1 } else { 0 });
    let end = (scroll + max_obs_rows).min(total);
    let mut out: Vec<String> = all_lines[scroll..end].to_vec();
    // Show a scroll indicator at the bottom of the obs pane if clipped
    if total > max_obs_rows {
        out.push(format!(
            "\u{2195} obs {}–{}/{} | [/] scroll",
            scroll + 1, end, total
        ));
    }
    Ok(out)
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
        "Tab: cycle pages | :tab 1..5 | :new | :open | :save | :help | :quit",
        columns,
    )
}

fn terminal_size() -> TerminalSize {
    #[cfg(windows)]
    {
        use std::ffi::c_void;
        type HANDLE = *mut c_void;
        type SHORT = i16;
        type WORD = u16;
        type DWORD = u32;
        
        #[repr(C)]
        struct COORD {
            x: SHORT,
            y: SHORT,
        }
        #[repr(C)]
        struct SMALL_RECT {
            left: SHORT,
            top: SHORT,
            right: SHORT,
            bottom: SHORT,
        }
        #[repr(C)]
        struct CONSOLE_SCREEN_BUFFER_INFO {
            dw_size: COORD,
            dw_cursor_position: COORD,
            w_attributes: WORD,
            sr_window: SMALL_RECT,
            dw_maximum_window_size: COORD,
        }
        
        extern "system" {
            fn GetStdHandle(nStdHandle: DWORD) -> HANDLE;
            fn GetConsoleScreenBufferInfo(
                hConsoleOutput: HANDLE,
                lpConsoleScreenBufferInfo: *mut CONSOLE_SCREEN_BUFFER_INFO,
            ) -> i32;
        }
        
        unsafe {
            let h_out = GetStdHandle(-11i32 as u32);
            let mut info = std::mem::zeroed();
            if GetConsoleScreenBufferInfo(h_out, &mut info) != 0 {
                let columns = (info.sr_window.right - info.sr_window.left + 1) as usize;
                let rows = (info.sr_window.bottom - info.sr_window.top + 1) as usize;
                return TerminalSize { columns, rows };
            }
        }
    }

    #[cfg(not(windows))]
    {
        if let Some(size) = fallback::terminal_size() {
            return size;
        }
    }

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
    fn tabbed_render_keeps_editor_and_observability_separate() {
        let mut store = NoteStore::new();
        let id = store.add("doc.md", "# Heading\nbody");
        let mut state = TuiState {
            active_tab: AppTab::Editor,
            selected: Some(id),
            source_file: Some(PathBuf::from("doc.md")),
            compaction_step: 1,
            status: "testing".to_string(),
            lines: vec!["# Heading".to_string(), "body".to_string()],
            cursor_row: 0,
            cursor_col: 0,
            command_mode: false,
            command_input: String::new(),
            selection: None,
            scroll_top: 0,
            obs_scroll_top: 0,
            calculator: CalculatorState::new(),
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

        assert!(rendered.contains("Notepad Editor"));
        assert!(rendered.contains("5 Observability"));
        assert!(rendered.contains("# Heading"));
        assert!(rendered.contains("sTryte") == false);
        assert!(rendered.contains("cmd>") == false);

        state.active_tab = AppTab::Observability;
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
        assert!(rendered.contains("Trinary Memory Observability"));
        assert!(rendered.contains("sTryte"));
    }

    #[test]
    fn line_edit_commands_update_selected_note() {
        let mut store = NoteStore::new();
        let id = store.add("note", "first");
        let mut state = TuiState {
            active_tab: AppTab::Editor,
            selected: Some(id),
            source_file: None,
            compaction_step: 0,
            status: String::new(),
            lines: vec!["first".to_string()],
            cursor_row: 0,
            cursor_col: 0,
            command_mode: false,
            command_input: String::new(),
            selection: None,
            scroll_top: 0,
            obs_scroll_top: 0,
            calculator: CalculatorState::new(),
        };
        let path = temp_path("tui-edit");

        handle_command(&mut store, &path, &mut state, "append second").unwrap();
        handle_command(&mut store, &path, &mut state, "set 1 changed").unwrap();
        handle_command(&mut store, &path, &mut state, "del 2").unwrap();

        assert_eq!(store.note(id).unwrap().body_text().unwrap(), "changed");
    }

    #[test]
    fn calculator_renders_balanced_trinary_side_panel() {
        let store = NoteStore::new();
        let mut state = TuiState {
            active_tab: AppTab::Calculator,
            selected: None,
            source_file: None,
            compaction_step: 0,
            status: String::new(),
            lines: vec![String::new()],
            cursor_row: 0,
            cursor_col: 0,
            command_mode: false,
            command_input: String::new(),
            selection: None,
            scroll_top: 0,
            obs_scroll_top: 0,
            calculator: CalculatorState::new(),
        };

        for key in [
            Key::Char('1'),
            Key::Char('0'),
            Key::Char('+'),
            Key::Char('2'),
            Key::Enter,
        ] {
            assert!(handle_calculator_key(&mut state, &key));
        }

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

        assert!(rendered.contains("Calculator"));
        assert!(rendered.contains("Balanced Trinary"));
        assert!(rendered.contains("SplitTryte groups"));
        assert!(rendered.contains("bt: ++0"));

        assert!(handle_calculator_key(&mut state, &Key::Char('m')));
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
        assert!(rendered.contains("sTryte words"));
        assert!(rendered.contains("0x"));
    }

    #[test]
    fn open_and_write_plain_text_file() {
        let input = temp_path("tui-input.md");
        let output = temp_path("tui-output.md");
        fs::write(&input, "# Title\nbody").unwrap();
        let mut store = NoteStore::new();
        let mut state = TuiState {
            active_tab: AppTab::Editor,
            selected: None,
            source_file: None,
            compaction_step: 0,
            status: String::new(),
            lines: Vec::new(),
            cursor_row: 0,
            cursor_col: 0,
            command_mode: false,
            command_input: String::new(),
            selection: None,
            scroll_top: 0,
            obs_scroll_top: 0,
            calculator: CalculatorState::new(),
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

    #[test]
    fn select_word_at_finds_alphanumeric_boundaries() {
        // Middle of word
        assert_eq!(select_word_at("hello world", 2), (0, 4));
        // Start of second word
        assert_eq!(select_word_at("hello world", 6), (6, 10));
        // On a space (not alphanumeric) — start==end at that position
        let (s, e) = select_word_at("hello world", 5);
        assert!(s <= e);
        // Underscore is part of word
        assert_eq!(select_word_at("foo_bar", 3), (0, 6));
        // Empty string
        assert_eq!(select_word_at("", 0), (0, 0));
    }

    #[test]
    fn gesture_tracker_detects_double_tap() {
        let mut tracker = GestureTracker::new();
        let click = MouseEvent { button: 0, col: 10, row: 5, pressed: true };

        // First tap → plain Tap
        let g1 = tracker.feed(&click, 120);
        assert!(matches!(g1, Some(Gesture::Tap { .. })));

        // Second tap immediately after at same position → DoubleTap
        let g2 = tracker.feed(&click, 120);
        assert!(matches!(g2, Some(Gesture::DoubleTap { .. })));

        // After double-tap, state is cleared — next tap is a fresh Tap
        let g3 = tracker.feed(&click, 120);
        assert!(matches!(g3, Some(Gesture::Tap { .. })));
    }

    #[test]
    fn gesture_tracker_swipe_up_fires_after_burst() {
        let mut tracker = GestureTracker::new();
        let scroll_up = MouseEvent { button: 64, col: 40, row: 10, pressed: true };

        // Two events — not enough for swipe
        assert!(tracker.feed(&scroll_up, 120).is_none());
        assert!(tracker.feed(&scroll_up, 120).is_none());
        // Third event within burst window — fires SwipeUp
        let g = tracker.feed(&scroll_up, 120);
        assert!(matches!(g, Some(Gesture::SwipeUp)));
    }

    #[test]
    fn parser_keeps_all_keys_from_one_stdin_read() {
        let keys = parse_keys(b":quit!\r");

        assert!(matches!(keys[0], Key::Char(':')));
        assert!(matches!(keys[1], Key::Char('q')));
        assert!(matches!(keys[2], Key::Char('u')));
        assert!(matches!(keys[3], Key::Char('i')));
        assert!(matches!(keys[4], Key::Char('t')));
        assert!(matches!(keys[5], Key::Char('!')));
        assert!(matches!(keys[6], Key::Enter));
    }

    #[test]
    fn parser_consumes_escape_sequences_without_dropping_following_keys() {
        let keys = parse_keys(b"\x1b[A:");

        assert!(matches!(keys[0], Key::Up));
        assert!(matches!(keys[1], Key::Char(':')));
    }
}
