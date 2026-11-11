use crate::{
    commands::{Backend, CompletionDisposition, CompletionError, ControllerState, PreparedPayload, VendorCompletion},
    controller::{CompletionHandle, ControllerError, QueueController, RequestHandle},
    controller_registration::{ControllerRegistrations, TimerProducers},
    controller_rtos::ControllerRtos,
    service::{Service, ServiceError},
    transport_start_cpu::{vote_connection, StartIo},
};
use xm1_power_services::{
    primitives::ServiceStatus,
    shutdown::{KernelStatus, QueueId, RequestId, ServiceMessage, ShutdownRtos, TaskId},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Queue36Event {
    ConnectionPoll, TransportStatus, Pending, Completed, Operation, Initialize, Shutdown,
    RegisterCompletion, OpenRequest, QueryStatus, BeginConnection, Connected, Disconnected,
    ClassInitialized, ClassError, SessionPoll, ShortTransfer, SubmitPrint, PrintResponse,
    FinishPrint, LongTransfer,
}
impl Queue36Event {
    pub const ALL: [Self; 21] = [Self::ConnectionPoll, Self::TransportStatus, Self::Pending, Self::Completed,
        Self::Operation, Self::Initialize, Self::Shutdown, Self::RegisterCompletion, Self::OpenRequest,
        Self::QueryStatus, Self::BeginConnection, Self::Connected, Self::Disconnected,
        Self::ClassInitialized, Self::ClassError, Self::SessionPoll, Self::ShortTransfer,
        Self::SubmitPrint, Self::PrintResponse, Self::FinishPrint, Self::LongTransfer];
    pub const fn opcode(self) -> u16 {
        match self {
            Self::ConnectionPoll => 0x340d, Self::TransportStatus => 0x340e,
            Self::Pending => 0x3407, Self::Completed => 0x3408, Self::Operation => 0x3406,
            Self::Initialize => 0x3400, Self::Shutdown => 0x3401, Self::RegisterCompletion => 0x3402,
            Self::OpenRequest => 0x3403, Self::QueryStatus => 0x3404, Self::BeginConnection => 0x3405,
            Self::Connected => 0x3409, Self::Disconnected => 0x340a, Self::ClassInitialized => 0x340b,
            Self::ClassError => 0x340c, Self::SessionPoll => 0x340f, Self::ShortTransfer => 0x3410,
            Self::SubmitPrint => 0x3412, Self::PrintResponse => 0x3413, Self::FinishPrint => 0x3414,
            Self::LongTransfer => 0x3411,
        }
    }
    pub fn parse(opcode: u16) -> Option<Self> { Self::ALL.into_iter().find(|event| event.opcode() == opcode) }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Queue36Payload([u8; 24]);
impl Queue36Payload {
    pub const EMPTY: Self = Self([0; 24]);
    pub fn parse(bytes: &[u8]) -> Result<Self, CompletionError> {
        let bytes: [u8; 24] = bytes.try_into().map_err(|_| CompletionError::Payload)?;
        Ok(Self(bytes))
    }
    pub const fn bytes(&self) -> &[u8; 24] { &self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Queue36Message { pub event: Queue36Event, pub payload: Queue36Payload }
impl Queue36Message {
    pub fn parse(opcode: u16, payload: &[u8]) -> Result<Option<Self>, CompletionError> {
        let Some(event) = Queue36Event::parse(opcode) else { return Ok(None); };
        Ok(Some(Self { event, payload: Queue36Payload::parse(payload)? }))
    }
    pub fn publish(self, rtos: &mut impl ShutdownRtos) -> Result<(), KernelStatus> {
        let sender = rtos.current_task();
        rtos.post(ServiceMessage { operation: RequestId(self.event.opcode()), sender, destination: QueueId(36), payload: self.payload.bytes() })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatusConsumer { Phase, Closed, Completion, Request, Status }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstalledConsumer { Task7, Absent }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ControllerConsumers {
    pub phase: InstalledConsumer,
    pub closed: InstalledConsumer,
    pub completion: InstalledConsumer,
    pub request: InstalledConsumer,
    pub status: InstalledConsumer,
}
impl ControllerConsumers {
    pub const fn new() -> Self { Self { phase: InstalledConsumer::Absent, closed: InstalledConsumer::Absent,
        completion: InstalledConsumer::Absent, request: InstalledConsumer::Absent, status: InstalledConsumer::Absent } }
    fn installed(self, consumer: StatusConsumer) -> InstalledConsumer {
        match consumer { StatusConsumer::Phase => self.phase, StatusConsumer::Closed => self.closed,
            StatusConsumer::Completion => self.completion, StatusConsumer::Request => self.request, StatusConsumer::Status => self.status }
    }
}

pub fn publish_status(consumer: StatusConsumer, installed: ControllerConsumers, completion: CompletionHandle,
    request: RequestHandle, status: ServiceStatus, rtos: &mut impl ShutdownRtos) -> Result<(), KernelStatus> {
    if installed.installed(consumer) == InstalledConsumer::Absent { return Ok(()); }
    let mut payload = [0; 8];
    let (operation, length) = match consumer {
        StatusConsumer::Phase => { payload[..4].copy_from_slice(&status.0.to_le_bytes()); (0xb400, 4) }
        StatusConsumer::Closed => (0xb401, 0),
        StatusConsumer::Completion => {
            payload[..4].copy_from_slice(&completion.0.to_le_bytes()); payload[4..].copy_from_slice(&status.0.to_le_bytes()); (0x1714, 8)
        }
        StatusConsumer::Request => {
            payload[..4].copy_from_slice(&request.0.to_le_bytes()); payload[4..].copy_from_slice(&status.0.to_le_bytes()); (0xb403, 8)
        }
        StatusConsumer::Status => { payload[..4].copy_from_slice(&status.0.to_le_bytes()); (0xb404, 4) }
    };
    let sender = rtos.current_task();
    rtos.post(ServiceMessage { operation: RequestId(operation), sender, destination: QueueId(7), payload: &payload[..length] })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionSnapshot { Disconnected, Connected, ConnectedAlternate }
impl ConnectionSnapshot {
    pub const fn completion_handle(self) -> CompletionHandle {
        CompletionHandle(match self { Self::Disconnected => 0, Self::Connected => 21, Self::ConnectedAlternate => 23 })
    }
    pub const fn from_status(high_bit: i32, bit14: i32) -> Self {
        if high_bit != 1 { Self::Disconnected } else if bit14 == 0 { Self::Connected } else { Self::ConnectedAlternate }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControllerOperation {
    Ignore, PollConnection, FlushTransport, Defer, EnterPending, Complete, CompleteUnlessService,
    CompleteAndActivate, OperationRejected(ServiceStatus), ExecuteOperation,
    Initialize, Report(StatusConsumer, ServiceStatus), Shutdown, RegisterCompletion,
    OpenRequest, CloseProtocol, FinishToConnected, FinishToDisconnected,
    BeginConnection, SignalConnected, SignalDisconnected, SignalRequest, RefreshRequest,
    StartTransport, ReportConnected, ReportDisconnected, CloseAfterRequest,
    PollSession, ShortTransfer, SubmitPrint, PrintResponse, FinishPrint, RefreshConnectionByte,
    CloseActiveAndDefer,
}

pub fn operation(event: Queue36Event, state: ControllerState) -> ControllerOperation {
    use ControllerOperation as O;
    use ControllerState as S;
    use Queue36Event as E;
    match event {
        E::ConnectionPoll => match state { S::Stopped | S::Starting => O::Ignore, _ => O::PollConnection },
        E::TransportStatus => match state { S::State8 | S::ActiveVendor => O::FlushTransport, S::PendingVendor => O::Defer, _ => O::Ignore },
        E::Pending => if state == S::ActiveVendor { O::EnterPending } else { O::Ignore },
        E::Completed => match state {
            S::State4 | S::State5 | S::State6 | S::State7 => O::Complete,
            S::ActiveVendor => O::CompleteUnlessService, S::PendingVendor => O::CompleteAndActivate, _ => O::Ignore,
        },
        E::Operation => match state {
            S::Stopped => O::OperationRejected(ServiceStatus(-10)), S::Starting => O::OperationRejected(ServiceStatus(-11)),
            S::State2 | S::State3 => O::OperationRejected(ServiceStatus(-13)),
            S::State4 | S::State5 | S::State6 | S::State7 | S::State8 | S::State9 => O::OperationRejected(ServiceStatus(-14)),
            S::ActiveVendor => O::ExecuteOperation, S::PendingVendor => O::Defer,
        },
        E::Initialize => if state == S::Stopped { O::Initialize } else { O::Report(StatusConsumer::Phase, ServiceStatus(-12)) },
        E::Shutdown => match state { S::State7 => O::Ignore, S::State5 => O::Defer, _ => O::Shutdown },
        E::RegisterCompletion => match state { S::Stopped => O::Report(StatusConsumer::Completion, ServiceStatus(-10)),
            S::Starting => O::RegisterCompletion, _ => O::Report(StatusConsumer::Completion, ServiceStatus(-12)) },
        E::OpenRequest => match state {
            S::Stopped => O::Report(StatusConsumer::Request, ServiceStatus(-10)), S::Starting => O::Report(StatusConsumer::Request, ServiceStatus(-11)),
            S::State2 => O::Report(StatusConsumer::Request, ServiceStatus(-13)), S::State3 => O::OpenRequest,
            S::State5 => O::Report(StatusConsumer::Request, ServiceStatus(-14)), _ => O::Report(StatusConsumer::Request, ServiceStatus(-12)),
        },
        E::QueryStatus => match state {
            S::Stopped => O::Report(StatusConsumer::Status, ServiceStatus(-10)), S::Starting => O::Report(StatusConsumer::Status, ServiceStatus(-11)),
            S::State2 | S::State3 | S::State6 => O::Report(StatusConsumer::Status, ServiceStatus(-13)),
            S::State4 => O::CloseProtocol, S::State5 => O::Report(StatusConsumer::Status, ServiceStatus(-12)),
            S::State7 => O::Report(StatusConsumer::Status, ServiceStatus(-14)),
            S::State8 | S::State9 | S::ActiveVendor => O::FinishToConnected, S::PendingVendor => O::CloseActiveAndDefer,
        },
        E::BeginConnection => if state == S::State9 { O::BeginConnection } else { O::Ignore },
        E::Connected => match state { S::State2 => O::SignalConnected, S::State5 | S::State6 => O::Defer, _ => O::Ignore },
        E::Disconnected => match state { S::State3 => O::SignalDisconnected, S::State4 => O::CloseProtocol,
            S::State5 | S::State6 => O::Defer, S::State8 | S::State9 | S::ActiveVendor => O::FinishToDisconnected,
            S::PendingVendor => O::CloseActiveAndDefer, _ => O::Ignore },
        E::ClassInitialized => match state { S::State8 => O::SignalRequest, S::ActiveVendor => O::RefreshRequest, _ => O::Ignore },
        E::ClassError => match state { S::State4 => O::StartTransport, S::State5 => O::ReportConnected,
            S::State6 => O::ReportDisconnected, S::State7 => O::CloseAfterRequest, _ => O::Ignore },
        E::SessionPoll => match state { S::ActiveVendor | S::PendingVendor => O::PollSession, _ => O::Ignore },
        E::ShortTransfer => if state == S::ActiveVendor { O::ShortTransfer } else { O::Ignore },
        E::SubmitPrint => match state { S::ActiveVendor => O::SubmitPrint, S::PendingVendor => O::Defer, _ => O::Ignore },
        E::PrintResponse => match state { S::ActiveVendor => O::PrintResponse, S::PendingVendor => O::Defer, _ => O::Ignore },
        E::FinishPrint => match state { S::ActiveVendor => O::FinishPrint, S::PendingVendor => O::Defer, _ => O::Ignore },
        E::LongTransfer => if state == S::ActiveVendor { O::RefreshConnectionByte } else { O::Ignore },
    }
}

pub fn table_next_state(event: Queue36Event, state: ControllerState) -> Option<ControllerState> {
    use ControllerState as S;
    use Queue36Event as E;
    match (event, state) {
        (E::Pending, S::ActiveVendor) => Some(S::PendingVendor),
        (E::Completed, S::PendingVendor) => Some(S::ActiveVendor),
        (E::Initialize, S::Stopped) => Some(S::Starting),
        (E::Shutdown, S::Starting | S::State2 | S::State3 | S::State8 | S::State9 | S::ActiveVendor | S::PendingVendor) => Some(S::Stopped),
        (E::Shutdown, S::State4 | S::State6) => Some(S::State7),
        (E::QueryStatus, S::State4) => Some(S::State5),
        (E::QueryStatus, S::State8 | S::State9) => Some(S::State3),
        (E::BeginConnection, S::State9) => Some(S::ActiveVendor),
        (E::Connected, S::State2) => Some(S::State3),
        (E::Disconnected, S::State3 | S::State8 | S::State9) => Some(S::State2),
        (E::Disconnected, S::State4) => Some(S::State6),
        (E::ClassInitialized, S::State8) => Some(S::State9),
        (E::ClassError, S::State4) => Some(S::State8),
        (E::ClassError, S::State5) => Some(S::State3),
        (E::ClassError, S::State6) => Some(S::State2),
        (E::ClassError, S::State7) => Some(S::Stopped),
        _ => None,
    }
}

pub struct DeferredMessages {
    slots: [Option<Queue36Message>; 20],
    read: usize,
    write: usize,
    blocked: bool,
}
impl DeferredMessages {
    pub const fn new() -> Self { Self { slots: [None; 20], read: 0, write: 0, blocked: false } }
    pub fn defer(&mut self, message: Queue36Message) {
        self.blocked = true;
        self.write = (self.write + 1) % self.slots.len();
        self.slots[self.write] = Some(message);
    }
    pub fn set_blocked(&mut self, blocked: bool) { self.blocked = blocked; }
    pub fn next(&mut self) -> Option<Queue36Message> {
        if self.blocked { return None; }
        while self.read != self.write {
            self.read = (self.read + 1) % self.slots.len();
            if let Some(message) = self.slots[self.read].take() { return Some(message); }
        }
        None
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestArgument(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestOffset(pub u32);
/// Stock +0x3f4 and +0x3f8.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestRegistration {
    pub argument: RequestArgument,
    pub offset: RequestOffset,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Queue36ReceiveError { Kernel(KernelStatus), Message(CompletionError) }
pub trait Queue36Receive {
    fn receive(&mut self) -> Result<Option<Queue36Message>, Queue36ReceiveError>;
}

#[cfg(target_arch = "arm")]
impl Queue36Receive for xm1_power_services::shutdown_rtos::NativeShutdownRtos {
    fn receive(&mut self) -> Result<Option<Queue36Message>, Queue36ReceiveError> {
        use xm1_power_services::shutdown_receive::{PayloadAddress, ReceiveKernel, ReceiveTimeout};
        let queue = QueueId(36);
        let block = ReceiveKernel::receive(self, queue, ReceiveTimeout::Forever).map_err(Queue36ReceiveError::Kernel)?;
        let (header, length) = ReceiveKernel::header(self, block);
        let mut payload = [0; 24];
        if length == 24 {
            // Safety: `payload` is 24 bytes and the copy length is exactly 24.
            unsafe { ReceiveKernel::copy_payload(self, block, PayloadAddress(payload.as_mut_ptr() as u32), 24); }
        }
        ReceiveKernel::release(self, queue, block);
        if Queue36Event::parse(header.operation.0).is_none() { return Ok(None); }
        if length != 24 { return Err(Queue36ReceiveError::Message(CompletionError::Payload)); }
        Queue36Message::parse(header.operation.0, &payload).map_err(Queue36ReceiveError::Message)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionLevel { Disconnected, Connected }
impl ConnectionLevel {
    pub const fn from_high_bit(value: i32) -> Self { if value == 1 { Self::Connected } else { Self::Disconnected } }
    pub const fn handle(self) -> CompletionHandle { CompletionHandle(match self { Self::Disconnected => 0, Self::Connected => 1 }) }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PollCount(u32);
pub struct ConnectionPolling { changed: PollCount, flag16: PollCount }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionPollEffect { None, Change(ConnectionLevel), ClearFlag16 }
impl ConnectionPolling {
    pub const fn new() -> Self { Self { changed: PollCount(0), flag16: PollCount(0) } }
    pub fn reset_changed(&mut self) { self.changed = PollCount(0); }
    pub fn poll(&mut self, state: ControllerState, handle: CompletionHandle, level: ConnectionLevel) -> ConnectionPollEffect {
        use ControllerState as S;
        match state {
            S::Stopped | S::Starting | S::State2 | S::State3 | S::State4 => {
                if handle.0 & 1 == level.handle().0 { self.reset_changed(); }
                else {
                    self.changed.0 = self.changed.0.wrapping_add(1);
                    if self.changed.0 > 15 { self.reset_changed(); return ConnectionPollEffect::Change(level); }
                }
            }
            S::ActiveVendor if handle.0 & 16 != 0 => {
                self.flag16.0 = self.flag16.0.wrapping_add(1);
                if self.flag16.0 > 19 { self.flag16 = PollCount(0); return ConnectionPollEffect::ClearFlag16; }
            }
            S::PendingVendor => self.flag16 = PollCount(0),
            _ => {}
        }
        ConnectionPollEffect::None
    }
}

#[derive(Debug)]
pub enum RuntimeError { Controller(ControllerError), Service(ServiceError), Queue(KernelStatus) }
pub struct ControllerRuntime<'a, B> {
    pub controller: QueueController,
    pub service: Service<'a, B>,
    pub registrations: ControllerRegistrations<'a>,
    pub consumers: ControllerConsumers,
    pub deferred: DeferredMessages,
    pub request: Option<RequestRegistration>,
    pub polling: ConnectionPolling,
}
impl<'a, B: Backend> ControllerRuntime<'a, B> {
    pub fn new(service: Service<'a, B>, producers: &'a TimerProducers) -> Self {
        Self { controller: QueueController::new(ControllerState::Stopped, CompletionHandle(0), RequestHandle(0)),
            service, registrations: ControllerRegistrations::new(producers), consumers: ControllerConsumers::new(), deferred: DeferredMessages::new(), request: None, polling: ConnectionPolling::new() }
    }
    pub fn next_message(&mut self, receiver: &mut impl Queue36Receive) -> Result<Queue36Message, Queue36ReceiveError> {
        loop {
            if let Some(message) = self.deferred.next() { return Ok(message); }
            if let Some(message) = Queue36Receive::receive(receiver)? { return Ok(message); }
        }
    }
    pub fn initialize_completion(&mut self, snapshot: ConnectionSnapshot, rtos: &mut impl ShutdownRtos) -> Result<(), RuntimeError> {
        let handle = snapshot.completion_handle();
        self.controller.set_completion_handle(handle);
        self.consumers.completion = InstalledConsumer::Task7;
        let queued = Queue36Message { event: Queue36Event::RegisterCompletion, payload: Queue36Payload::EMPTY }.publish(rtos);
        let notified = if snapshot != ConnectionSnapshot::Disconnected {
            self.publish(StatusConsumer::Completion, ServiceStatus(0), rtos)
        } else { Ok(()) };
        queued.and(notified).map_err(RuntimeError::Queue)
    }
    pub fn initialize_phase(&mut self, rtos: &mut impl ShutdownRtos) -> Result<(), KernelStatus> {
        self.consumers.phase = InstalledConsumer::Task7;
        Queue36Message { event: Queue36Event::Initialize, payload: Queue36Payload::EMPTY }.publish(rtos)
    }
    pub fn request_close(&mut self, rtos: &mut impl ShutdownRtos) -> Result<(), KernelStatus> {
        self.consumers.closed = InstalledConsumer::Task7;
        Queue36Message { event: Queue36Event::Shutdown, payload: Queue36Payload::EMPTY }.publish(rtos)
    }
    pub fn request_open(&mut self, handle: RequestHandle, registration: RequestRegistration, rtos: &mut impl ShutdownRtos) -> Result<(), KernelStatus> {
        self.controller.set_request_handle(handle);
        self.request = Some(registration);
        self.consumers.request = InstalledConsumer::Task7;
        Queue36Message { event: Queue36Event::OpenRequest, payload: Queue36Payload::EMPTY }.publish(rtos)
    }
    pub fn request_status(&mut self, rtos: &mut impl ShutdownRtos) -> Result<(), KernelStatus> {
        self.consumers.status = InstalledConsumer::Task7;
        Queue36Message { event: Queue36Event::QueryStatus, payload: Queue36Payload::EMPTY }.publish(rtos)
    }
    pub fn mark_transport_status(&mut self, rtos: &mut impl ShutdownRtos) -> Result<(), KernelStatus> {
        let old = self.controller.completion_handle();
        let published = if old.0 & 16 == 0 {
            self.controller.set_completion_handle(CompletionHandle(old.0 | 16));
            self.publish(StatusConsumer::Completion, ServiceStatus(0), rtos)
        } else { Ok(()) };
        let queued = Queue36Message { event: Queue36Event::TransportStatus, payload: Queue36Payload::EMPTY }.publish(rtos);
        published.and(queued)
    }
    /// Stock 0x00d815c0 after its 0x00d7b75c call into the USB driver.
    pub fn class_initialized(&mut self, handle: RequestHandle, rtos: &mut impl ShutdownRtos) -> Result<(), KernelStatus> {
        self.controller.set_request_handle(handle);
        Queue36Message { event: Queue36Event::ClassInitialized, payload: Queue36Payload::EMPTY }.publish(rtos)
    }
    /// Stock 0x00d81618.
    pub fn class_error(&mut self, status: ServiceStatus, rtos: &mut impl ShutdownRtos) -> Result<(), KernelStatus> {
        let mut payload = [0; 24];
        payload[..4].copy_from_slice(&status.0.to_le_bytes());
        Queue36Message { event: Queue36Event::ClassError, payload: Queue36Payload(payload) }.publish(rtos)
    }
    /// Stock 0x00d814f4.
    pub fn update_connection_handle(&mut self, handle: CompletionHandle, io: &mut impl StartIo, rtos: &mut impl ShutdownRtos) -> Result<(), KernelStatus> {
        let old = self.controller.completion_handle();
        self.controller.set_completion_handle(handle);
        if (old.0 ^ handle.0) & 1 == 0 { return Ok(()); }
        self.polling.reset_changed();
        let connected = handle.0 & 1 != 0;
        let _ = vote_connection(io, connected);
        let event = if connected { Queue36Event::Connected } else { Queue36Event::Disconnected };
        Queue36Message { event, payload: Queue36Payload::EMPTY }.publish(rtos)
    }
    /// Stock 0x00d81d04.
    pub fn poll_connection(&mut self, level: ConnectionLevel, io: &mut impl StartIo, rtos: &mut impl ControllerRtos) -> Result<(), KernelStatus> {
        let effect = self.polling.poll(self.controller.state(), self.controller.completion_handle(), level);
        let result = match effect {
            ConnectionPollEffect::Change(level) => self.update_connection_handle(level.handle(), io, rtos),
            ConnectionPollEffect::ClearFlag16 => {
                self.controller.set_completion_handle(CompletionHandle(self.controller.completion_handle().0 & !16));
                self.publish(StatusConsumer::Completion, ServiceStatus(0), rtos)
            }
            ConnectionPollEffect::None => Ok(()),
        };
        self.registrations.acknowledge_primary_poll();
        if effect == ConnectionPollEffect::Change(ConnectionLevel::Connected) { return result; }
        let priority = rtos.set_task_priority(TaskId(36), 11);
        result.and(priority)
    }
    pub fn publish(&self, consumer: StatusConsumer, status: ServiceStatus, rtos: &mut impl ShutdownRtos) -> Result<(), KernelStatus> {
        publish_status(consumer, self.consumers, self.controller.completion_handle(), self.controller.request_handle(), status, rtos)
    }
    /// Applies the 0x3408 cell of the current state and keeps the deferred queue with it.
    pub fn completion(&mut self, message: Queue36Message, prepared: Option<PreparedPayload<'a>>) -> Result<CompletionDisposition, RuntimeError> {
        let pending = self.controller.state() == ControllerState::PendingVendor;
        let result = self.service.consume_queue_completion(&mut self.controller, VendorCompletion::parse(message.payload.bytes()), prepared);
        if pending && self.controller.state() == ControllerState::ActiveVendor { self.deferred.set_blocked(false); }
        if matches!(result, Ok(CompletionDisposition::Deferred)) { self.deferred.defer(message); }
        result.map_err(RuntimeError::Service)
    }
}

#[cfg(test)]
#[path = "controller_runtime_tests.rs"]
mod tests;
