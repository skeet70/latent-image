//! X-M1 1.50 network task 86, 0x013e98f8..0x013eac1c.
//! x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; transition indexing is confirmed from stock bytes.

use super::vendor_wireless_network::transport::socket_task::{
    SocketTaskRuntime, SocketTaskOperationId, SocketTaskError, SocketTaskCompletion, SocketTaskCompletionKind, LanStartConfiguration,
};
#[path = "vendor_wireless_service_task_interface.rs"]
pub mod interface;
#[path = "vendor_wireless_service_task_interface_modes.rs"]
pub mod interface_modes;
#[path = "vendor_wireless_service_task_station.rs"]
pub mod station;
#[path = "vendor_wireless_service_task_ap.rs"]
pub mod ap;
#[path = "vendor_wireless_service_task_discovery.rs"]
pub mod discovery;
use interface::{ControllerError, InterfaceRuntime, TaskContext};
use interface_modes::{InterfaceConfiguration, InterfaceCompletion, InterfaceCompletionKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum TaskState { S0, S1, S2, S3, S4, S5, S6, S7 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum MessageClass { Operation, OperationResult, Initialize, Deinitialize, Start, Stop, Reset, StartResult, ControlResult, ConfigurationResult, InterfaceResult, Completion, Event, Other }
impl MessageClass {
    pub const fn parse(kind: u16) -> Self {
        match kind {
            0x660b => Self::Operation, 0xe60b => Self::OperationResult,
            0x6600 => Self::Initialize, 0x6601 => Self::Deinitialize,
            0x6602 => Self::Start, 0x6603 => Self::Stop, 0x6606 => Self::Reset,
            0x660c => Self::StartResult, 0x662a => Self::ControlResult,
            0x660d => Self::ConfigurationResult, 0x660e => Self::InterfaceResult,
            0x6609 => Self::Completion, 0x660a => Self::Event, _ => Self::Other,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TaskMessageKind(pub u16);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PayloadError { Length, Offset }
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskPayload([u8; 200]);
impl TaskPayload {
    pub fn parse(bytes: &[u8]) -> Result<Self, PayloadError> {
        let bytes = bytes.try_into().map_err(|_| PayloadError::Length)?;
        Ok(Self(bytes))
    }
    pub const fn bytes(&self) -> &[u8; 200] { &self.0 }
    pub fn word(&self, offset: usize) -> Result<u32, PayloadError> {
        let end = offset.checked_add(4).ok_or(PayloadError::Offset)?;
        let bytes = self.0.get(offset..end).ok_or(PayloadError::Offset)?;
        Ok(u32::from_le_bytes(bytes.try_into().map_err(|_| PayloadError::Offset)?))
    }
    pub fn halfword(&self, offset: usize) -> Result<u16, PayloadError> {
        let end = offset.checked_add(2).ok_or(PayloadError::Offset)?;
        let bytes = self.0.get(offset..end).ok_or(PayloadError::Offset)?;
        Ok(u16::from_le_bytes(bytes.try_into().map_err(|_| PayloadError::Offset)?))
    }
}
pub use discovery::NetworkPacketLease;
#[derive(Debug, Eq, PartialEq)]
pub struct TaskMessage { pub kind: TaskMessageKind, pub payload: TaskPayload, pub packet: Option<NetworkPacketLease> }
impl TaskMessage {
    pub const fn new(kind: TaskMessageKind, payload: TaskPayload) -> Self { Self { kind, payload, packet: None } }
    pub fn with_packet(mut self, packet: NetworkPacketLease) -> Self { self.packet = Some(packet); self }
    pub const fn class(&self) -> MessageClass { MessageClass::parse(self.kind.0) }
    pub fn status(kind: TaskMessageKind, status: CompletionStatus) -> Self {
        let mut payload = [0; 200];
        payload[..4].copy_from_slice(&status.raw().to_le_bytes());
        Self::new(kind, TaskPayload(payload))
    }
    pub fn from_interface_completion(completion: InterfaceCompletion) -> Self {
        let kind = match completion.kind { InterfaceCompletionKind::Initialized => 0x660c, InterfaceCompletionKind::Configured | InterfaceCompletionKind::Reset => 0xe60b, InterfaceCompletionKind::Started => 0x662a };
        Self::status(TaskMessageKind(kind), CompletionStatus::from_raw(completion.status.raw()))
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueInitializationError { Occupied }
#[derive(Debug)]
pub struct TaskEnqueueError { pub message: TaskMessage }
pub struct TaskQueue<'a> { storage: &'a mut [Option<TaskMessage>], head: usize, length: usize }
impl<'a> TaskQueue<'a> {
    pub fn new(storage: &'a mut [Option<TaskMessage>]) -> Result<Self, QueueInitializationError> {
        if storage.iter().any(Option::is_some) { return Err(QueueInitializationError::Occupied); }
        Ok(Self { storage, head: 0, length: 0 })
    }
    pub fn enqueue(&mut self, message: TaskMessage) -> Result<(), TaskEnqueueError> {
        if self.length == self.storage.len() { return Err(TaskEnqueueError { message }); }
        let index = (self.head + self.length) % self.storage.len();
        self.storage[index] = Some(message);
        self.length += 1;
        Ok(())
    }
    pub fn dequeue(&mut self) -> Option<TaskMessage> {
        if self.length == 0 { return None; }
        let message = self.storage[self.head].take();
        self.head = (self.head + 1) % self.storage.len();
        self.length -= 1;
        message
    }
    pub const fn is_empty(&self) -> bool { self.length == 0 }
    pub const fn len(&self) -> usize { self.length }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompletionStatus { Success, NotInitialized, InvalidState, AlreadyInitialized, Failed, Other(i32) }
impl CompletionStatus {
    pub const fn from_raw(status: i32) -> Self {
        match status { 0 => Self::Success, -10 => Self::NotInitialized, -14 => Self::InvalidState, -12 => Self::AlreadyInitialized, -255 => Self::Failed, value => Self::Other(value) }
    }
    pub const fn raw(self) -> i32 {
        match self { Self::Success => 0, Self::NotInitialized => -10, Self::InvalidState => -14, Self::AlreadyInitialized => -12, Self::Failed => -255, Self::Other(value) => value }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InterfaceOperation { Configure, Reset, Start, ConfigureNamed, ConfigureMedia }
impl InterfaceOperation {
    pub const fn parse(value: u32) -> Option<Self> {
        match value { 0 => Some(Self::Configure), 1 => Some(Self::Reset), 2 => Some(Self::Start), 3 => Some(Self::ConfigureNamed), 4 => Some(Self::ConfigureMedia), _ => None }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompletionTarget { Initialize, Deinitialize, Start, Stop, Reset, Operation, OperationReset, OperationDone, Completion, Event }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompletionRegistration(pub u64);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TaskNotification { pub registration: CompletionRegistration, pub target: CompletionTarget, pub status: CompletionStatus, pub published_phase: TaskState, pub transition_state: TaskState }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskError { NotificationCapacity, Busy, UnexpectedCompletion, Payload(PayloadError), Socket(SocketTaskError) }
#[derive(Debug)]
pub enum DispatchError { Task(TaskError), Interface(ControllerError), MissingLanConfiguration }
impl From<TaskError> for DispatchError { fn from(error: TaskError) -> Self { Self::Task(error) } }
impl From<ControllerError> for DispatchError { fn from(error: ControllerError) -> Self { Self::Interface(error) } }

#[derive(Clone, Copy)]
enum SocketContinuation { Initialize, Deinitialize }
struct PendingSocket { operation: SocketTaskOperationId, continuation: SocketContinuation }
pub struct TaskRuntime {
    state: TaskState,
    phase: TaskState,
    operation: Option<InterfaceOperation>,
    completion_flag: bool,
    command_enabled: bool,
    registrations: [Option<CompletionRegistration>; 10],
    notifications: [Option<TaskNotification>; 32],
    read: usize,
    length: usize,
    pending_socket: Option<PendingSocket>,
    lan_configuration: Option<LanStartConfiguration>,
    queued_stop: bool,
}
impl Default for TaskRuntime { fn default() -> Self { Self::new() } }
impl TaskRuntime {
    pub const fn new() -> Self {
        Self { state: TaskState::S0, phase: TaskState::S0, operation: None, completion_flag: false, command_enabled: true,
            registrations: [None; 10], notifications: [None; 32], read: 0, length: 0, pending_socket: None, lan_configuration: None, queued_stop: false }
    }
    pub fn set_lan_configuration(&mut self, configuration: LanStartConfiguration) { self.lan_configuration = Some(configuration); }
    pub fn reconfigure_ap(&mut self, configuration: &super::vendor_wireless_access_point::ServiceConfiguration, socket: &mut SocketTaskRuntime) -> Result<SocketTaskOperationId, TaskError> {
        let network = super::vendor_wireless_network::lan::LanMode::AccessPoint {
            configuration: super::vendor_wireless_network::dhcp::ServerConfiguration::camera_default(),
        };
        socket.reconfigure_ap(configuration.configuration(), network).map_err(TaskError::Socket)
    }
    pub fn disable_reconfigured_ap(&mut self, socket: &mut SocketTaskRuntime) -> Result<SocketTaskOperationId, TaskError> {
        socket.disable_reconfigured_ap().map_err(TaskError::Socket)
    }
    pub fn dispatch<I: crate::filesystem_runtime::SectorIo>(&mut self, message: &mut TaskMessage, configuration: &mut Option<InterfaceConfiguration>, interface: &mut InterfaceRuntime, context: &mut TaskContext<'_, '_, I>) -> Result<(), DispatchError> {
        if self.pending_socket.is_some() { return Err(TaskError::Busy.into()); }
        self.reserve(3)?;
        match message.class() {
            MessageClass::Operation => {
                let operation = message.payload.word(0).map_err(TaskError::Payload)?;
                if let Some(operation) = InterfaceOperation::parse(operation) {
                    if matches!(self.state, TaskState::S6 | TaskState::S7) {
                        self.operation = Some(operation);
                        interface.begin_operation(operation, configuration, context)?;
                    } else { self.reject_operation(operation)?; }
                } else if matches!(self.state, TaskState::S6 | TaskState::S7) { self.operation = None; }
            }
            MessageClass::OperationResult => self.operation_result(CompletionStatus::from_raw(message.payload.word(0).map_err(TaskError::Payload)? as i32))?,
            MessageClass::Initialize => {
                if self.state == TaskState::S0 { interface.reset_service_state(context)?; }
                self.initialize(context.socket)?;
            }
            MessageClass::Deinitialize => self.deinitialize(context.socket)?,
            MessageClass::Start if self.state == TaskState::S1 => {
                let result = interface.initialize(context);
                self.completion_flag = false;
                self.phase = TaskState::S2;
                self.state = TaskState::S2;
                result?;
            }
            MessageClass::Start => self.reject_start()?,
            MessageClass::Stop if matches!(self.state, TaskState::S6 | TaskState::S7) => {
                if self.state == TaskState::S6 { self.command_enabled = true; }
                interface.stop(context)?;
                if interface.dispatch(message, context)? {
                    self.state = TaskState::S6;
                    self.phase = TaskState::S6;
                } else {
                    self.state = TaskState::S3;
                    let result = context.socket.stop_lan().map_err(TaskError::Socket);
                    self.phase = TaskState::S3;
                    result?;
                }
            }
            MessageClass::Stop => self.reject_stop()?,
            MessageClass::Reset if matches!(self.state, TaskState::S2 | TaskState::S5) => {
                self.phase = TaskState::S4;
                self.state = TaskState::S4;
            }
            MessageClass::Reset if self.state == TaskState::S7 => {
                interface.dispatch(message, context)?;
                if let Some(operation) = self.operation.take() {
                    interface.abort_operation(operation, context)?;
                    self.finish_operation(operation, CompletionStatus::Failed);
                }
                self.notify(CompletionTarget::Reset, CompletionStatus::Success);
            }
            MessageClass::Reset => self.reject_reset()?,
            MessageClass::StartResult if self.state == TaskState::S2 => {
                interface.prepare_control(context)?;
                let configuration = self.lan_configuration.as_ref().ok_or(DispatchError::MissingLanConfiguration)?.clone();
                let result = context.socket.start_lan(configuration).map_err(TaskError::Socket);
                self.phase = TaskState::S5;
                self.state = TaskState::S5;
                result?;
            }
            MessageClass::StartResult if self.state == TaskState::S4 => self.complete_start_failure()?,
            MessageClass::ControlResult if self.state == TaskState::S7 => { interface.dispatch(message, context)?; }
            MessageClass::ConfigurationResult if self.state == TaskState::S4 => { context.socket.stop_lan().map_err(TaskError::Socket)?; }
            MessageClass::ConfigurationResult => self.complete_configuration()?,
            MessageClass::InterfaceResult => self.complete_interface_initialize()?,
            MessageClass::Completion => self.completion_request()?,
            MessageClass::Event => self.event_request()?,
            MessageClass::Other if matches!(self.state, TaskState::S6 | TaskState::S7) => {
                let handled = interface.dispatch(message, context)?;
                if self.state == TaskState::S6 && !handled && self.command_enabled {
                    self.queued_stop = true;
                    self.command_enabled = false;
                }
            }
            _ => {}
        }
        Ok(())
    }
    pub fn queued_message(&mut self) -> Option<TaskMessage> {
        if !self.queued_stop { return None; }
        self.queued_stop = false;
        Some(TaskMessage::new(TaskMessageKind(0x6603), TaskPayload([0; 200])))
    }
    pub fn socket_completion(&mut self, completion: SocketTaskCompletion) -> Result<Option<TaskMessage>, TaskError> {
        match completion.kind {
            SocketTaskCompletionKind::Initialize | SocketTaskCompletionKind::Deinitialize => { self.receive_socket_completion(completion)?; Ok(None) }
            SocketTaskCompletionKind::StartLan | SocketTaskCompletionKind::StopLan => {
                let kind = if completion.kind == SocketTaskCompletionKind::StartLan { 0x660d } else { 0x660e };
                let status = if completion.status.is_ok() { CompletionStatus::Success } else { CompletionStatus::Failed };
                Ok(Some(TaskMessage::status(TaskMessageKind(kind), status)))
            }
            SocketTaskCompletionKind::ReconfigureAp => Err(TaskError::UnexpectedCompletion),
        }
    }
    pub fn initialize(&mut self, socket: &mut SocketTaskRuntime) -> Result<(), TaskError> {
        if self.pending_socket.is_some() { return Err(TaskError::Busy); }
        if self.state != TaskState::S0 { return self.reject_initialize(); }
        self.reserve(1)?;
        self.lan_configuration = None;
        let operation = socket.initialize().map_err(TaskError::Socket)?;
        self.phase = TaskState::S0;
        self.completion_flag = false;
        self.pending_socket = Some(PendingSocket { operation, continuation: SocketContinuation::Initialize });
        Ok(())
    }
    pub fn deinitialize(&mut self, socket: &mut SocketTaskRuntime) -> Result<(), TaskError> {
        if self.pending_socket.is_some() { return Err(TaskError::Busy); }
        if self.state != TaskState::S1 { return self.reject_deinitialize(); }
        self.reserve(1)?;
        self.phase = TaskState::S0;
        let operation = socket.deinitialize().map_err(TaskError::Socket)?;
        self.pending_socket = Some(PendingSocket { operation, continuation: SocketContinuation::Deinitialize });
        Ok(())
    }
    pub fn receive_socket_completion(&mut self, completion: SocketTaskCompletion) -> Result<(), TaskError> {
        let pending = self.pending_socket.as_ref().ok_or(TaskError::UnexpectedCompletion)?;
        if completion.operation != pending.operation { return Err(TaskError::UnexpectedCompletion); }
        self.reserve(1)?;
        let continuation = pending.continuation;
        self.pending_socket = None;
        match continuation {
            SocketContinuation::Initialize => {
                self.phase = TaskState::S1;
                self.notify(CompletionTarget::Initialize, CompletionStatus::Success);
                self.state = TaskState::S1;
            }
            SocketContinuation::Deinitialize => {
                self.notify(CompletionTarget::Deinitialize, CompletionStatus::Success);
                self.state = TaskState::S0;
            }
        }
        Ok(())
    }
    pub const fn state(&self) -> TaskState { self.state }
    pub const fn phase(&self) -> TaskState { self.phase }
    pub const fn completion_flag(&self) -> bool { self.completion_flag }
    pub const fn command_enabled(&self) -> bool { self.command_enabled }
    pub const fn operation(&self) -> Option<InterfaceOperation> { self.operation }
    pub fn register(&mut self, target: CompletionTarget, registration: CompletionRegistration) { self.registrations[target as usize] = Some(registration); }
    pub fn notification(&mut self) -> Option<TaskNotification> {
        if self.length == 0 { return None; }
        let result = self.notifications[self.read].take();
        self.read = (self.read + 1) % self.notifications.len(); self.length -= 1;
        result
    }
    fn reserve(&self, count: usize) -> Result<(), TaskError> {
        if self.notifications.len() - self.length < count { Err(TaskError::NotificationCapacity) } else { Ok(()) }
    }
    fn notify(&mut self, target: CompletionTarget, status: CompletionStatus) {
        if let Some(registration) = self.registrations[target as usize] {
            let index = (self.read + self.length) % self.notifications.len();
            self.notifications[index] = Some(TaskNotification { registration, target, status, published_phase: self.phase, transition_state: self.state }); self.length += 1;
        }
    }
    fn finish_operation(&mut self, operation: InterfaceOperation, status: CompletionStatus) {
        self.operation = None;
        self.notify(match operation { InterfaceOperation::Reset => CompletionTarget::OperationReset, InterfaceOperation::Start => CompletionTarget::OperationDone, _ => CompletionTarget::Operation }, status);
    }
    pub fn reject_operation(&mut self, operation: InterfaceOperation) -> Result<(), TaskError> {
        self.reserve(1)?;
        self.finish_operation(operation, if self.state == TaskState::S0 { CompletionStatus::NotInitialized } else { CompletionStatus::InvalidState });
        Ok(())
    }
    pub fn operation_result(&mut self, status: CompletionStatus) -> Result<(), TaskError> {
        self.reserve(1)?;
        if let Some(operation) = self.operation {
            self.finish_operation(operation, match self.state { TaskState::S0 => CompletionStatus::NotInitialized, TaskState::S6 | TaskState::S7 => status, _ => CompletionStatus::InvalidState });
        }
        Ok(())
    }
    pub fn reject_initialize(&mut self) -> Result<(), TaskError> { self.reserve(1)?; self.notify(CompletionTarget::Initialize, CompletionStatus::AlreadyInitialized); Ok(()) }
    pub fn reject_deinitialize(&mut self) -> Result<(), TaskError> {
        self.reserve(1)?;
        self.notify(CompletionTarget::Deinitialize, if self.state == TaskState::S0 { CompletionStatus::NotInitialized } else { CompletionStatus::InvalidState }); Ok(())
    }
    pub fn reject_start(&mut self) -> Result<(), TaskError> {
        self.reserve(1)?;
        let status = match self.state { TaskState::S0 => CompletionStatus::NotInitialized, TaskState::S6 | TaskState::S7 => CompletionStatus::Success, _ => CompletionStatus::AlreadyInitialized };
        self.notify(CompletionTarget::Start, status); Ok(())
    }
    pub fn reject_stop(&mut self) -> Result<(), TaskError> {
        self.reserve(1)?;
        self.notify(CompletionTarget::Stop, match self.state { TaskState::S0 => CompletionStatus::NotInitialized, TaskState::S3 => CompletionStatus::AlreadyInitialized, _ => CompletionStatus::InvalidState }); Ok(())
    }
    pub fn reject_reset(&mut self) -> Result<(), TaskError> {
        self.reserve(1)?;
        self.notify(if self.state == TaskState::S6 { CompletionTarget::Deinitialize } else { CompletionTarget::Reset }, match self.state { TaskState::S0 => CompletionStatus::NotInitialized, TaskState::S1 => CompletionStatus::Success, TaskState::S4 => CompletionStatus::AlreadyInitialized, _ => CompletionStatus::InvalidState }); Ok(())
    }
    pub fn complete_interface_initialize(&mut self) -> Result<(), TaskError> {
        if !matches!(self.state, TaskState::S3 | TaskState::S4) { return Ok(()); }
        self.reserve(if self.state == TaskState::S4 { 2 } else { 1 })?;
        self.phase = TaskState::S1;
        if self.state == TaskState::S4 { self.notify(CompletionTarget::Start, CompletionStatus::Failed); self.notify(CompletionTarget::Reset, CompletionStatus::Success); }
        else { self.notify(CompletionTarget::Stop, CompletionStatus::Success); }
        self.state = TaskState::S1; Ok(())
    }
    pub fn complete_configuration(&mut self) -> Result<(), TaskError> {
        if self.state != TaskState::S5 { return Ok(()); }
        self.reserve(1)?; self.phase = TaskState::S7; self.notify(CompletionTarget::Start, CompletionStatus::Success); self.state = TaskState::S7; Ok(())
    }
    pub fn complete_start_failure(&mut self) -> Result<(), TaskError> {
        if self.state != TaskState::S4 { return Ok(()); }
        self.reserve(2)?; self.phase = TaskState::S1;
        self.notify(CompletionTarget::Start, CompletionStatus::Failed); self.notify(CompletionTarget::Reset, CompletionStatus::Success);
        self.state = TaskState::S1; Ok(())
    }
    pub fn completion_request(&mut self) -> Result<(), TaskError> {
        if self.state != TaskState::S7 { return Ok(()); }
        self.reserve(1)?; self.completion_flag = true; self.notify(CompletionTarget::Completion, CompletionStatus::Success); Ok(())
    }
    pub fn event_request(&mut self) -> Result<(), TaskError> {
        if self.state != TaskState::S7 { return Ok(()); }
        self.reserve(1)?; self.completion_flag = false; self.notify(CompletionTarget::Event, CompletionStatus::Success); Ok(())
    }
}
