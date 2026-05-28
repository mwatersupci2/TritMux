# TritMux

TritMux is a pure Rust prototype for a Termux-friendly trinary note editor.

The first slice is intentionally small:

- no third-party Rust crates
- split-pane ANSI TUI plus a line-mode terminal shell
- notes stored as packed sTryte payloads in RAM
- notes persisted as zipped base-3 U640 grid artifacts on disk

The current storage artifact is a text file containing only `0`, `1`, `2`, and
newlines. Each logical note record is serialized to bytes, encoded into trits,
held hot in RAM as sTrytes, compacted into pTrytes/U320/U640, grouped into 9x9
U640 grids, then written as base-3 integer lines.

## Run

```sh
cargo run
```

Open the split editor/observability frontend directly with:

```sh
cargo run -- --tui
```

By default the note artifact is written to `tritmux-notes.trit` in the current
directory. Override it with:

```sh
TRITMUX_STORE=/path/to/notes.trit cargo run
cargo run -- --store /path/to/notes.trit --tui
```

## Commands

- `new` / `add`: create a note
- `list`: list notes
- `show <id>`: show a note
- `edit <id>`: replace a note title and body
- `delete <id>`: delete a note
- `save` / `flush`: persist dirty notes
- `observe [id]`: show RAM, disk, compaction, U320/U640, and base-3 state
- `watch <id> [frames]`: animate the compactor lock window for a note
- `tui`: open the split editor/observability frontend
- `path`: show the artifact path
- `help`: show commands
- `quit` / `exit`: save if dirty, then exit

For multi-line body input, finish with a single `.` on its own line.

## Split TUI

The split frontend keeps the notepad editor and observability applet on screen
together:

- left pane: selected note title/body, normal text and Markdown content, source
  file path, dirty state, and line numbers
- right pane: live trinary view for the same selected note, including sTrytes,
  pTrytes, U320/U640, base-3 zip previews, and compactor lock windows
- command strip: edit/save/open/export commands without a third-party terminal
  dependency

TUI commands:

- `new <title>`: create and select a note
- `select <id>`: select an existing note
- `title <text>`: rename selected note
- `append <text>`: append a line
- `set <line> <text>`: replace one body line
- `del <line>`: delete one body line
- `body`: replace the full body until a single `.` line
- `open <file.txt|file.md>`: load a text/Markdown file into a new trinary note
- `write [file.txt|file.md]`: export the selected note body as text/Markdown
- `save`: flush the trinary artifact
- `watch`: advance the compactor frame shown in the observability pane
- `quit` / `quit!`: exit with or without flushing dirty trinary notes

This first TUI is ANSI redraw and command driven. It gives the two-window
workflow now, while leaving raw keystroke editing and true async workers for the
next layer.

## Architecture Direction

The code is organized around the uploaded trit architecture diagram:

- `trit::core`: primitive balanced trit values
- `trit::logic`: 3-state boolean logic
- `trit::tryte`: sTryte, a spaced 9-trit word in a marked `u32`
- `trit::block`: pTryte U320/U640 compaction blocks
- `trit::compact`: sliding mutex windows for guarded compaction
- `trit::zip`: U640, 9x U640, 9x9 U640, and base-3 disk encoding
- `trit::payload`: stored value payloads
- `trit::scalar`: tagged values for programmer-facing types
- `trit::storage_artifact`: disk artifact encoding
- `commands`: synchronous command/event bus for runtime, process, memory, and KV work
- `kv`: hot/cold typed key-value storage over scalar payloads
- `memory`: trinary heap/stack regions with logical and host pinning
- `observe`: observability snapshots and ANSI applet rendering
- `runtime`: tab/window/session/process regions for future async TUI state
- `tui`: split editor/observability frontend
- `workers`: std-only command pump for future executor/worker scheduling
- `notes`: note records over trit payloads with dirty flush tracking

Later UI work can replace the line-mode shell with a richer TUI while keeping
the note store and artifact format stable.

## Trit Lane Layout

There are four trit lane states, each represented with two bits:

- `00`: negative
- `01`: neutral
- `10`: positive
- `11`: boundary marker or misc

An sTryte stores 9 trit lanes in the lower 18 bits of a `u32`. The unused upper
14 bits are set to `1`, making them a marker region.

A U320 pTryte block is ten `u32` words. It packs 16 pTrytes; each pTryte is 9
trit lanes plus one marker lane. Two U320 blocks join as a U640 block without
using a large integer type.

The cold-storage zip pipeline is:

```text
Trit Lane -> sTryte/u32 -> pTryte/u32 lanes -> U320 -> U640
    -> 9x U640 row -> 9x9 U640 grid -> base-3 integer lines on disk
```

On load, the decoder reads base-3 integer lines into 9x9 U640 grids, unzips the
needed U640 blocks back through pTrytes, and reconstructs sTrytes for hot RAM
payloads.

