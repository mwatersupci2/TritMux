# Trit Storage Architecture

TritMux treats the trit block sizes as alignment targets, not arbitrary packing
constants. The current prototype is sized so later storage, networking, bridge,
and PCIe-oriented work can reason about cache lines, packet boundaries, and
flush pressure without rewriting the basic memory model.

## Lane States

Each trit lane is two bits:

- `00`: low / `-1`
- `01`: middle / unknown
- `10`: high / `+1`
- `11`: boundary marker or misc

`11` is a lane state, but it is not a value trit. Value trits are the first
three states only.

## Hot To Cold Pipeline

```text
Trit Lane
  -> sTryte/u32
  -> pTryte/u32 lanes
  -> U320
  -> U640
  -> 9x U640 row
  -> 9x9 U640 grid
  -> base-3 integer encoding on disk
```

Hot values stay as sTryte-backed payloads in RAM. Rare values can stay zipped
on disk as base-3 U640 grid artifacts and be unzipped on demand.

Trytes are type-agnostic. Programmer-facing types are represented above them as
tagged scalar payloads: text, Unicode, raw bytes, signed and unsigned integers,
floats, big integer magnitudes, exact decimals, and exact fractions all encode
to the same trit payload transport.

The KV layer uses this scalar transport directly. It indexes typed scalar keys
at load time, keeps hot scalar values decoded in RAM or in managed trinary heap
regions, and keeps rare values cold as zipped base-3 9x9 U640 lines until a
lookup promotes them.

The current implementation is a synchronous in-process model. It does not yet
allocate separate OS memory regions, run OS worker processes, or perform async
unzip work.

## Memory Management

Trinary RAM is managed as explicit heap and stack regions. A region owns a
buffer of sTrytes plus metadata for used trits, residency, pin count, stack
frame ownership, and best-effort host pinning.

Logical pinning is the authoritative TritMux rule: pinned regions cannot be
compacted, resized, evicted, or popped from a stack frame. On Linux and Android,
the memory layer also calls `mlock` for pinned sTryte buffers and `munlock` when
the last logical pin is released. This reduces host paging risk, but it is still
subject to OS limits.

## Runtime Regions

The TUI runtime model is organized around state regions and logical processes:

- tabs contain windows
- windows attach to state regions
- regions can be shared by multiple windows
- regions can be duplicated as independent snapshots
- duplicated regions preserve the source region id for traceability
- managed regions point at `TrinaryMemoryManager` heap regions
- duplicating a managed runtime region forks the underlying sTryte buffer
- logical processes represent applications, subapplications, background
  workers, and system tasks
- processes can attach windows, regions, and one stack frame for local working
  state
- process lifecycle is explicit: created, running, suspended, stopped, or failed

This supports multiple tabs and multiple windows per tab while keeping the
state model explicit. Today the regions and processes are logical handles inside
one Rust process. The next step toward real async behavior is to put command
draining behind workers while preserving these same ids.

## Command Bus

Runtime, process, memory, and KV operations now flow through a command/event
bus. Commands carry a source (`Runtime`, `Tab`, `Window`, or `Worker`) and a
reply target. Events are routed back to that target, which lets future TUI
windows drive backend work without calling storage APIs directly.

The bus is still synchronous. It is intentionally shaped so an async executor,
worker threads, or process-backed subapplications can later drain the same
command envelopes and emit the same events.

## Worker Pump

Workers are currently deterministic pumps over the command bus. A worker has an
id, kind, lifecycle state, counters, and a per-tick command budget. Ticking a
worker processes queued command envelopes and leaves all resulting events in the
same event queue used by direct command bus calls.

This keeps scheduling explicit without pulling in an async runtime. The model is
ready to move behind background tasks later, but the current code stays on one
thread so the trinary memory manager does not need unsafe cross-thread promises.

## Observability Applet

The first applet is a dependency-free ANSI terminal view over RAM, disk, and
compaction state. It reports:

- store-level RAM totals for notes, trits, sTrytes, pTrytes, U640 blocks, and
  9x9 grids
- disk artifact totals, base-3 line previews, and `0`/`1`/`2` digit counts
- per-payload sTryte `u32` words with marker-bit status
- pTryte/U320 previews
- U640 previews as both `u32` storage words and diagnostic `u64` pairs
- zipped base-3 U640 line previews
- compactor windows with settle, active, lookahead, and free sTryte regions

The `watch` command animates the existing compaction trace by advancing the
active window each frame. That gives a real view of which sTryte indexes the
compactor would lock and convert at each step, while staying synchronous until
the worker layer becomes a real background executor.

## Split TUI Frontend

The first frontend is a split-pane ANSI TUI. It keeps two logical windows on
screen:

- an editor window for the selected note body, line-numbered text/Markdown
  content, source file path, and artifact dirty state
- an observability window for the same selected note payload, including RAM,
  disk, compactor window, pTryte/U320, U640, and base-3 zip state

Editing commands update the selected note immediately, so every redraw uses the
current trinary payload. Text and Markdown files can be opened into a new
trinary note and written back out as normal files. The trinary artifact remains
the authoritative persistence format for the note store.

This frontend does not yet use raw-key editing or a terminal UI crate. That is
intentional for Termux portability and to keep the project dependency-free while
the backend memory model is still moving.

## Compaction Guard

The compactor moves through sTrytes with a sliding mutex window:

- 2 sTrytes settle behind the active index
- 1 active sTryte is being shifted
- 4 sTrytes are held as lookahead

This gives compaction room to bit shift pTryte lanes while preventing readers
or writers from corrupting the window currently being converted.
