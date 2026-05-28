use crate::commands::{CommandBus, CommandSource};
use crate::runtime::RuntimeSession;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WorkerId(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerKind {
    Runtime,
    Storage,
    Memory,
    Ui,
    Process,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerState {
    Idle,
    Running,
    Drained,
    Paused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkerStats {
    pub id: WorkerId,
    pub kind: WorkerKind,
    pub state: WorkerState,
    pub tick_count: u64,
    pub processed_commands: u64,
    pub emitted_events: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkerTick {
    pub worker_id: WorkerId,
    pub processed_commands: usize,
    pub emitted_events: usize,
    pub remaining_commands: usize,
}

#[derive(Debug, Clone)]
pub struct WorkerPump {
    id: WorkerId,
    kind: WorkerKind,
    state: WorkerState,
    tick_count: u64,
    processed_commands: u64,
    emitted_events: u64,
}

impl WorkerId {
    pub fn new(id: u64) -> Self {
        Self(id)
    }

    pub fn as_u64(self) -> u64 {
        self.0
    }
}

impl WorkerPump {
    pub fn new(id: WorkerId, kind: WorkerKind) -> Self {
        Self {
            id,
            kind,
            state: WorkerState::Idle,
            tick_count: 0,
            processed_commands: 0,
            emitted_events: 0,
        }
    }

    pub fn id(&self) -> WorkerId {
        self.id
    }

    pub fn kind(&self) -> WorkerKind {
        self.kind
    }

    pub fn state(&self) -> WorkerState {
        self.state
    }

    pub fn source(&self) -> CommandSource {
        CommandSource::Worker(self.id.as_u64())
    }

    pub fn stats(&self) -> WorkerStats {
        WorkerStats {
            id: self.id,
            kind: self.kind,
            state: self.state,
            tick_count: self.tick_count,
            processed_commands: self.processed_commands,
            emitted_events: self.emitted_events,
        }
    }

    pub fn pause(&mut self) {
        self.state = WorkerState::Paused;
    }

    pub fn resume(&mut self) {
        if matches!(self.state, WorkerState::Paused) {
            self.state = WorkerState::Idle;
        }
    }

    pub fn tick(
        &mut self,
        bus: &mut CommandBus,
        session: &mut RuntimeSession,
        command_budget: usize,
    ) -> WorkerTick {
        if matches!(self.state, WorkerState::Paused) || command_budget == 0 {
            return WorkerTick {
                worker_id: self.id,
                processed_commands: 0,
                emitted_events: 0,
                remaining_commands: bus.pending_commands(),
            };
        }

        self.state = WorkerState::Running;
        self.tick_count = self.tick_count.saturating_add(1);
        let events_before = bus.pending_events();
        let mut processed = 0usize;

        while processed < command_budget && bus.process_one(session) {
            processed += 1;
        }

        let emitted = bus.pending_events().saturating_sub(events_before);
        self.processed_commands = self
            .processed_commands
            .saturating_add(processed as u64);
        self.emitted_events = self.emitted_events.saturating_add(emitted as u64);
        self.state = if bus.pending_commands() == 0 {
            WorkerState::Drained
        } else {
            WorkerState::Idle
        };

        WorkerTick {
            worker_id: self.id,
            processed_commands: processed,
            emitted_events: emitted,
            remaining_commands: bus.pending_commands(),
        }
    }

    pub fn drain(&mut self, bus: &mut CommandBus, session: &mut RuntimeSession) -> WorkerTick {
        let budget = bus.pending_commands();
        self.tick(bus, session, budget)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{RegionCommand, TritMuxCommand, TritMuxEvent};

    #[test]
    fn worker_tick_respects_command_budget() {
        let mut session = RuntimeSession::new();
        let mut bus = CommandBus::new();
        let mut worker = WorkerPump::new(WorkerId::new(7), WorkerKind::Runtime);

        bus.submit(
            worker.source(),
            TritMuxCommand::Region(RegionCommand::CreateKv {
                name: "first".to_string(),
            }),
        );
        bus.submit(
            worker.source(),
            TritMuxCommand::Region(RegionCommand::CreateKv {
                name: "second".to_string(),
            }),
        );

        let first_tick = worker.tick(&mut bus, &mut session, 1);
        assert_eq!(first_tick.processed_commands, 1);
        assert_eq!(first_tick.emitted_events, 1);
        assert_eq!(first_tick.remaining_commands, 1);
        assert_eq!(worker.state(), WorkerState::Idle);

        let second_tick = worker.drain(&mut bus, &mut session);
        assert_eq!(second_tick.processed_commands, 1);
        assert_eq!(second_tick.remaining_commands, 0);
        assert_eq!(worker.stats().processed_commands, 2);
        assert_eq!(worker.stats().emitted_events, 2);
        assert_eq!(worker.state(), WorkerState::Drained);
        assert_eq!(session.region_count(), 2);
    }

    #[test]
    fn paused_workers_do_not_drain_commands() {
        let mut session = RuntimeSession::new();
        let mut bus = CommandBus::new();
        let mut worker = WorkerPump::new(WorkerId::new(2), WorkerKind::Storage);
        worker.pause();

        bus.submit(
            worker.source(),
            TritMuxCommand::Region(RegionCommand::CreateKv {
                name: "cold".to_string(),
            }),
        );

        let tick = worker.tick(&mut bus, &mut session, 1);
        assert_eq!(tick.processed_commands, 0);
        assert_eq!(tick.remaining_commands, 1);
        assert_eq!(session.region_count(), 0);

        worker.resume();
        let tick = worker.tick(&mut bus, &mut session, 1);
        assert_eq!(tick.processed_commands, 1);
        assert_eq!(worker.state(), WorkerState::Drained);
    }

    #[test]
    fn worker_emits_normal_command_events() {
        let mut session = RuntimeSession::new();
        let mut bus = CommandBus::new();
        let mut worker = WorkerPump::new(WorkerId::new(4), WorkerKind::Process);

        bus.submit(
            worker.source(),
            TritMuxCommand::Region(RegionCommand::CreateTab {
                title: "main".to_string(),
            }),
        );

        worker.drain(&mut bus, &mut session);
        let event = bus.pop_event().unwrap();
        assert_eq!(event.source(), CommandSource::Worker(4));
        assert!(matches!(event.event(), TritMuxEvent::TabCreated { .. }));
    }
}
