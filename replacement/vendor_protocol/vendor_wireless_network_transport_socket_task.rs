use super::{SocketConfiguration, SocketHandle, SocketPhase, TcpEndpoint, TcpPort, TcpSequence, TransportError};
use super::super::dhcp::{Milliseconds, UdpPort};
use super::super::lan::{LanError, LanMode, LanOutcome, LanRuntime, LanState, UdpSocket};
use super::super::packet::UdpEndpoint;
use super::super::super::vendor_wireless_adapter::Adapter;
use super::super::super::vendor_wireless_access_point::{self as access_point, AccessPoint};
use super::super::super::wireless_association::station::{StationError, StationOperationId, StationOutcome, StationProfile, StationRequest, StationRuntime};

const QUEUE: usize = 16;
const COMPLETIONS: usize = 32;
pub const SOCKET_TASK_SEND_CAPACITY: usize = 2048;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SocketTaskOperationId(u32);
impl SocketTaskOperationId { pub const fn get(self) -> u32 { self.0 } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SocketTaskSlot(u8);
impl SocketTaskSlot {
    pub fn parse(slot: u8) -> Option<Self> { (slot < 10).then_some(Self(slot)) }
    pub const fn get(self) -> u8 { self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SocketTaskError { QueueFull, ExhaustedIds, NotInitialized, Busy, Slot, Capacity, Configuration, Cancelled, Lan(LanError), Station(StationError), AccessPoint(access_point::Error) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SocketTaskCompletionKind { Initialize, Deinitialize, StartLan, StopLan, ReconfigureAp }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SocketTaskCompletion {
    pub operation: SocketTaskOperationId,
    pub kind: SocketTaskCompletionKind,
    pub status: Result<(), SocketTaskError>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SocketTaskHandle { Tcp(SocketHandle), Udp(UdpSocket) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SocketTaskSocketResult { Opened(SocketTaskHandle), Closed, Sent(usize), Configured }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SocketTaskSocketCompletion {
    pub operation: SocketTaskOperationId,
    pub slot: SocketTaskSlot,
    pub status: Result<SocketTaskSocketResult, SocketTaskError>,
}
#[derive(Clone)]
pub enum LanStartConfiguration {
    AccessPoint { radio: access_point::Configuration, network: LanMode },
    Station { profile: StationProfile, network: LanMode },
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SocketTaskPhase { Stopped, Ready, Starting, Active, Stopping }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SocketTaskEventKind { Accepted(SocketHandle), Readable, End, Failed(SocketTaskError) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SocketTaskEvent { pub slot: SocketTaskSlot, pub kind: SocketTaskEventKind }
#[derive(Debug)]
pub enum SocketTaskProgress { Idle, Progress, Frame { length: usize }, AccessPoint(access_point::Effect) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SocketServiceKind { Listener, Client, AcceptingListener, Datagram, BroadcastDatagram }
impl SocketServiceKind {
    pub fn parse(raw: u32) -> Option<Self> {
        match raw { 0 => Some(Self::Listener), 1 => Some(Self::Client), 3 => Some(Self::AcceptingListener),
            4 => Some(Self::Datagram), 5 => Some(Self::BroadcastDatagram), _ => None }
    }
}

enum Request {
    Initialize, Deinitialize, StartLan(LanStartConfiguration), StopLan, ReconfigureAp(Option<LanStartConfiguration>),
    Service { slot: SocketTaskSlot, kind: SocketServiceKind, port: TcpPort },
    ConnectService { slot: SocketTaskSlot, peer: TcpEndpoint, sequence: TcpSequence },
    Open { slot: SocketTaskSlot, port: TcpPort, sequence: TcpSequence },
    Connect { slot: SocketTaskSlot, port: TcpPort, peer: TcpEndpoint, sequence: TcpSequence },
    Datagram { slot: SocketTaskSlot, port: UdpPort },
    Close(SocketTaskSlot),
    Configure { slot: SocketTaskSlot, configuration: SocketConfiguration },
    Send { slot: SocketTaskSlot, length: usize, bytes: [u8; SOCKET_TASK_SEND_CAPACITY], destination: Option<UdpEndpoint> },
}
struct Queued { operation: SocketTaskOperationId, request: Request }
#[derive(Clone, Copy)]
struct Slot {
    handle: Option<SocketTaskHandle>, listener: Option<SocketHandle>,
    opening: Option<SocketTaskOperationId>, sending: Option<(SocketTaskOperationId, usize)>,
    closing: Option<SocketTaskOperationId>, readable: bool, ended: bool,
    destination_port: Option<UdpPort>,
    broadcast: bool,
}
impl Slot {
    const fn new() -> Self { Self { handle: None, listener: None, opening: None, sending: None, closing: None, readable: false, ended: false, destination_port: None, broadcast: false } }
}
enum PendingLan {
    StationStart { operation: SocketTaskOperationId, station: StationOperationId, network: LanMode, kind: SocketTaskCompletionKind },
    AccessPointStart { operation: SocketTaskOperationId, network: LanMode, kind: SocketTaskCompletionKind },
    StationStop { operation: SocketTaskOperationId, station: StationOperationId, kind: SocketTaskCompletionKind },
    AccessPointStop { operation: SocketTaskOperationId, kind: SocketTaskCompletionKind },
}
#[derive(Clone, Copy)]
enum RadioOwner { None, Station, AccessPoint }

pub struct SocketTaskRuntime {
    phase: SocketTaskPhase,
    next_operation: u32,
    requests: [Option<Queued>; QUEUE], request_head: usize, request_count: usize,
    completions: [Option<SocketTaskCompletion>; COMPLETIONS], completion_head: usize, completion_count: usize,
    socket_completions: [Option<SocketTaskSocketCompletion>; COMPLETIONS], socket_head: usize, socket_count: usize,
    events: [Option<SocketTaskEvent>; COMPLETIONS], event_head: usize, event_count: usize,
    slots: [Slot; 10],
    pending_lan: Option<PendingLan>,
    radio: RadioOwner,
    sequence: u32,
    restart: Option<LanStartConfiguration>,
    cancelled_start: Option<(SocketTaskOperationId, SocketTaskCompletionKind)>,
}
impl SocketTaskRuntime {
    pub const fn new() -> Self {
        Self { phase: SocketTaskPhase::Stopped, next_operation: 0,
            requests: [const { None }; QUEUE], request_head: 0, request_count: 0,
            completions: [None; COMPLETIONS], completion_head: 0, completion_count: 0,
            socket_completions: [None; COMPLETIONS], socket_head: 0, socket_count: 0,
            events: [None; COMPLETIONS], event_head: 0, event_count: 0,
            slots: [Slot::new(); 10], pending_lan: None, radio: RadioOwner::None, sequence: 0, restart: None, cancelled_start: None }
    }
    pub fn phase(&self) -> SocketTaskPhase { self.phase }
    pub fn set_sequence_seed(&mut self, seed: TcpSequence) { self.sequence = seed.0; }
    fn enqueue(&mut self, request: Request) -> Result<SocketTaskOperationId, SocketTaskError> {
        if self.request_count == QUEUE { return Err(SocketTaskError::QueueFull); }
        self.next_operation = self.next_operation.checked_add(1).ok_or(SocketTaskError::ExhaustedIds)?;
        let operation = SocketTaskOperationId(self.next_operation);
        let tail = (self.request_head + self.request_count) % QUEUE;
        self.requests[tail] = Some(Queued { operation, request });
        self.request_count += 1;
        Ok(operation)
    }
    pub fn initialize(&mut self) -> Result<SocketTaskOperationId, SocketTaskError> { self.enqueue(Request::Initialize) }
    pub fn deinitialize(&mut self) -> Result<SocketTaskOperationId, SocketTaskError> { self.enqueue(Request::Deinitialize) }
    pub fn start_lan(&mut self, configuration: LanStartConfiguration) -> Result<SocketTaskOperationId, SocketTaskError> { self.enqueue(Request::StartLan(configuration)) }
    pub fn stop_lan(&mut self) -> Result<SocketTaskOperationId, SocketTaskError> { self.enqueue(Request::StopLan) }
    pub fn reconfigure_ap(&mut self, radio: access_point::Configuration, network: LanMode) -> Result<SocketTaskOperationId, SocketTaskError> {
        self.enqueue(Request::ReconfigureAp(Some(LanStartConfiguration::AccessPoint { radio, network })))
    }
    /// 0x013e5340: the reconfiguration stop without a following start.
    pub fn disable_reconfigured_ap(&mut self) -> Result<SocketTaskOperationId, SocketTaskError> { self.enqueue(Request::ReconfigureAp(None)) }
    pub fn open_service(&mut self, slot: SocketTaskSlot, kind: SocketServiceKind, port: TcpPort) -> Result<SocketTaskOperationId, SocketTaskError> {
        self.enqueue(Request::Service { slot, kind, port })
    }
    pub fn connect_service(&mut self, slot: SocketTaskSlot, peer: TcpEndpoint, sequence: TcpSequence) -> Result<SocketTaskOperationId, SocketTaskError> {
        self.enqueue(Request::ConnectService { slot, peer, sequence })
    }
    pub fn open(&mut self, slot: SocketTaskSlot, local: TcpEndpoint, sequence: TcpSequence) -> Result<SocketTaskOperationId, SocketTaskError> {
        self.enqueue(Request::Open { slot, port: local.port, sequence })
    }
    pub fn connect(&mut self, slot: SocketTaskSlot, local: TcpEndpoint, peer: TcpEndpoint, sequence: TcpSequence) -> Result<SocketTaskOperationId, SocketTaskError> {
        self.enqueue(Request::Connect { slot, port: local.port, peer, sequence })
    }
    pub fn open_datagram(&mut self, slot: SocketTaskSlot, port: UdpPort) -> Result<SocketTaskOperationId, SocketTaskError> {
        self.enqueue(Request::Datagram { slot, port })
    }
    pub fn close(&mut self, slot: SocketTaskSlot) -> Result<SocketTaskOperationId, SocketTaskError> { self.enqueue(Request::Close(slot)) }
    pub fn configure(&mut self, slot: SocketTaskSlot, configuration: SocketConfiguration) -> Result<SocketTaskOperationId, SocketTaskError> {
        self.enqueue(Request::Configure { slot, configuration })
    }
    pub fn send(&mut self, slot: SocketTaskSlot, bytes: &[u8]) -> Result<SocketTaskOperationId, SocketTaskError> { self.queue_send(slot, bytes, None) }
    pub fn send_to(&mut self, slot: SocketTaskSlot, destination: UdpEndpoint, bytes: &[u8]) -> Result<SocketTaskOperationId, SocketTaskError> {
        self.queue_send(slot, bytes, Some(destination))
    }
    fn queue_send(&mut self, slot: SocketTaskSlot, bytes: &[u8], destination: Option<UdpEndpoint>) -> Result<SocketTaskOperationId, SocketTaskError> {
        if self.request_count == QUEUE { return Err(SocketTaskError::QueueFull); }
        if bytes.len() > SOCKET_TASK_SEND_CAPACITY { return Err(SocketTaskError::Capacity); }
        let mut payload = [0; SOCKET_TASK_SEND_CAPACITY];
        payload[..bytes.len()].copy_from_slice(bytes);
        self.enqueue(Request::Send { slot, length: bytes.len(), bytes: payload, destination })
    }
    pub fn completion(&mut self) -> Option<SocketTaskCompletion> {
        if self.completion_count == 0 { return None; }
        let completion = self.completions[self.completion_head].take();
        self.completion_head = (self.completion_head + 1) % COMPLETIONS;
        self.completion_count -= 1;
        completion
    }
    pub fn socket_completion(&mut self) -> Option<SocketTaskSocketCompletion> {
        if self.socket_count == 0 { return None; }
        let completion = self.socket_completions[self.socket_head].take();
        self.socket_head = (self.socket_head + 1) % COMPLETIONS;
        self.socket_count -= 1;
        completion
    }
    pub fn event(&mut self) -> Option<SocketTaskEvent> {
        if self.event_count == 0 { return None; }
        let event = self.events[self.event_head].take();
        self.event_head = (self.event_head + 1) % COMPLETIONS;
        self.event_count -= 1;
        event
    }
    fn finish(&mut self, operation: SocketTaskOperationId, kind: SocketTaskCompletionKind, status: Result<(), SocketTaskError>) {
        let tail = (self.completion_head + self.completion_count) % COMPLETIONS;
        self.completions[tail] = Some(SocketTaskCompletion { operation, kind, status });
        self.completion_count += 1;
    }
    fn finish_socket(&mut self, operation: SocketTaskOperationId, slot: SocketTaskSlot, status: Result<SocketTaskSocketResult, SocketTaskError>) {
        let tail = (self.socket_head + self.socket_count) % COMPLETIONS;
        self.socket_completions[tail] = Some(SocketTaskSocketCompletion { operation, slot, status });
        self.socket_count += 1;
    }
    fn notify(&mut self, slot: SocketTaskSlot, kind: SocketTaskEventKind) {
        let tail = (self.event_head + self.event_count) % COMPLETIONS;
        self.events[tail] = Some(SocketTaskEvent { slot, kind });
        self.event_count += 1;
    }
}
impl Default for SocketTaskRuntime { fn default() -> Self { Self::new() } }

impl SocketTaskRuntime {
    pub fn handle(&self, slot: SocketTaskSlot) -> Option<SocketTaskHandle> { self.slots[usize::from(slot.0)].handle }
    pub fn receive(&mut self, slot: SocketTaskSlot, lan: &mut LanRuntime<'_>, output: &mut [u8])
        -> Result<SocketTaskReceived, SocketTaskError> {
        let value = match self.slots[usize::from(slot.0)].handle.ok_or(SocketTaskError::Slot)? {
            SocketTaskHandle::Tcp(handle) => SocketTaskReceived::Stream(lan.receive_tcp(handle, output).map_err(SocketTaskError::Lan)?),
            SocketTaskHandle::Udp(handle) => SocketTaskReceived::Datagram(lan.receive_udp(handle, output).map_err(SocketTaskError::Lan)?),
        };
        self.slots[usize::from(slot.0)].readable = false;
        Ok(value)
    }
    fn pop(&mut self) -> Option<Queued> {
        if self.request_count == 0 { return None; }
        let request = self.requests[self.request_head].take();
        self.request_head = (self.request_head + 1) % QUEUE;
        self.request_count -= 1;
        request
    }
    fn clear_slots(&mut self) {
        for index in 0..10 {
            let slot = SocketTaskSlot(index as u8);
            let state = self.slots[index];
            for operation in [state.opening, state.sending.map(|pending| pending.0), state.closing].into_iter().flatten() {
                self.finish_socket(operation, slot, Err(SocketTaskError::Cancelled));
            }
            self.slots[index] = Slot::new();
        }
        self.events.fill(None);
        self.event_count = 0;
        self.event_head = 0;
    }
    fn reset<const F: usize, const D: usize>(&mut self, lan: &mut LanRuntime<'_>, adapter: &mut Adapter<'_, F, D>) {
        lan.stop();
        self.clear_slots();
        adapter.protocols().reset_owner_notification();
    }
    pub fn service_lifecycle<const F: usize, const D: usize>(&mut self, lan: &mut LanRuntime<'_>,
        adapter: &mut Adapter<'_, F, D>) -> Result<SocketTaskProgress, SocketTaskError> {
        if self.completion_count == COMPLETIONS || self.socket_count > COMPLETIONS - 30 { return Err(SocketTaskError::QueueFull); }
        if self.pending_lan.is_some() || !matches!(self.radio, RadioOwner::None) { return Err(SocketTaskError::Busy); }
        let Some(queued) = self.requests[self.request_head].as_ref() else { return Ok(SocketTaskProgress::Idle); };
        let kind = match queued.request {
            Request::Initialize => SocketTaskCompletionKind::Initialize,
            Request::Deinitialize => SocketTaskCompletionKind::Deinitialize,
            _ => return Ok(SocketTaskProgress::Idle),
        };
        let Some(queued) = self.pop() else { return Ok(SocketTaskProgress::Idle); };
        self.reset(lan, adapter);
        self.finish(queued.operation, kind, Ok(()));
        self.phase = if kind == SocketTaskCompletionKind::Initialize { SocketTaskPhase::Ready } else { SocketTaskPhase::Stopped };
        Ok(SocketTaskProgress::Progress)
    }
    fn begin_start(&mut self, operation: SocketTaskOperationId, kind: SocketTaskCompletionKind,
        configuration: LanStartConfiguration, station: &mut StationRuntime, ap: &mut AccessPoint) {
        let pending = match configuration {
            LanStartConfiguration::Station { profile, network } => station.submit(StationRequest::Select(profile))
                .map(|station| PendingLan::StationStart { operation, station, network, kind }).map_err(SocketTaskError::Station),
            LanStartConfiguration::AccessPoint { radio, network } => ap.start(radio)
                .map(|()| PendingLan::AccessPointStart { operation, network, kind }).map_err(SocketTaskError::AccessPoint),
        };
        match pending {
            Ok(pending) => {
                self.radio = if matches!(pending, PendingLan::StationStart { .. }) { RadioOwner::Station } else { RadioOwner::AccessPoint };
                self.pending_lan = Some(pending);
                self.phase = SocketTaskPhase::Starting;
            }
            Err(error) => { self.finish(operation, kind, Err(error)); self.phase = SocketTaskPhase::Ready; }
        }
    }
    fn begin_stop(&mut self, operation: SocketTaskOperationId, kind: SocketTaskCompletionKind,
        lan: &mut LanRuntime<'_>, station: &mut StationRuntime, ap: &mut AccessPoint) {
        if let Some(pending) = self.pending_lan.take() {
            match pending {
                PendingLan::StationStart { operation, kind, .. } | PendingLan::AccessPointStart { operation, kind, .. } =>
                    self.cancelled_start = Some((operation, kind)),
                other => { self.pending_lan = Some(other); self.finish(operation, kind, Err(SocketTaskError::Busy)); return; }
            }
        }
        lan.stop();
        self.clear_slots();
        let pending = match self.radio {
            RadioOwner::Station => {
                let result = if let Some(active) = station.current_operation() { station.cancel(active).map(|()| active) }
                    else { station.submit(StationRequest::Leave) };
                result.map(|station| PendingLan::StationStop { operation, station, kind }).map_err(SocketTaskError::Station)
            }
            RadioOwner::AccessPoint => ap.stop().map(|()| PendingLan::AccessPointStop { operation, kind }).map_err(SocketTaskError::AccessPoint),
            RadioOwner::None => {
                self.pending_lan = Some(PendingLan::AccessPointStop { operation, kind });
                self.phase = SocketTaskPhase::Stopping;
                return;
            }
        };
        match pending {
            Ok(pending) => { self.pending_lan = Some(pending); self.phase = SocketTaskPhase::Stopping; }
            Err(error) => { self.finish(operation, kind, Err(error)); self.phase = SocketTaskPhase::Ready; }
        }
    }
    fn stopped<const F: usize, const D: usize>(&mut self, operation: SocketTaskOperationId, kind: SocketTaskCompletionKind,
        status: Result<(), SocketTaskError>, lan: &mut LanRuntime<'_>, station: &mut StationRuntime,
        ap: &mut AccessPoint, adapter: &mut Adapter<'_, F, D>) {
        if let Some((cancelled, cancelled_kind)) = self.cancelled_start.take() {
            self.finish(cancelled, cancelled_kind, Err(SocketTaskError::Cancelled));
        }
        self.pending_lan = None;
        self.radio = RadioOwner::None;
        if let Err(error) = status {
            self.restart = None;
            self.phase = SocketTaskPhase::Ready;
            self.finish(operation, kind, Err(error));
            return;
        }
        if kind == SocketTaskCompletionKind::Deinitialize || kind == SocketTaskCompletionKind::ReconfigureAp { self.reset(lan, adapter); }
        if let Some(configuration) = self.restart.take() {
            self.begin_start(operation, SocketTaskCompletionKind::ReconfigureAp, configuration, station, ap);
        } else {
            self.finish(operation, kind, Ok(()));
            self.phase = if kind == SocketTaskCompletionKind::Deinitialize { SocketTaskPhase::Stopped } else { SocketTaskPhase::Ready };
        }
    }
    fn start_network(&mut self, operation: SocketTaskOperationId, kind: SocketTaskCompletionKind, mode: LanMode,
        lan: &mut LanRuntime<'_>, now: Milliseconds, output: &mut [u8]) -> SocketTaskProgress {
        self.pending_lan = None;
        match lan.start(mode, now, output) {
            Ok(outcome) => {
                self.phase = SocketTaskPhase::Active;
                self.finish(operation, kind, Ok(()));
                match outcome { LanOutcome::Frame { length } => SocketTaskProgress::Frame { length }, _ => SocketTaskProgress::Progress }
            }
            Err(error) => {
                self.phase = SocketTaskPhase::Ready;
                self.finish(operation, kind, Err(SocketTaskError::Lan(error)));
                SocketTaskProgress::Progress
            }
        }
    }
    pub fn service<const F: usize, const D: usize>(&mut self, lan: &mut LanRuntime<'_>, station: &mut StationRuntime,
        ap: &mut AccessPoint, adapter: &mut Adapter<'_, F, D>, now: Milliseconds, output: &mut [u8])
        -> Result<SocketTaskProgress, SocketTaskError> {
        if self.completion_count > COMPLETIONS - 3 || self.socket_count > COMPLETIONS - 30 ||
            self.event_count > COMPLETIONS - 3 { return Err(SocketTaskError::QueueFull); }
        let interrupt = self.requests[self.request_head].as_ref().is_some_and(|queued|
            matches!(queued.request, Request::StopLan | Request::Deinitialize | Request::ReconfigureAp(_)));
        if self.pending_lan.is_some() && !interrupt {
            match self.pending_lan.as_ref() {
                Some(PendingLan::StationStart { operation, station: expected, network, kind }) => {
                    let (operation, expected, network, kind) = (*operation, *expected, *network, *kind);
                    if let Some(completion) = station.completion().filter(|completion| completion.operation() == expected) {
                        match completion.outcome() {
                            StationOutcome::Connected(_) => return Ok(self.start_network(operation, kind, network, lan, now, output)),
                            StationOutcome::Failed(error) => {
                                self.pending_lan = None; self.radio = RadioOwner::None; self.phase = SocketTaskPhase::Ready;
                                self.finish(operation, kind, Err(SocketTaskError::Station(error)));
                            }
                            _ => {
                                self.pending_lan = None; self.radio = RadioOwner::None; self.phase = SocketTaskPhase::Ready;
                                self.finish(operation, kind, Err(SocketTaskError::Cancelled));
                            }
                        }
                    }
                }
                Some(PendingLan::StationStop { operation, station: expected, kind }) => {
                    let (operation, expected, kind) = (*operation, *expected, *kind);
                    if let Some(completion) = station.completion().filter(|completion| completion.operation() == expected) {
                        let status = match completion.outcome() { StationOutcome::Failed(error) => Err(SocketTaskError::Station(error)), _ => Ok(()) };
                        self.stopped(operation, kind, status, lan, station, ap, adapter);
                    }
                }
                Some(PendingLan::AccessPointStart { .. }) | Some(PendingLan::AccessPointStop { .. }) => {
                    if matches!(self.radio, RadioOwner::None) {
                        if let Some(PendingLan::AccessPointStop { operation, kind }) = self.pending_lan.take() {
                            self.stopped(operation, kind, Ok(()), lan, station, ap, adapter);
                        }
                    } else if let Some(effect) = ap.poll() {
                        match effect {
                            access_point::Effect::Started => {
                                if let Some(PendingLan::AccessPointStart { operation, network, kind }) = self.pending_lan.take() {
                                    return Ok(self.start_network(operation, kind, network, lan, now, output));
                                }
                            }
                            access_point::Effect::Stopped | access_point::Effect::Cancelled => {
                                if let Some(PendingLan::AccessPointStop { operation, kind }) = self.pending_lan.take() {
                                    self.stopped(operation, kind, Ok(()), lan, station, ap, adapter);
                                }
                            }
                            access_point::Effect::Failed(error) => {
                                match self.pending_lan.take() {
                                    Some(PendingLan::AccessPointStart { operation, kind, .. }) => {
                                        self.phase = SocketTaskPhase::Ready; self.radio = RadioOwner::None;
                                        self.finish(operation, kind, Err(SocketTaskError::AccessPoint(error)));
                                    }
                                    Some(PendingLan::AccessPointStop { operation, kind }) =>
                                        self.stopped(operation, kind, Err(SocketTaskError::AccessPoint(error)), lan, station, ap, adapter),
                                    _ => {}
                                }
                            }
                            other => return Ok(SocketTaskProgress::AccessPoint(other)),
                        }
                    }
                    else if matches!(ap.state(), access_point::State::Stopped) {
                        if let Some(PendingLan::AccessPointStop { operation, kind }) = self.pending_lan.take() {
                            self.stopped(operation, kind, Ok(()), lan, station, ap, adapter);
                        }
                    }
                }
                None => {}
            }
            return Ok(SocketTaskProgress::Idle);
        }
        if let Some(queued) = self.pop() {
            match queued.request {
                Request::Initialize => {
                    if !matches!(self.radio, RadioOwner::None) { self.finish(queued.operation, SocketTaskCompletionKind::Initialize, Err(SocketTaskError::Busy)); }
                    else {
                        self.reset(lan, adapter);
                        self.finish(queued.operation, SocketTaskCompletionKind::Initialize, Ok(()));
                        self.phase = SocketTaskPhase::Ready;
                    }
                }
                Request::Deinitialize => self.begin_stop(queued.operation, SocketTaskCompletionKind::Deinitialize, lan, station, ap),
                Request::StopLan => self.begin_stop(queued.operation, SocketTaskCompletionKind::StopLan, lan, station, ap),
                Request::StartLan(configuration) => {
                    if self.phase != SocketTaskPhase::Ready {
                        self.finish(queued.operation, SocketTaskCompletionKind::StartLan, Err(if self.phase == SocketTaskPhase::Stopped { SocketTaskError::NotInitialized } else { SocketTaskError::Busy }));
                    } else { self.begin_start(queued.operation, SocketTaskCompletionKind::StartLan, configuration, station, ap); }
                }
                Request::ReconfigureAp(configuration) => {
                    self.restart = configuration;
                    self.begin_stop(queued.operation, SocketTaskCompletionKind::ReconfigureAp, lan, station, ap);
                }
                request => return self.execute_socket(queued.operation, request, lan, output),
            }
            return Ok(SocketTaskProgress::Progress);
        }
        self.drive_slots(lan)?;
        Ok(SocketTaskProgress::Idle)
    }
    fn execute_socket(&mut self, operation: SocketTaskOperationId, request: Request, lan: &mut LanRuntime<'_>,
        output: &mut [u8]) -> Result<SocketTaskProgress, SocketTaskError> {
        let slot = match &request {
            Request::Service { slot, .. } | Request::ConnectService { slot, .. } | Request::Open { slot, .. } |
            Request::Connect { slot, .. } | Request::Datagram { slot, .. } | Request::Configure { slot, .. } |
            Request::Send { slot, .. } | Request::Close(slot) => *slot,
            _ => return Err(SocketTaskError::Configuration),
        };
        let index = usize::from(slot.0);
        if self.phase != SocketTaskPhase::Active {
            self.finish_socket(operation, slot, Err(SocketTaskError::NotInitialized));
            return Ok(SocketTaskProgress::Progress);
        }
        let result = match request {
            Request::Service { kind, port, .. } => {
                if self.slots[index].handle.is_some() { Err(SocketTaskError::Busy) }
                else {
                    let result = match kind {
                        SocketServiceKind::Client => lan.open_tcp_unbound().map(SocketTaskHandle::Tcp),
                        SocketServiceKind::Datagram => lan.open_udp(UdpPort(port.get())).map(SocketTaskHandle::Udp),
                        SocketServiceKind::BroadcastDatagram => lan.open_udp_ephemeral().map(SocketTaskHandle::Udp),
                        SocketServiceKind::Listener | SocketServiceKind::AcceptingListener => {
                            let sequence = TcpSequence(self.sequence);
                            self.sequence = self.sequence.wrapping_add(65537);
                            let existing = self.slots.iter().filter_map(|slot| slot.listener).find(|handle|
                                lan.tcp_status(*handle).is_ok_and(|status| status.local.port() == Some(port)));
                            match existing { Some(handle) => Ok(SocketTaskHandle::Tcp(handle)), None => lan.listen_tcp(port, sequence).map(SocketTaskHandle::Tcp) }
                        }
                    };
                    result.map(|handle| {
                        self.slots[index].handle = Some(handle);
                        if matches!(kind, SocketServiceKind::Listener | SocketServiceKind::AcceptingListener) {
                            if let SocketTaskHandle::Tcp(listener) = handle { self.slots[index].listener = Some(listener); }
                        }
                        self.slots[index].destination_port = Some(UdpPort(port.get()));
                        self.slots[index].broadcast = kind == SocketServiceKind::BroadcastDatagram;
                        SocketTaskSocketResult::Opened(handle)
                    }).map_err(SocketTaskError::Lan)
                }
            }
            Request::Open { port, sequence, .. } => {
                if self.slots[index].handle.is_some() { Err(SocketTaskError::Busy) }
                else { lan.listen_tcp(port, sequence).map(|handle| {
                    self.slots[index].listener = Some(handle); self.slots[index].handle = Some(SocketTaskHandle::Tcp(handle));
                    SocketTaskSocketResult::Opened(SocketTaskHandle::Tcp(handle))
                }).map_err(SocketTaskError::Lan) }
            }
            Request::Datagram { port, .. } => {
                if self.slots[index].handle.is_some() { Err(SocketTaskError::Busy) }
                else { lan.open_udp(port).map(|handle| {
                    self.slots[index].handle = Some(SocketTaskHandle::Udp(handle));
                    SocketTaskSocketResult::Opened(SocketTaskHandle::Udp(handle))
                }).map_err(SocketTaskError::Lan) }
            }
            Request::Connect { port, peer, sequence, .. } => {
                if self.slots[index].handle.is_some() { Err(SocketTaskError::Busy) }
                else { match lan.connect_tcp(port, peer, sequence) {
                    Ok(handle) => {
                        self.slots[index].handle = Some(SocketTaskHandle::Tcp(handle)); self.slots[index].opening = Some(operation);
                        return Ok(SocketTaskProgress::Progress);
                    }
                    Err(error) => Err(SocketTaskError::Lan(error)),
                } }
            }
            Request::ConnectService { peer, sequence, .. } => {
                if let Some(SocketTaskHandle::Tcp(handle)) = self.slots[index].handle {
                    match lan.connect_tcp_handle(handle, peer, sequence) {
                        Ok(()) => { self.slots[index].opening = Some(operation); return Ok(SocketTaskProgress::Progress); }
                        Err(error) => Err(SocketTaskError::Lan(error)),
                    }
                } else { Err(SocketTaskError::Slot) }
            }
            Request::Configure { configuration, .. } => {
                if let Some(SocketTaskHandle::Tcp(handle)) = self.slots[index].handle {
                    lan.configure_tcp(handle, configuration).map(|()| SocketTaskSocketResult::Configured).map_err(SocketTaskError::Lan)
                } else { Err(SocketTaskError::Slot) }
            }
            Request::Close(_) => {
                if self.slots[index].handle == self.slots[index].listener.map(SocketTaskHandle::Tcp) {
                    if let Some(listener) = self.slots[index].listener {
                        if self.slots.iter().enumerate().any(|(other, slot)| other != index && slot.listener == Some(listener)) {
                            self.slots[index] = Slot::new();
                            self.finish_socket(operation, slot, Ok(SocketTaskSocketResult::Closed));
                            return Ok(SocketTaskProgress::Progress);
                        }
                    }
                }
                match self.slots[index].handle {
                    Some(SocketTaskHandle::Tcp(handle)) => match lan.close_tcp(handle) {
                        Ok(()) => {
                            if matches!(lan.tcp_status(handle), Err(LanError::Transport(TransportError::InvalidHandle))) {
                                self.slots[index] = Slot::new(); Ok(SocketTaskSocketResult::Closed)
                            } else { self.slots[index].closing = Some(operation); return Ok(SocketTaskProgress::Progress); }
                        }
                        Err(error) => Err(SocketTaskError::Lan(error)),
                    }
                    Some(SocketTaskHandle::Udp(handle)) => lan.close_udp(handle).map(|()| { self.slots[index] = Slot::new(); SocketTaskSocketResult::Closed }).map_err(SocketTaskError::Lan),
                    None => Err(SocketTaskError::Slot),
                }
            }
            Request::Send { bytes, length, destination, .. } => {
                if self.slots[index].sending.is_some() { Err(SocketTaskError::Busy) }
                else {
                    match self.slots[index].handle {
                        Some(SocketTaskHandle::Tcp(handle)) => match lan.send_tcp(handle, &bytes[..length]) {
                            Ok(count) if count != 0 => {
                                self.slots[index].sending = Some((operation, count));
                                return Ok(SocketTaskProgress::Progress);
                            }
                            Ok(count) => Ok(SocketTaskSocketResult::Sent(count)),
                            Err(error) => Err(SocketTaskError::Lan(error)),
                        }
                        Some(SocketTaskHandle::Udp(handle)) => {
                            let destination = destination.or_else(|| self.slots[index].destination_port.filter(|_| self.slots[index].broadcast).map(|port|
                                UdpEndpoint { address: super::super::state::Ipv4Address([255; 4]), port }));
                            match destination {
                                Some(destination) => match lan.send_udp(handle, destination, &bytes[..length], output) {
                                    Ok(outcome) => {
                                        self.finish_socket(operation, slot, Ok(SocketTaskSocketResult::Sent(length)));
                                        return Ok(match outcome { LanOutcome::Frame { length } => SocketTaskProgress::Frame { length }, _ => SocketTaskProgress::Progress });
                                    }
                                    Err(error) => Err(SocketTaskError::Lan(error)),
                                },
                                None => Err(SocketTaskError::Configuration),
                            }
                        }
                        None => Err(SocketTaskError::Slot),
                    }
                }
            }
            _ => Err(SocketTaskError::Configuration),
        };
        self.finish_socket(operation, slot, result);
        Ok(SocketTaskProgress::Progress)
    }
    fn drive_slots(&mut self, lan: &mut LanRuntime<'_>) -> Result<(), SocketTaskError> {
        for index in 0..10 {
            if self.socket_count > COMPLETIONS - 3 || self.event_count > COMPLETIONS - 3 { break; }
            let slot = SocketTaskSlot(index as u8);
            let current = self.slots[index];
            if let Some(listener) = current.listener {
                if current.handle == Some(SocketTaskHandle::Tcp(listener)) && current.closing.is_none() {
                    match lan.accept_tcp(listener) {
                        Ok(handle) => { self.slots[index].handle = Some(SocketTaskHandle::Tcp(handle)); self.notify(slot, SocketTaskEventKind::Accepted(handle)); }
                        Err(LanError::Transport(TransportError::WouldBlock)) => {}
                        Err(error) => self.notify(slot, SocketTaskEventKind::Failed(SocketTaskError::Lan(error))),
                    }
                }
            }
            match self.slots[index].handle {
                Some(SocketTaskHandle::Tcp(handle)) => match lan.tcp_status(handle) {
                    Ok(status) => {
                        if let Some(error) = status.failure {
                            let error = SocketTaskError::Lan(LanError::Transport(error));
                            for operation in [self.slots[index].opening.take(), self.slots[index].sending.take().map(|pending| pending.0),
                                self.slots[index].closing.take()].into_iter().flatten() { self.finish_socket(operation, slot, Err(error)); }
                            lan.close_tcp(handle).map_err(SocketTaskError::Lan)?;
                            self.slots[index].handle = None;
                            self.notify(slot, SocketTaskEventKind::Failed(error));
                            continue;
                        }
                        if status.phase == SocketPhase::Established {
                            if let Some(operation) = self.slots[index].opening.take() { self.finish_socket(operation, slot, Ok(SocketTaskSocketResult::Opened(SocketTaskHandle::Tcp(handle)))); }
                        }
                        if !status.write_pending {
                            if let Some((operation, count)) = self.slots[index].sending.take() { self.finish_socket(operation, slot, Ok(SocketTaskSocketResult::Sent(count))); }
                        }
                        if status.readable != 0 && !self.slots[index].readable {
                            self.slots[index].readable = true; self.notify(slot, SocketTaskEventKind::Readable);
                        }
                        if matches!(status.phase, SocketPhase::CloseWait | SocketPhase::Closed) && !self.slots[index].ended {
                            self.slots[index].ended = true; self.notify(slot, SocketTaskEventKind::End);
                        }
                        if status.phase == SocketPhase::Closed {
                            if let Some(operation) = self.slots[index].closing.take() {
                                lan.close_tcp(handle).map_err(SocketTaskError::Lan)?;
                                if let Some(listener) = self.slots[index].listener.filter(|listener| *listener != handle &&
                                    !self.slots.iter().enumerate().any(|(other, slot)| other != index && slot.listener == Some(*listener))) {
                                    lan.cancel_tcp(listener).map_err(SocketTaskError::Lan)?;
                                }
                                self.slots[index] = Slot::new();
                                self.finish_socket(operation, slot, Ok(SocketTaskSocketResult::Closed));
                            }
                        }
                    }
                    Err(error) => { self.notify(slot, SocketTaskEventKind::Failed(SocketTaskError::Lan(error))); self.slots[index] = Slot::new(); }
                },
                Some(SocketTaskHandle::Udp(handle)) => {
                    if lan.udp_received(handle).map_err(SocketTaskError::Lan)?.is_some() && !self.slots[index].readable {
                        self.slots[index].readable = true; self.notify(slot, SocketTaskEventKind::Readable);
                    }
                }
                None => {}
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SocketTaskReceived { Stream(super::ReceiveOutcome), Datagram(Option<super::super::lan::UdpReceived>) }

#[cfg(test)]
#[path = "vendor_wireless_network_transport_socket_task_tests.rs"]
mod tests;
