use crate::kv::{KvError, KvStore};
use crate::memory::{
    HostPinState, MemoryError, MemoryRegionId, MemoryStats, Residency, StackFrameId,
};
use crate::runtime::{
    ProcessId, ProcessKind, ProcessState, RegionId, RegionPayload, RuntimeError,
    RuntimeSession, TabId, WindowId, WindowRole,
};
use crate::trit::scalar::TritScalar;
use std::collections::VecDeque;
use std::error::Error;
use std::fmt;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CommandId(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandSource {
    Runtime,
    Tab(TabId),
    Window(WindowId),
    Worker(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventTarget {
    Runtime,
    Tab(TabId),
    Window(WindowId),
    Worker(u64),
    Region(RegionId),
    Memory(MemoryRegionId),
    Broadcast,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CommandEnvelope {
    id: CommandId,
    source: CommandSource,
    reply_to: EventTarget,
    command: TritMuxCommand,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EventEnvelope {
    command_id: CommandId,
    source: CommandSource,
    target: EventTarget,
    event: TritMuxEvent,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TritMuxCommand {
    Region(RegionCommand),
    Process(ProcessCommand),
    Memory(MemoryCommand),
    Kv(KvCommand),
}

#[derive(Debug, Clone, PartialEq)]
pub enum RegionCommand {
    CreateKv {
        name: String,
    },
    CreateScalar {
        name: String,
        value: TritScalar,
        residency: Residency,
    },
    CreateBytes {
        name: String,
        bytes: Vec<u8>,
        residency: Residency,
    },
    Duplicate {
        source: RegionId,
        name: String,
    },
    CreateTab {
        title: String,
    },
    OpenWindow {
        tab_id: TabId,
        title: String,
        role: WindowRole,
        app_name: String,
        region_id: RegionId,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum MemoryCommand {
    Pin {
        region_id: MemoryRegionId,
    },
    Unpin {
        region_id: MemoryRegionId,
    },
    Resize {
        region_id: MemoryRegionId,
        stryte_count: usize,
    },
    PushStackFrame {
        name: String,
    },
    AllocateStack {
        frame_id: StackFrameId,
        name: String,
        stryte_count: usize,
    },
    PopStackFrame,
    Stats,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProcessCommand {
    Create {
        name: String,
        kind: ProcessKind,
        parent: Option<ProcessId>,
        tab_id: Option<TabId>,
    },
    Start {
        process_id: ProcessId,
    },
    Suspend {
        process_id: ProcessId,
    },
    Resume {
        process_id: ProcessId,
    },
    Stop {
        process_id: ProcessId,
    },
    Fail {
        process_id: ProcessId,
        reason: String,
    },
    AttachWindow {
        process_id: ProcessId,
        window_id: WindowId,
    },
    AttachRegion {
        process_id: ProcessId,
        region_id: RegionId,
    },
    AttachStackFrame {
        process_id: ProcessId,
        frame_id: StackFrameId,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum KvCommand {
    Put {
        region_id: RegionId,
        key: TritScalar,
        value: TritScalar,
    },
    PutManaged {
        region_id: RegionId,
        key: TritScalar,
        value: TritScalar,
        residency: Residency,
    },
    Get {
        region_id: RegionId,
        key: TritScalar,
    },
    GetManaged {
        region_id: RegionId,
        key: TritScalar,
    },
    Delete {
        region_id: RegionId,
        key: TritScalar,
    },
    Flush {
        region_id: RegionId,
        path: PathBuf,
    },
    FlushManaged {
        region_id: RegionId,
        path: PathBuf,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum TritMuxEvent {
    RegionCreated {
        region_id: RegionId,
    },
    RegionDuplicated {
        source: RegionId,
        duplicate: RegionId,
    },
    TabCreated {
        tab_id: TabId,
    },
    WindowOpened {
        window_id: WindowId,
        tab_id: TabId,
        region_id: RegionId,
    },
    ProcessCreated {
        process_id: ProcessId,
    },
    ProcessStateChanged {
        process_id: ProcessId,
        state: ProcessState,
    },
    ProcessAttachedWindow {
        process_id: ProcessId,
        window_id: WindowId,
    },
    ProcessAttachedRegion {
        process_id: ProcessId,
        region_id: RegionId,
    },
    ProcessAttachedStackFrame {
        process_id: ProcessId,
        frame_id: StackFrameId,
    },
    MemoryPinned {
        region_id: MemoryRegionId,
        host_state: HostPinState,
    },
    MemoryUnpinned {
        region_id: MemoryRegionId,
    },
    MemoryResized {
        region_id: MemoryRegionId,
        stryte_count: usize,
    },
    StackFramePushed {
        frame_id: StackFrameId,
    },
    StackRegionAllocated {
        frame_id: StackFrameId,
        region_id: MemoryRegionId,
    },
    StackFramePopped {
        frame_id: Option<StackFrameId>,
    },
    MemoryStats {
        stats: MemoryStats,
    },
    KvPut {
        region_id: RegionId,
    },
    KvGot {
        region_id: RegionId,
        value: Option<TritScalar>,
    },
    KvDeleted {
        region_id: RegionId,
        removed: bool,
    },
    KvFlushed {
        region_id: RegionId,
        path: PathBuf,
    },
    CommandFailed {
        error: String,
    },
}

#[derive(Debug, Default)]
pub struct CommandBus {
    next_id: u64,
    commands: VecDeque<CommandEnvelope>,
    events: VecDeque<EventEnvelope>,
}

#[derive(Debug)]
pub enum CommandError {
    Runtime(RuntimeError),
    Memory(MemoryError),
    Kv(KvError),
    WrongRegionPayload(RegionId),
    LockPoisoned(&'static str),
}

impl CommandId {
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

impl CommandSource {
    pub fn reply_target(self) -> EventTarget {
        match self {
            CommandSource::Runtime => EventTarget::Runtime,
            CommandSource::Tab(id) => EventTarget::Tab(id),
            CommandSource::Window(id) => EventTarget::Window(id),
            CommandSource::Worker(id) => EventTarget::Worker(id),
        }
    }
}

impl CommandEnvelope {
    pub fn id(&self) -> CommandId {
        self.id
    }

    pub fn source(&self) -> CommandSource {
        self.source
    }

    pub fn reply_to(&self) -> EventTarget {
        self.reply_to
    }

    pub fn command(&self) -> &TritMuxCommand {
        &self.command
    }
}

impl EventEnvelope {
    pub fn command_id(&self) -> CommandId {
        self.command_id
    }

    pub fn source(&self) -> CommandSource {
        self.source
    }

    pub fn target(&self) -> EventTarget {
        self.target
    }

    pub fn event(&self) -> &TritMuxEvent {
        &self.event
    }
}

impl CommandBus {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn submit(&mut self, source: CommandSource, command: TritMuxCommand) -> CommandId {
        self.submit_to(source, source.reply_target(), command)
    }

    pub fn submit_to(
        &mut self,
        source: CommandSource,
        reply_to: EventTarget,
        command: TritMuxCommand,
    ) -> CommandId {
        let id = self.alloc_command_id();
        self.commands.push_back(CommandEnvelope {
            id,
            source,
            reply_to,
            command,
        });
        id
    }

    pub fn pop_event(&mut self) -> Option<EventEnvelope> {
        self.events.pop_front()
    }

    pub fn drain_events(&mut self) -> Vec<EventEnvelope> {
        self.events.drain(..).collect()
    }

    pub fn pending_commands(&self) -> usize {
        self.commands.len()
    }

    pub fn pending_events(&self) -> usize {
        self.events.len()
    }

    pub fn process_one(&mut self, session: &mut RuntimeSession) -> bool {
        let Some(envelope) = self.commands.pop_front() else {
            return false;
        };

        let event = match dispatch_command(session, &envelope.command) {
            Ok(event) => event,
            Err(err) => TritMuxEvent::CommandFailed {
                error: err.to_string(),
            },
        };

        self.events.push_back(EventEnvelope {
            command_id: envelope.id,
            source: envelope.source,
            target: envelope.reply_to,
            event,
        });
        true
    }

    pub fn process_all(&mut self, session: &mut RuntimeSession) -> usize {
        let mut processed = 0;
        while self.process_one(session) {
            processed += 1;
        }
        processed
    }

    fn alloc_command_id(&mut self) -> CommandId {
        self.next_id = self.next_id.saturating_add(1);
        CommandId(self.next_id)
    }
}

fn dispatch_command(
    session: &mut RuntimeSession,
    command: &TritMuxCommand,
) -> Result<TritMuxEvent, CommandError> {
    match command {
        TritMuxCommand::Region(command) => dispatch_region_command(session, command),
        TritMuxCommand::Process(command) => dispatch_process_command(session, command),
        TritMuxCommand::Memory(command) => dispatch_memory_command(session, command),
        TritMuxCommand::Kv(command) => dispatch_kv_command(session, command),
    }
}

fn dispatch_region_command(
    session: &mut RuntimeSession,
    command: &RegionCommand,
) -> Result<TritMuxEvent, CommandError> {
    match command {
        RegionCommand::CreateKv { name } => {
            let region_id = session.create_region(name, RegionPayload::Kv(KvStore::new()));
            Ok(TritMuxEvent::RegionCreated { region_id })
        }
        RegionCommand::CreateScalar {
            name,
            value,
            residency,
        } => {
            let region_id = session
                .create_scalar_region(name, value, *residency)
                .map_err(CommandError::Runtime)?;
            Ok(TritMuxEvent::RegionCreated { region_id })
        }
        RegionCommand::CreateBytes {
            name,
            bytes,
            residency,
        } => {
            let region_id = session
                .create_bytes_region(name, bytes, *residency)
                .map_err(CommandError::Runtime)?;
            Ok(TritMuxEvent::RegionCreated { region_id })
        }
        RegionCommand::Duplicate { source, name } => {
            let duplicate = session
                .duplicate_region(*source, name)
                .map_err(CommandError::Runtime)?;
            Ok(TritMuxEvent::RegionDuplicated {
                source: *source,
                duplicate,
            })
        }
        RegionCommand::CreateTab { title } => {
            let tab_id = session.create_tab(title);
            Ok(TritMuxEvent::TabCreated { tab_id })
        }
        RegionCommand::OpenWindow {
            tab_id,
            title,
            role,
            app_name,
            region_id,
        } => {
            let window_id = session
                .open_window(*tab_id, title, *role, app_name, *region_id)
                .map_err(CommandError::Runtime)?;
            Ok(TritMuxEvent::WindowOpened {
                window_id,
                tab_id: *tab_id,
                region_id: *region_id,
            })
        }
    }
}

fn dispatch_process_command(
    session: &mut RuntimeSession,
    command: &ProcessCommand,
) -> Result<TritMuxEvent, CommandError> {
    match command {
        ProcessCommand::Create {
            name,
            kind,
            parent,
            tab_id,
        } => {
            let process_id = session
                .create_process(name, *kind, *parent, *tab_id)
                .map_err(CommandError::Runtime)?;
            Ok(TritMuxEvent::ProcessCreated { process_id })
        }
        ProcessCommand::Start { process_id } => {
            let state = session
                .start_process(*process_id)
                .map_err(CommandError::Runtime)?;
            Ok(TritMuxEvent::ProcessStateChanged {
                process_id: *process_id,
                state,
            })
        }
        ProcessCommand::Suspend { process_id } => {
            let state = session
                .suspend_process(*process_id)
                .map_err(CommandError::Runtime)?;
            Ok(TritMuxEvent::ProcessStateChanged {
                process_id: *process_id,
                state,
            })
        }
        ProcessCommand::Resume { process_id } => {
            let state = session
                .resume_process(*process_id)
                .map_err(CommandError::Runtime)?;
            Ok(TritMuxEvent::ProcessStateChanged {
                process_id: *process_id,
                state,
            })
        }
        ProcessCommand::Stop { process_id } => {
            let state = session
                .stop_process(*process_id)
                .map_err(CommandError::Runtime)?;
            Ok(TritMuxEvent::ProcessStateChanged {
                process_id: *process_id,
                state,
            })
        }
        ProcessCommand::Fail { process_id, reason } => {
            let state = session
                .fail_process(*process_id, reason)
                .map_err(CommandError::Runtime)?;
            Ok(TritMuxEvent::ProcessStateChanged {
                process_id: *process_id,
                state,
            })
        }
        ProcessCommand::AttachWindow {
            process_id,
            window_id,
        } => {
            session
                .attach_window_to_process(*process_id, *window_id)
                .map_err(CommandError::Runtime)?;
            Ok(TritMuxEvent::ProcessAttachedWindow {
                process_id: *process_id,
                window_id: *window_id,
            })
        }
        ProcessCommand::AttachRegion {
            process_id,
            region_id,
        } => {
            session
                .attach_region_to_process(*process_id, *region_id)
                .map_err(CommandError::Runtime)?;
            Ok(TritMuxEvent::ProcessAttachedRegion {
                process_id: *process_id,
                region_id: *region_id,
            })
        }
        ProcessCommand::AttachStackFrame {
            process_id,
            frame_id,
        } => {
            session
                .attach_stack_frame_to_process(*process_id, *frame_id)
                .map_err(CommandError::Runtime)?;
            Ok(TritMuxEvent::ProcessAttachedStackFrame {
                process_id: *process_id,
                frame_id: *frame_id,
            })
        }
    }
}

fn dispatch_memory_command(
    session: &mut RuntimeSession,
    command: &MemoryCommand,
) -> Result<TritMuxEvent, CommandError> {
    match command {
        MemoryCommand::Pin { region_id } => {
            let host_state = session
                .mutate_memory(|memory| memory.pin_region(*region_id))
                .map_err(CommandError::Runtime)?
                .map_err(CommandError::Memory)?;
            Ok(TritMuxEvent::MemoryPinned {
                region_id: *region_id,
                host_state,
            })
        }
        MemoryCommand::Unpin { region_id } => {
            session
                .mutate_memory(|memory| memory.unpin_region(*region_id))
                .map_err(CommandError::Runtime)?
                .map_err(CommandError::Memory)?;
            Ok(TritMuxEvent::MemoryUnpinned {
                region_id: *region_id,
            })
        }
        MemoryCommand::Resize {
            region_id,
            stryte_count,
        } => {
            session
                .mutate_memory(|memory| memory.resize_region(*region_id, *stryte_count))
                .map_err(CommandError::Runtime)?
                .map_err(CommandError::Memory)?;
            Ok(TritMuxEvent::MemoryResized {
                region_id: *region_id,
                stryte_count: *stryte_count,
            })
        }
        MemoryCommand::PushStackFrame { name } => {
            let frame_id = session
                .mutate_memory(|memory| memory.push_stack_frame(name))
                .map_err(CommandError::Runtime)?;
            Ok(TritMuxEvent::StackFramePushed { frame_id })
        }
        MemoryCommand::AllocateStack {
            frame_id,
            name,
            stryte_count,
        } => {
            let region_id = session
                .mutate_memory(|memory| memory.allocate_stack(*frame_id, name, *stryte_count))
                .map_err(CommandError::Runtime)?
                .map_err(CommandError::Memory)?;
            Ok(TritMuxEvent::StackRegionAllocated {
                frame_id: *frame_id,
                region_id,
            })
        }
        MemoryCommand::PopStackFrame => {
            let frame_id = session
                .mutate_memory(|memory| memory.pop_stack_frame())
                .map_err(CommandError::Runtime)?
                .map_err(CommandError::Memory)?
                .map(|frame| frame.id());
            Ok(TritMuxEvent::StackFramePopped { frame_id })
        }
        MemoryCommand::Stats => {
            let stats = session
                .with_memory(|memory| memory.stats())
                .map_err(CommandError::Runtime)?;
            Ok(TritMuxEvent::MemoryStats { stats })
        }
    }
}

fn dispatch_kv_command(
    session: &mut RuntimeSession,
    command: &KvCommand,
) -> Result<TritMuxEvent, CommandError> {
    match command {
        KvCommand::Put {
            region_id,
            key,
            value,
        } => {
            with_kv_store_mut(session, *region_id, |store| {
                store.put(key.clone(), value.clone())
            })?;
            Ok(TritMuxEvent::KvPut {
                region_id: *region_id,
            })
        }
        KvCommand::PutManaged {
            region_id,
            key,
            value,
            residency,
        } => {
            with_kv_store_and_memory_mut(session, *region_id, |store, memory| {
                store
                    .put_managed(key.clone(), value.clone(), memory, *residency)
                    .map(|_| ())
            })?;
            Ok(TritMuxEvent::KvPut {
                region_id: *region_id,
            })
        }
        KvCommand::Get { region_id, key } => {
            let value = with_kv_store_mut(session, *region_id, |store| store.get(key))?;
            Ok(TritMuxEvent::KvGot {
                region_id: *region_id,
                value,
            })
        }
        KvCommand::GetManaged { region_id, key } => {
            let value = with_kv_store_and_memory_mut(session, *region_id, |store, memory| {
                store.get_managed(key, memory)
            })?;
            Ok(TritMuxEvent::KvGot {
                region_id: *region_id,
                value,
            })
        }
        KvCommand::Delete { region_id, key } => {
            let removed = with_kv_store_mut(session, *region_id, |store| store.delete(key))?;
            Ok(TritMuxEvent::KvDeleted {
                region_id: *region_id,
                removed,
            })
        }
        KvCommand::Flush { region_id, path } => {
            with_kv_store_mut(session, *region_id, |store| store.flush(path))?;
            Ok(TritMuxEvent::KvFlushed {
                region_id: *region_id,
                path: path.clone(),
            })
        }
        KvCommand::FlushManaged { region_id, path } => {
            with_kv_store_and_memory_mut(session, *region_id, |store, memory| {
                store.flush_managed(path, memory)
            })?;
            Ok(TritMuxEvent::KvFlushed {
                region_id: *region_id,
                path: path.clone(),
            })
        }
    }
}

fn with_kv_store_mut<T>(
    session: &RuntimeSession,
    region_id: RegionId,
    f: impl FnOnce(&mut KvStore) -> Result<T, KvError>,
) -> Result<T, CommandError> {
    session
        .mutate_region(region_id, |region| match region.payload_mut() {
            RegionPayload::Kv(store) => f(store),
            _ => Err(KvError::MemoryManagerRequired),
        })
        .map_err(CommandError::Runtime)?
        .map_err(|err| match err {
            KvError::MemoryManagerRequired => CommandError::WrongRegionPayload(region_id),
            other => CommandError::Kv(other),
        })
}

fn with_kv_store_and_memory_mut<T>(
    session: &RuntimeSession,
    region_id: RegionId,
    f: impl FnOnce(&mut KvStore, &mut crate::memory::TrinaryMemoryManager) -> Result<T, KvError>,
) -> Result<T, CommandError> {
    let memory_handle = session.memory_handle();
    session
        .mutate_region(region_id, |region| {
            let mut memory = memory_handle
                .write()
                .map_err(|_| KvError::MemoryManagerRequired)?;
            match region.payload_mut() {
                RegionPayload::Kv(store) => f(store, &mut memory),
                _ => Err(KvError::MemoryManagerRequired),
            }
        })
        .map_err(CommandError::Runtime)?
        .map_err(|err| match err {
            KvError::MemoryManagerRequired => CommandError::WrongRegionPayload(region_id),
            other => CommandError::Kv(other),
        })
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CommandError::Runtime(err) => write!(f, "{err}"),
            CommandError::Memory(err) => write!(f, "{err}"),
            CommandError::Kv(err) => write!(f, "{err}"),
            CommandError::WrongRegionPayload(region_id) => {
                write!(f, "region {} does not contain a KV store", region_id.as_u64())
            }
            CommandError::LockPoisoned(name) => write!(f, "{name} lock is poisoned"),
        }
    }
}

impl Error for CommandError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::Residency;

    fn pop_event(bus: &mut CommandBus) -> TritMuxEvent {
        bus.pop_event().expect("event should be queued").event
    }

    #[test]
    fn command_bus_routes_region_tab_and_window_events() {
        let mut session = RuntimeSession::new();
        let mut bus = CommandBus::new();

        bus.submit(
            CommandSource::Runtime,
            TritMuxCommand::Region(RegionCommand::CreateKv {
                name: "kv".to_string(),
            }),
        );
        bus.process_one(&mut session);
        let TritMuxEvent::RegionCreated { region_id } = pop_event(&mut bus) else {
            panic!("expected region created event");
        };

        bus.submit(
            CommandSource::Runtime,
            TritMuxCommand::Region(RegionCommand::CreateTab {
                title: "main".to_string(),
            }),
        );
        bus.process_one(&mut session);
        let TritMuxEvent::TabCreated { tab_id } = pop_event(&mut bus) else {
            panic!("expected tab created event");
        };

        let command_id = bus.submit_to(
            CommandSource::Runtime,
            EventTarget::Runtime,
            TritMuxCommand::Region(RegionCommand::OpenWindow {
                tab_id,
                title: "kv".to_string(),
                role: WindowRole::Application,
                app_name: "kv".to_string(),
                region_id,
            }),
        );
        bus.process_one(&mut session);
        let event = bus.pop_event().unwrap();
        assert_eq!(event.command_id(), command_id);
        assert_eq!(event.source(), CommandSource::Runtime);
        assert_eq!(event.target(), EventTarget::Runtime);
        assert!(matches!(
            event.event(),
            TritMuxEvent::WindowOpened {
                tab_id: opened_tab,
                region_id: opened_region,
                ..
            } if *opened_tab == tab_id && *opened_region == region_id
        ));
    }

    #[test]
    fn process_commands_attach_runtime_state() {
        let mut session = RuntimeSession::new();
        let mut bus = CommandBus::new();
        let region_id = session.create_region("kv", RegionPayload::Kv(KvStore::new()));
        let tab_id = session.create_tab("workspace");
        let window_id = session
            .open_window(
                tab_id,
                "editor",
                WindowRole::Application,
                "notes",
                region_id,
            )
            .unwrap();
        let frame_id = session
            .mutate_memory(|memory| memory.push_stack_frame("notes app"))
            .unwrap();

        bus.submit(
            CommandSource::Window(window_id),
            TritMuxCommand::Process(ProcessCommand::Create {
                name: "notes".to_string(),
                kind: ProcessKind::Application,
                parent: None,
                tab_id: Some(tab_id),
            }),
        );
        bus.process_one(&mut session);
        let TritMuxEvent::ProcessCreated { process_id } = pop_event(&mut bus) else {
            panic!("expected process created event");
        };

        bus.submit(
            CommandSource::Window(window_id),
            TritMuxCommand::Process(ProcessCommand::AttachWindow {
                process_id,
                window_id,
            }),
        );
        bus.submit(
            CommandSource::Window(window_id),
            TritMuxCommand::Process(ProcessCommand::AttachRegion {
                process_id,
                region_id,
            }),
        );
        bus.submit(
            CommandSource::Window(window_id),
            TritMuxCommand::Process(ProcessCommand::AttachStackFrame {
                process_id,
                frame_id,
            }),
        );
        bus.submit(
            CommandSource::Window(window_id),
            TritMuxCommand::Process(ProcessCommand::Start { process_id }),
        );
        assert_eq!(bus.process_all(&mut session), 4);

        let events = bus.drain_events();
        assert!(matches!(
            events[0].event(),
            TritMuxEvent::ProcessAttachedWindow { .. }
        ));
        assert!(matches!(
            events[1].event(),
            TritMuxEvent::ProcessAttachedRegion { .. }
        ));
        assert!(matches!(
            events[2].event(),
            TritMuxEvent::ProcessAttachedStackFrame { .. }
        ));
        assert_eq!(
            events[3].event(),
            &TritMuxEvent::ProcessStateChanged {
                process_id,
                state: ProcessState::Running,
            }
        );

        let descriptor = session.process(process_id).unwrap();
        assert_eq!(descriptor.windows(), &[window_id]);
        assert_eq!(descriptor.regions(), &[region_id]);
        assert_eq!(descriptor.stack_frame(), Some(frame_id));
        assert_eq!(session.window(window_id).unwrap().process_id(), Some(process_id));
    }

    #[test]
    fn kv_commands_can_promote_values_into_managed_memory() {
        let mut session = RuntimeSession::new();
        let mut bus = CommandBus::new();
        let region_id = session.create_region("kv", RegionPayload::Kv(KvStore::new()));

        bus.submit(
            CommandSource::Runtime,
            TritMuxCommand::Kv(KvCommand::PutManaged {
                region_id,
                key: TritScalar::Text("key".to_string()),
                value: TritScalar::Text("value".to_string()),
                residency: Residency::Hot,
            }),
        );
        bus.submit(
            CommandSource::Runtime,
            TritMuxCommand::Kv(KvCommand::GetManaged {
                region_id,
                key: TritScalar::Text("key".to_string()),
            }),
        );
        assert_eq!(bus.process_all(&mut session), 2);

        let events = bus.drain_events();
        assert!(matches!(events[0].event(), TritMuxEvent::KvPut { .. }));
        assert_eq!(
            events[1].event(),
            &TritMuxEvent::KvGot {
                region_id,
                value: Some(TritScalar::Text("value".to_string())),
            }
        );
        assert_eq!(session.with_memory(|memory| memory.stats().heap_regions).unwrap(), 1);
    }

    #[test]
    fn memory_commands_emit_failures_for_protected_regions() {
        let mut session = RuntimeSession::new();
        let mut bus = CommandBus::new();
        let region_id = session
            .mutate_memory(|memory| memory.allocate_heap("critical", 2, Residency::Hot))
            .unwrap();

        bus.submit(
            CommandSource::Runtime,
            TritMuxCommand::Memory(MemoryCommand::Pin { region_id }),
        );
        bus.submit(
            CommandSource::Runtime,
            TritMuxCommand::Memory(MemoryCommand::Resize {
                region_id,
                stryte_count: 4,
            }),
        );
        assert_eq!(bus.process_all(&mut session), 2);

        let first = bus.pop_event().unwrap();
        assert!(matches!(first.event(), TritMuxEvent::MemoryPinned { .. }));
        let second = bus.pop_event().unwrap();
        assert!(matches!(second.event(), TritMuxEvent::CommandFailed { .. }));
    }
}
