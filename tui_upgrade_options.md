# TUI Upgrade Options for TritMux

We can upgrade our Text User Interface (TUI) to feel significantly more premium, responsive, and visual. Below is a detailed breakdown of the potential upgrades, their visual impact, and how we can implement them using standard Rust (`std::io` and standard terminal ANSI escape codes, without external crates).

---

## Upgrade Options Matrix

| Option | Visual & Interactive Impact | Implementation Complexity | Highlight |
| :--- | :--- | :--- | :--- |
| **1. TCF-27 Palette & ANSI Colorization** | **High**: Rich, modern UI with accented status bars, colored memory blocks, and syntax-highlighted Markdown. | Low | Modern, sleek dark mode theme using trinary-themed colors. |
| **2. Raw Terminal Mode & Interactive Keyboard Editing** | **Critical**: Real-time cursor movement (arrows), immediate character typing, and backspace. Replaces line commands. | Medium | True character-by-character screen editor like `nano`/`micro`. |
| **3. Visual Tab Headers & Note Switcher** | **Medium**: Neatly formatted tabs at the top of the editor pane to swap notes instantly. | Low | Interactive note management. |
| **4. Real-time Compactor Animations** | **High**: The memory visualization animates live in the background while typing, rather than on manual commands. | Medium-High | Asynchronous compaction thread/timer. |
| **5. Mouse Interactive Controls** | **Medium**: Click tabs, position cursors, and scroll views via mouse. | Medium | Sleek CLI mouse integration. |

---

## Detailed Upgrade Breakdowns

### 1. TCF-27 Palette & ANSI Colorization
Currently, the TUI is monochrome text. We can leverage ANSI 24-bit direct color codes (`\x1b[38;2;R;G;Bm` for foreground, `\x1b[48;2;R;G;Bm` for background) to apply the standardized **TCF-27 81-color palette**.
* **Visual Additions**:
  * **Editor Pane**: Highlight Markdown headers (e.g. `# title` in vibrant blue/azure, list items in yellow/orange).
  * **Observability Pane**: Color code the compactor window:
    * `A` (Active pointer) $\rightarrow$ Vibrant Red (`#FF0000`)
    * `S` (Settle area) $\rightarrow$ Vibrant Orange/Yellow (`#FFC000` / `#FFFF00`)
    * `L` (Lookahead guard) $\rightarrow$ Vibrant Azure/Blue (`#00C0FF` / `#0000FF`)
    * `.` (Free memory) $\rightarrow$ Dark Slate Gray (`#404040`)
  * **Borders & Status**: Soft neon cyan borders (`#00FFFF`) and a dark gray background status bar with white text.

### 2. Raw Terminal Mode & Interactive Keyboard Editing
Currently, we block on `io::stdin().read_line()`, requiring the user to type `set 1 some text` or `append text` followed by Enter. 
* **Interactive Additions**:
  * Put the terminal into **raw mode** using ANSI escape sequences (`\x1b[?1049h` for alternate screen buffer, disabling line buffering and local echo).
  * Capture key presses instantly:
    * **Arrow keys** (`\x1b[A`, `\x1b[B`, etc.) to move a text cursor around the note.
    * **Backspace** (`\x7f` or `\x08`) to delete characters.
    * **Enter** (`\r` or `\n`) to insert newlines.
    * Direct typing to insert text at the cursor position.
  * Command prompt transitions to a overlay or hotkey menu (e.g. pressing `:` to enter command mode, similar to `vi`).

### 3. Visual Tab Headers & Note Switcher
We can make multitasking between notes clean and beautiful.
* **Visual Additions**:
  * Render a row of tab headers at the top of the Editor panel:
    * `[ #1 draft.md * ]  [ #2 todo.md ]  [ + New Note ]`
  * Add keyboard shortcuts (e.g., `Alt+1`, `Alt+2` or `Tab`) to cycle through open notes.

### 4. Real-time Compactor Animations
The compactor observability grid can animate in real-time.
* **Interactive Additions**:
  * Run a background thread to handle ticks or read stdin with a timeout.
  * When the user sits idle, the right pane's window range `A = active`, `S = settle`, `L = lookahead` shifts smoothly across the screen.
  * Provides an active visual loop that makes the TUI feel "alive".

### 5. Mouse Interactive Controls
Terminals support mouse hover and click reporting through standard escape codes (`\x1b[?1000h` / `\x1b[?1006h`).
* **Interactive Additions**:
  * Users can click a tab header to switch active notes.
  * Click anywhere inside the edit pane to place the text cursor.
  * Double-click words to select them.

### 6. Mobile & Touch Screen Pipeline (Termux on Android via TKB-27)
We can map phone taps, swipes, and multi-finger gestures from Termux on Android directly into our trinary keyboard and bindings bank (`TKB-27`).
* **Mobile/Touch Additions**:
  * **Taps to Clicks (Bank -)**: Single finger taps on the mobile terminal are reported as SGR mouse clicks (`MOUSE_CLICK_L`), enabling users to navigate tabs and edit notes by touching the phone screen.
  * **Swipes to Scrolls (Bank -)**: Finger dragging/swiping on screen generates mouse wheel reports (`MOUSE_SCROLL_UP/DOWN`), allowing natural scrolling of documents.
  * **Native Gestures (Bank 0)**: Provides mapping structures for custom mobile shell gestures (like double-taps, long presses, pinches, and edge-swipes) directly into raw trinary representation.

---

## Next Steps

We can implement these one by one or combine them into a single comprehensive upgrade plan. Let's decide which features we want to introduce.