## Typed Values

Trytes stay type-agnostic. Programmer-facing values are encoded one layer above
them as tagged `trit::scalar` payloads. The current scalar codec supports:

- null, bool, trit, and trit lane values
- UTF-8 text, including emoticons and other Unicode
- raw bytes
- signed and unsigned 64-bit and 128-bit integers
- 32-bit and 64-bit floats
- arbitrary-size signed and unsigned integer magnitudes
- exact decimals as sign + coefficient + scale
- exact fractions as numerator + nonzero denominator

Once encoded, scalar values travel through the same sTryte, pTryte, U320, U640,
9x9 grid, and base-3 disk pipeline as note text.

## KV Storage

The `kv` module is the first backend-oriented store. It uses typed scalar keys
and scalar values:

- hot values are decoded `TritScalar` instances in RAM
- managed hot values can live in `memory` heap regions
- cold values remain zipped as base-3 9x9 U640 grid lines
- loading indexes keys but leaves values cold
- `get` unzips and promotes a value to hot RAM on demand
- `get_managed` unzips and promotes a value into a managed trinary heap region
- `flush` writes the full trinary-only artifact back to disk

This is currently a synchronous in-process store. It is not yet split into
separate OS memory regions, async workers, or OS processes.

## Runtime Regions

The `runtime` module models the state shape needed by the future TUI:

- a session owns tabs, windows, and state regions
- each tab can contain multiple windows
- each window points at a region
- multiple windows can share one region and see the same state
- a region can be duplicated into an independent snapshot for another tab,
  window, process, application, or subapplication
- managed runtime regions allocate through `TrinaryMemoryManager`
- duplicating a managed region duplicates the underlying sTryte buffer instead
  of sharing the same memory region
- logical processes model applications, subapplications, background workers, and
  system tasks
- a process can attach windows, regions, and one stack frame
- process state is tracked as created, running, suspended, stopped, or failed

Regions are guarded with `Arc<RwLock<...>>`, so the code is ready to be moved
behind async tasks or worker threads later. The current implementation is still
synchronous and in-process; these are not OS processes yet, and TritMux does not
yet start worker processes or run an async executor.

## Memory Management

The `memory` module manages trinary RAM regions separately from the host Rust
heap shape:

- heap regions for long-lived trit payloads
- stack frames for app/subapp/process-local working state
- hot, warm, cold, flushable, and pinned residency classes
- logical pin counts so the TritMux runtime will not compact, evict, or resize
  protected sTryte buffers
- best-effort host pinning through `mlock`/`munlock` on Linux and Android

Host pinning can still fail because Android/Linux impose process limits. TritMux
therefore treats OS pinning as best effort and always keeps the logical pinning
rules active.

## Command Bus

The `commands` module is the backend contract for tabs, windows, and future
workers:

- commands are submitted with a source: runtime, tab, window, or worker
- callers can choose a reply target for routed events
- region commands create KV/scalar/byte regions, duplicate regions, create
  tabs, and open windows
- process commands create app/subapp/worker descriptors, drive lifecycle state,
  and attach windows, regions, or stack frames
- memory commands pin, unpin, resize, manage stack frames, and report stats
- KV commands put/get/delete/flush values, including managed hot-value paths
- failures are emitted as routed `CommandFailed` events

This bus is synchronous today. It gives the TUI and future async worker model a
stable command/event boundary before we introduce an executor or process pool.

## Worker Pump

The `workers` module adds a deterministic command pump:

- worker ids map to `CommandSource::Worker`
- each tick drains up to a caller-provided command budget
- workers can pause, resume, drain all queued commands, and report stats
- emitted events still flow through the normal command bus queue

This is not a thread pool yet. It is the dependency-free executor boundary we
can later move behind async tasks or OS threads once the memory ownership rules
are tight enough.

## Observability Applet

The first built-in applet is `observe`:

- RAM totals for notes, trits, sTrytes, pTrytes, U640 blocks, and 9x9 grids
- disk totals for artifact bytes, base-3 lines, and digit counts
- per-payload sTryte `u32` previews with marker-bit status
- pTryte/U320 previews and diagnostic U640-as-`u64` pair views
- zipped base-3 U640 previews
- compactor lock windows showing settle, active, lookahead, and free regions

`observe <id>` renders the current note payload state. `watch <id> [frames]`
redraws the view and advances through the compactor windows so the lock window
can be inspected frame by frame. This is still a synchronous ANSI terminal
applet, not a full-screen TUI framework.

## Sliding Compaction Guard

The in-place compactor uses a sliding mutex window before converting sTrytes
into pTrytes:

- 2 sTrytes settle behind the active index
- 1 active sTryte is being shifted
- 4 sTrytes are held as lookahead

The guarded range is therefore up to 7 sTrytes wide. Writers must check the
window before touching a payload slot; locked slots are treated as unavailable
until the window slides forward.
