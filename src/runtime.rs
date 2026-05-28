use crate::kv::KvStore;
use crate::memory::{
    MemoryError, MemoryRegionId, Residency, StackFrameId, TrinaryMemoryManager,
};
use crate::trit::payload::TritPayload;
use crate::trit::scalar::{ScalarError, TritScalar};
use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RegionId(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TabId(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WindowId(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProcessId(u64);

#[derive(Debug, Clone)]
pub enum RegionPayload {
    Empty,
    Kv(KvStore),
    Memory(MemoryRegionId),
    Scalar(TritScalar),
    Bytes(Vec<u8>),
}

#[derive(Debug, Clone)]
pub struct StateRegion {
    id: RegionId,
    name: String,
    payload: RegionPayload,
    revision: u64,
    source: Option<RegionId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowRole {
    Application,
    SubApplication,
    Process,
    Inspector,
    Editor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tab {
    id: TabId,
    title: String,
    windows: Vec<WindowId>,
    active_window: Option<WindowId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    id: WindowId,
    tab_id: TabId,
    title: String,
    role: WindowRole,
    app_name: String,
    region_id: RegionId,
    process_id: Option<ProcessId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessKind {
    Application,
    SubApplication,
    BackgroundWorker,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessState {
    Created,
    Running,
    Suspended,
    Stopped,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessDescriptor {
    id: ProcessId,
    name: String,
    kind: ProcessKind,
    parent: Option<ProcessId>,
    tab_id: Option<TabId>,
    windows: Vec<WindowId>,
    regions: Vec<RegionId>,
    stack_frame: Option<StackFrameId>,
    state: ProcessState,
}

#[derive(Debug, Default)]
pub struct RuntimeSession {
    next_region: u64,
    next_tab: u64,
    next_window: u64,
    next_process: u64,
    memory: Arc<RwLock<TrinaryMemoryManager>>,
    regions: HashMap<RegionId, Arc<RwLock<StateRegion>>>,
    tabs: HashMap<TabId, Tab>,
    windows: HashMap<WindowId, Window>,
    processes: HashMap<ProcessId, ProcessDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeCommand {
    FocusTab(TabId),
    FocusWindow(WindowId),
    DuplicateRegion {
        source: RegionId,
        name: String,
    },
    OpenWindow {
        tab_id: TabId,
        title: String,
        role: WindowRole,
        app_name: String,
        region_id: RegionId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeEvent {
    RegionCreated(RegionId),
    RegionDuplicated {
        source: RegionId,
        duplicate: RegionId,
    },
    RegionMutated {
        id: RegionId,
        revision: u64,
    },
    TabCreated(TabId),
    WindowOpened(WindowId),
}

#[derive(Debug)]
pub enum RuntimeError {
    RegionNotFound(RegionId),
    TabNotFound(TabId),
    WindowNotFound(WindowId),
    ProcessNotFound(ProcessId),
    Memory(MemoryError),
    Scalar(ScalarError),
    LockPoisoned(&'static str),
}

impl RegionId {
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

impl TabId {
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

impl WindowId {
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

impl ProcessId {
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

impl StateRegion {
    fn new(id: RegionId, name: impl Into<String>, payload: RegionPayload) -> Self {
        Self {
            id,
            name: name.into(),
            payload,
            revision: 0,
            source: None,
        }
    }

    fn duplicate_with_payload(
        &self,
        id: RegionId,
        name: impl Into<String>,
        payload: RegionPayload,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            payload,
            revision: self.revision,
            source: Some(self.id),
        }
    }

    pub fn id(&self) -> RegionId {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn payload(&self) -> &RegionPayload {
        &self.payload
    }

    pub fn payload_mut(&mut self) -> &mut RegionPayload {
        &mut self.payload
    }

    pub fn memory_region_id(&self) -> Option<MemoryRegionId> {
        match self.payload {
            RegionPayload::Memory(id) => Some(id),
            _ => None,
        }
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn source(&self) -> Option<RegionId> {
        self.source
    }
}

impl Tab {
    pub fn id(&self) -> TabId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn windows(&self) -> &[WindowId] {
        &self.windows
    }

    pub fn active_window(&self) -> Option<WindowId> {
        self.active_window
    }
}

impl Window {
    pub fn id(&self) -> WindowId {
        self.id
    }

    pub fn tab_id(&self) -> TabId {
        self.tab_id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn role(&self) -> WindowRole {
        self.role
    }

    pub fn app_name(&self) -> &str {
        &self.app_name
    }

    pub fn region_id(&self) -> RegionId {
        self.region_id
    }

    pub fn process_id(&self) -> Option<ProcessId> {
        self.process_id
    }
}

impl ProcessDescriptor {
    fn new(
        id: ProcessId,
        name: impl Into<String>,
        kind: ProcessKind,
        parent: Option<ProcessId>,
        tab_id: Option<TabId>,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            kind,
            parent,
            tab_id,
            windows: Vec::new(),
            regions: Vec::new(),
            stack_frame: None,
            state: ProcessState::Created,
        }
    }

    pub fn id(&self) -> ProcessId {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn kind(&self) -> ProcessKind {
        self.kind
    }

    pub fn parent(&self) -> Option<ProcessId> {
        self.parent
    }

    pub fn tab_id(&self) -> Option<TabId> {
        self.tab_id
    }

    pub fn windows(&self) -> &[WindowId] {
        &self.windows
    }

    pub fn regions(&self) -> &[RegionId] {
        &self.regions
    }

    pub fn stack_frame(&self) -> Option<StackFrameId> {
        self.stack_frame
    }

    pub fn state(&self) -> &ProcessState {
        &self.state
    }
}

impl RuntimeSession {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create_region(&mut self, name: impl Into<String>, payload: RegionPayload) -> RegionId {
        let id = self.alloc_region_id();
        self.regions
            .insert(id, Arc::new(RwLock::new(StateRegion::new(id, name, payload))));
        id
    }

    pub fn create_payload_region(
        &mut self,
        name: impl Into<String>,
        payload: &TritPayload,
        residency: Residency,
    ) -> Result<RegionId, RuntimeError> {
        let name = name.into();
        let memory_region = self
            .memory
            .write()
            .map_err(|_| RuntimeError::LockPoisoned("memory write"))?
            .allocate_heap_payload(format!("{name} memory"), payload, residency);
        Ok(self.create_region(name, RegionPayload::Memory(memory_region)))
    }

    pub fn create_scalar_region(
        &mut self,
        name: impl Into<String>,
        value: &TritScalar,
        residency: Residency,
    ) -> Result<RegionId, RuntimeError> {
        let payload = value
            .to_payload()
            .map_err(RuntimeError::Scalar)?;
        self.create_payload_region(name, &payload, residency)
    }

    pub fn create_bytes_region(
        &mut self,
        name: impl Into<String>,
        bytes: &[u8],
        residency: Residency,
    ) -> Result<RegionId, RuntimeError> {
        self.create_payload_region(name, &TritPayload::from_bytes(bytes), residency)
    }

    pub fn duplicate_region(
        &mut self,
        source: RegionId,
        name: impl Into<String>,
    ) -> Result<RegionId, RuntimeError> {
        let source_region = self.region_handle(source)?;
        let snapshot = source_region
            .read()
            .map_err(|_| RuntimeError::LockPoisoned("region read"))?;
        let id = self.alloc_region_id();
        let name = name.into();
        let duplicate_payload = match snapshot.payload() {
            RegionPayload::Memory(memory_id) => {
                let mut memory = self
                    .memory
                    .write()
                    .map_err(|_| RuntimeError::LockPoisoned("memory write"))?;
                let duplicate_memory = memory
                    .duplicate_as_heap(*memory_id, format!("{name} memory"))
                    .map_err(RuntimeError::Memory)?;
                RegionPayload::Memory(duplicate_memory)
            }
            other => other.clone(),
        };
        let duplicate = snapshot.duplicate_with_payload(id, name, duplicate_payload);
        drop(snapshot);

        self.regions.insert(id, Arc::new(RwLock::new(duplicate)));
        Ok(id)
    }

    pub fn create_tab(&mut self, title: impl Into<String>) -> TabId {
        let id = self.alloc_tab_id();
        self.tabs.insert(
            id,
            Tab {
                id,
                title: title.into(),
                windows: Vec::new(),
                active_window: None,
            },
        );
        id
    }

    pub fn open_window(
        &mut self,
        tab_id: TabId,
        title: impl Into<String>,
        role: WindowRole,
        app_name: impl Into<String>,
        region_id: RegionId,
    ) -> Result<WindowId, RuntimeError> {
        if !self.regions.contains_key(&region_id) {
            return Err(RuntimeError::RegionNotFound(region_id));
        }

        let id = self.alloc_window_id();
        let tab = self
            .tabs
            .get_mut(&tab_id)
            .ok_or(RuntimeError::TabNotFound(tab_id))?;
        tab.windows.push(id);
        tab.active_window = Some(id);

        self.windows.insert(
            id,
            Window {
                id,
                tab_id,
                title: title.into(),
                role,
                app_name: app_name.into(),
                region_id,
                process_id: None,
            },
        );
        Ok(id)
    }

    pub fn create_process(
        &mut self,
        name: impl Into<String>,
        kind: ProcessKind,
        parent: Option<ProcessId>,
        tab_id: Option<TabId>,
    ) -> Result<ProcessId, RuntimeError> {
        if let Some(parent_id) = parent {
            if !self.processes.contains_key(&parent_id) {
                return Err(RuntimeError::ProcessNotFound(parent_id));
            }
        }
        if let Some(tab_id) = tab_id {
            if !self.tabs.contains_key(&tab_id) {
                return Err(RuntimeError::TabNotFound(tab_id));
            }
        }

        let id = self.alloc_process_id();
        self.processes.insert(
            id,
            ProcessDescriptor::new(id, name, kind, parent, tab_id),
        );
        Ok(id)
    }

    pub fn start_process(&mut self, id: ProcessId) -> Result<ProcessState, RuntimeError> {
        self.set_process_state(id, ProcessState::Running)
    }

    pub fn suspend_process(&mut self, id: ProcessId) -> Result<ProcessState, RuntimeError> {
        self.set_process_state(id, ProcessState::Suspended)
    }

    pub fn resume_process(&mut self, id: ProcessId) -> Result<ProcessState, RuntimeError> {
        self.set_process_state(id, ProcessState::Running)
    }

    pub fn stop_process(&mut self, id: ProcessId) -> Result<ProcessState, RuntimeError> {
        self.set_process_state(id, ProcessState::Stopped)
    }

    pub fn fail_process(
        &mut self,
        id: ProcessId,
        reason: impl Into<String>,
    ) -> Result<ProcessState, RuntimeError> {
        self.set_process_state(id, ProcessState::Failed(reason.into()))
    }

    pub fn attach_window_to_process(
        &mut self,
        process_id: ProcessId,
        window_id: WindowId,
    ) -> Result<(), RuntimeError> {
        if !self.processes.contains_key(&process_id) {
            return Err(RuntimeError::ProcessNotFound(process_id));
        }

        let previous_process = self
            .windows
            .get(&window_id)
            .ok_or(RuntimeError::WindowNotFound(window_id))?
            .process_id;
        if let Some(previous_id) = previous_process {
            if previous_id != process_id {
                if let Some(previous) = self.processes.get_mut(&previous_id) {
                    previous.windows.retain(|id| *id != window_id);
                }
            }
        }

        let process = self
            .processes
            .get_mut(&process_id)
            .ok_or(RuntimeError::ProcessNotFound(process_id))?;
        if !process.windows.contains(&window_id) {
            process.windows.push(window_id);
        }

        self.windows
            .get_mut(&window_id)
            .ok_or(RuntimeError::WindowNotFound(window_id))?
            .process_id = Some(process_id);
        Ok(())
    }

    pub fn attach_region_to_process(
        &mut self,
        process_id: ProcessId,
        region_id: RegionId,
    ) -> Result<(), RuntimeError> {
        if !self.regions.contains_key(&region_id) {
            return Err(RuntimeError::RegionNotFound(region_id));
        }

        let process = self
            .processes
            .get_mut(&process_id)
            .ok_or(RuntimeError::ProcessNotFound(process_id))?;
        if !process.regions.contains(&region_id) {
            process.regions.push(region_id);
        }
        Ok(())
    }

    pub fn attach_stack_frame_to_process(
        &mut self,
        process_id: ProcessId,
        frame_id: StackFrameId,
    ) -> Result<(), RuntimeError> {
        let frame_exists = self.with_memory(|memory| {
            memory
                .stack_frames()
                .iter()
                .any(|frame| frame.id() == frame_id)
        })?;
        if !frame_exists {
            return Err(RuntimeError::Memory(MemoryError::StackFrameNotFound(
                frame_id,
            )));
        }

        let process = self
            .processes
            .get_mut(&process_id)
            .ok_or(RuntimeError::ProcessNotFound(process_id))?;
        process.stack_frame = Some(frame_id);
        Ok(())
    }

    pub fn duplicate_window_region(
        &mut self,
        window_id: WindowId,
        new_region_name: impl Into<String>,
    ) -> Result<RegionId, RuntimeError> {
        let window = self
            .windows
            .get(&window_id)
            .ok_or(RuntimeError::WindowNotFound(window_id))?;
        self.duplicate_region(window.region_id, new_region_name)
    }

    pub fn region_handle(
        &self,
        id: RegionId,
    ) -> Result<Arc<RwLock<StateRegion>>, RuntimeError> {
        self.regions
            .get(&id)
            .cloned()
            .ok_or(RuntimeError::RegionNotFound(id))
    }

    pub fn memory_handle(&self) -> Arc<RwLock<TrinaryMemoryManager>> {
        Arc::clone(&self.memory)
    }

    pub fn with_memory<T>(
        &self,
        f: impl FnOnce(&TrinaryMemoryManager) -> T,
    ) -> Result<T, RuntimeError> {
        let memory = self
            .memory
            .read()
            .map_err(|_| RuntimeError::LockPoisoned("memory read"))?;
        Ok(f(&memory))
    }

    pub fn mutate_memory<T>(
        &self,
        f: impl FnOnce(&mut TrinaryMemoryManager) -> T,
    ) -> Result<T, RuntimeError> {
        let mut memory = self
            .memory
            .write()
            .map_err(|_| RuntimeError::LockPoisoned("memory write"))?;
        Ok(f(&mut memory))
    }

    pub fn with_region<T>(
        &self,
        id: RegionId,
        f: impl FnOnce(&StateRegion) -> T,
    ) -> Result<T, RuntimeError> {
        let region = self.region_handle(id)?;
        let guard = region
            .read()
            .map_err(|_| RuntimeError::LockPoisoned("region read"))?;
        Ok(f(&guard))
    }

    pub fn mutate_region<T>(
        &self,
        id: RegionId,
        f: impl FnOnce(&mut StateRegion) -> T,
    ) -> Result<T, RuntimeError> {
        let region = self.region_handle(id)?;
        let mut guard = region
            .write()
            .map_err(|_| RuntimeError::LockPoisoned("region write"))?;
        let result = f(&mut guard);
        guard.revision = guard.revision.saturating_add(1);
        Ok(result)
    }

    pub fn tab(&self, id: TabId) -> Result<&Tab, RuntimeError> {
        self.tabs.get(&id).ok_or(RuntimeError::TabNotFound(id))
    }

    pub fn window(&self, id: WindowId) -> Result<&Window, RuntimeError> {
        self.windows
            .get(&id)
            .ok_or(RuntimeError::WindowNotFound(id))
    }

    pub fn process(&self, id: ProcessId) -> Result<&ProcessDescriptor, RuntimeError> {
        self.processes
            .get(&id)
            .ok_or(RuntimeError::ProcessNotFound(id))
    }

    pub fn tabs(&self) -> impl Iterator<Item = &Tab> {
        self.tabs.values()
    }

    pub fn windows(&self) -> impl Iterator<Item = &Window> {
        self.windows.values()
    }

    pub fn processes(&self) -> impl Iterator<Item = &ProcessDescriptor> {
        self.processes.values()
    }

    pub fn region_count(&self) -> usize {
        self.regions.len()
    }

    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    pub fn window_count(&self) -> usize {
        self.windows.len()
    }

    pub fn process_count(&self) -> usize {
        self.processes.len()
    }

    fn alloc_region_id(&mut self) -> RegionId {
        self.next_region = self.next_region.saturating_add(1);
        RegionId(self.next_region)
    }

    fn alloc_tab_id(&mut self) -> TabId {
        self.next_tab = self.next_tab.saturating_add(1);
        TabId(self.next_tab)
    }

    fn alloc_window_id(&mut self) -> WindowId {
        self.next_window = self.next_window.saturating_add(1);
        WindowId(self.next_window)
    }

    fn alloc_process_id(&mut self) -> ProcessId {
        self.next_process = self.next_process.saturating_add(1);
        ProcessId(self.next_process)
    }

    fn set_process_state(
        &mut self,
        id: ProcessId,
        state: ProcessState,
    ) -> Result<ProcessState, RuntimeError> {
        let process = self
            .processes
            .get_mut(&id)
            .ok_or(RuntimeError::ProcessNotFound(id))?;
        process.state = state.clone();
        Ok(state)
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuntimeError::RegionNotFound(id) => write!(f, "region {} was not found", id.as_u64()),
            RuntimeError::TabNotFound(id) => write!(f, "tab {} was not found", id.as_u64()),
            RuntimeError::WindowNotFound(id) => write!(f, "window {} was not found", id.as_u64()),
            RuntimeError::ProcessNotFound(id) => {
                write!(f, "process {} was not found", id.as_u64())
            }
            RuntimeError::Memory(err) => write!(f, "{err}"),
            RuntimeError::Scalar(err) => write!(f, "{err}"),
            RuntimeError::LockPoisoned(name) => write!(f, "{name} lock is poisoned"),
        }
    }
}

impl Error for RuntimeError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trit::scalar::TritScalar;

    #[test]
    fn tabs_and_windows_attach_to_regions() {
        let mut session = RuntimeSession::new();
        let region = session.create_region("main kv", RegionPayload::Kv(KvStore::new()));
        let tab = session.create_tab("notes");
        let window = session
            .open_window(
                tab,
                "editor",
                WindowRole::Application,
                "notes",
                region,
            )
            .unwrap();

        assert_eq!(session.tab(tab).unwrap().active_window(), Some(window));
        assert_eq!(session.window(window).unwrap().region_id(), region);
        assert_eq!(session.region_count(), 1);
        assert_eq!(session.tab_count(), 1);
        assert_eq!(session.window_count(), 1);
    }

    #[test]
    fn duplicated_regions_are_independent_snapshots() {
        let mut session = RuntimeSession::new();
        let region = session.create_region("kv", RegionPayload::Kv(KvStore::new()));
        session
            .mutate_region(region, |region| {
                if let RegionPayload::Kv(store) = region.payload_mut() {
                    store
                        .put(
                            TritScalar::Text("key".to_string()),
                            TritScalar::Text("original".to_string()),
                        )
                        .unwrap();
                }
            })
            .unwrap();

        let duplicate = session.duplicate_region(region, "kv duplicate").unwrap();
        session
            .mutate_region(duplicate, |region| {
                if let RegionPayload::Kv(store) = region.payload_mut() {
                    store
                        .put(
                            TritScalar::Text("key".to_string()),
                            TritScalar::Text("copy".to_string()),
                        )
                        .unwrap();
                }
            })
            .unwrap();

        let original_value = session
            .mutate_region(region, |region| match region.payload_mut() {
                RegionPayload::Kv(store) => store
                    .get(&TritScalar::Text("key".to_string()))
                    .unwrap()
                    .unwrap(),
                _ => panic!("expected kv region"),
            })
            .unwrap();
        let copied_value = session
            .mutate_region(duplicate, |region| match region.payload_mut() {
                RegionPayload::Kv(store) => store
                    .get(&TritScalar::Text("key".to_string()))
                    .unwrap()
                    .unwrap(),
                _ => panic!("expected kv region"),
            })
            .unwrap();

        assert_eq!(original_value, TritScalar::Text("original".to_string()));
        assert_eq!(copied_value, TritScalar::Text("copy".to_string()));
        assert_eq!(
            session.with_region(duplicate, StateRegion::source).unwrap(),
            Some(region)
        );
    }

    #[test]
    fn shared_region_windows_see_same_state() {
        let mut session = RuntimeSession::new();
        let region = session.create_region("shared", RegionPayload::Kv(KvStore::new()));
        let tab = session.create_tab("workspace");
        let first = session
            .open_window(tab, "left", WindowRole::Editor, "editor", region)
            .unwrap();
        let second = session
            .open_window(tab, "right", WindowRole::Inspector, "inspector", region)
            .unwrap();

        assert_eq!(session.window(first).unwrap().region_id(), region);
        assert_eq!(session.window(second).unwrap().region_id(), region);

        session
            .mutate_region(region, |region| {
                if let RegionPayload::Kv(store) = region.payload_mut() {
                    store.put(TritScalar::U64(7), TritScalar::Bool(true)).unwrap();
                }
            })
            .unwrap();

        let value = session
            .mutate_region(region, |region| match region.payload_mut() {
                RegionPayload::Kv(store) => store.get(&TritScalar::U64(7)).unwrap(),
                _ => None,
            })
            .unwrap();

        assert_eq!(value, Some(TritScalar::Bool(true)));
        assert_eq!(session.tab(tab).unwrap().windows(), &[first, second]);
    }

    #[test]
    fn scalar_regions_allocate_managed_trinary_memory() {
        let mut session = RuntimeSession::new();
        let region = session
            .create_scalar_region(
                "scalar",
                &TritScalar::Text("managed region".to_string()),
                Residency::Hot,
            )
            .unwrap();

        let memory_region = session
            .with_region(region, StateRegion::memory_region_id)
            .unwrap()
            .unwrap();
        let scalar = session
            .with_memory(|memory| {
                let payload = memory.region(memory_region).unwrap().to_payload().unwrap();
                TritScalar::from_payload(&payload).unwrap()
            })
            .unwrap();

        assert_eq!(scalar, TritScalar::Text("managed region".to_string()));
    }

    #[test]
    fn duplicating_managed_regions_forks_underlying_memory() {
        let mut session = RuntimeSession::new();
        let original = session
            .create_bytes_region("bytes", &[1, 2, 3, 4], Residency::Hot)
            .unwrap();
        let duplicate = session.duplicate_region(original, "bytes copy").unwrap();

        let original_memory = session
            .with_region(original, StateRegion::memory_region_id)
            .unwrap()
            .unwrap();
        let duplicate_memory = session
            .with_region(duplicate, StateRegion::memory_region_id)
            .unwrap()
            .unwrap();

        assert_ne!(original_memory, duplicate_memory);

        session
            .mutate_memory(|memory| {
                memory.region_mut(duplicate_memory).unwrap().strytes_mut()[0] =
                    crate::trit::tryte::STryte::zero();
            })
            .unwrap();

        let original_payload = session
            .with_memory(|memory| memory.region(original_memory).unwrap().to_payload().unwrap())
            .unwrap();
        let duplicate_payload = session
            .with_memory(|memory| memory.region(duplicate_memory).unwrap().to_payload().unwrap())
            .unwrap();

        assert_ne!(original_payload.as_storage_digits(), duplicate_payload.as_storage_digits());
        assert_eq!(
            session.with_region(duplicate, StateRegion::source).unwrap(),
            Some(original)
        );
    }

    #[test]
    fn processes_attach_windows_regions_and_stack_frames() {
        let mut session = RuntimeSession::new();
        let region = session.create_region("notes kv", RegionPayload::Kv(KvStore::new()));
        let tab = session.create_tab("notes");
        let window = session
            .open_window(
                tab,
                "editor",
                WindowRole::Application,
                "notes",
                region,
            )
            .unwrap();
        let frame = session
            .mutate_memory(|memory| memory.push_stack_frame("notes process"))
            .unwrap();

        let process = session
            .create_process(
                "notes",
                ProcessKind::Application,
                None,
                Some(tab),
            )
            .unwrap();
        session.attach_window_to_process(process, window).unwrap();
        session.attach_region_to_process(process, region).unwrap();
        session
            .attach_stack_frame_to_process(process, frame)
            .unwrap();
        assert_eq!(
            session.start_process(process).unwrap(),
            ProcessState::Running
        );

        let descriptor = session.process(process).unwrap();
        assert_eq!(descriptor.name(), "notes");
        assert_eq!(descriptor.kind(), ProcessKind::Application);
        assert_eq!(descriptor.tab_id(), Some(tab));
        assert_eq!(descriptor.windows(), &[window]);
        assert_eq!(descriptor.regions(), &[region]);
        assert_eq!(descriptor.stack_frame(), Some(frame));
        assert_eq!(descriptor.state(), &ProcessState::Running);
        assert_eq!(session.window(window).unwrap().process_id(), Some(process));
        assert_eq!(session.process_count(), 1);
    }

    #[test]
    fn subapplication_processes_track_parent_processes() {
        let mut session = RuntimeSession::new();
        let parent = session
            .create_process(
                "workspace",
                ProcessKind::Application,
                None,
                None,
            )
            .unwrap();
        let child = session
            .create_process(
                "search panel",
                ProcessKind::SubApplication,
                Some(parent),
                None,
            )
            .unwrap();

        assert_eq!(session.process(child).unwrap().parent(), Some(parent));
        assert!(matches!(
            session.create_process(
                "broken child",
                ProcessKind::SubApplication,
                Some(ProcessId(999)),
                None,
            ),
            Err(RuntimeError::ProcessNotFound(_))
        ));
    }
}
