//! Queue-36 task body: executes the cells of the stock table at 0x00d834d8 over the owned controller state.

use core::cell::RefCell;
use crate::{
    commands::{Backend, CompletionDisposition, ControllerState, PreparedPayload, ProtocolCompletion, VendorCompletion},
    controller::{publish_secondary_status, CompletionHandle, ControllerError, ProtocolFiles, RequestKind, SessionTeardown, ShutdownDisposition, ShutdownResources},
    controller_registration::{PollPeriod, SharedRtos, TimerProducers, TimerRegistry},
    controller_runtime::{operation, publish_status, table_next_state, ConnectionLevel, ControllerOperation, ControllerRuntime,
        Queue36Message, Queue36Payload, Queue36Receive, Queue36ReceiveError, RuntimeError, StatusConsumer},
    pictbridge_runtime::{AbortStyle, Copies, DpsTransport, FileId, Identity, PictBridge, PrintRequest, Publication},
    pictbridge_runtime::xml::XmlError,
    ptp_runtime::PtpRuntime,
    ptp_types::BatteryLevel,
    service::Service,
    transport::{cpu, irq_control, scheduler, ChannelLease, ControllerTeardown, PublishedRegion, Register, TeardownPlatform, TeardownProgress, TransportError},
    transport_connection_cpu::{ActiveFileClose, CloseError, ConnectionState},
    transport_pump::{TransportPump, UsbEvent},
    transport_start_cpu::{bulk_packet_size, link_connected, CallbackTable, SessionMode, StartError, StartIo, StartProgress, StartRegister, StartRuntime, UsbStart},
};
use xm1_power_services::{
    primitives::ServiceStatus,
    shutdown::{KernelStatus, ShutdownRtos},
    shutdown_task7_8_transport::unregister_secondary,
};

#[cfg(test)]
#[path = "controller_root_tests.rs"]
mod tests;

#[derive(Debug)]
pub enum RootError {
    Controller(ControllerError), Runtime(RuntimeError), Start(StartError), Close(CloseError), Timer(ServiceStatus),
    Kernel(KernelStatus), Receive(Queue36ReceiveError), Transport(TransportError), Print(XmlError),
}
impl From<ControllerError> for RootError { fn from(error: ControllerError) -> Self { Self::Controller(error) } }
impl From<RuntimeError> for RootError { fn from(error: RuntimeError) -> Self { Self::Runtime(error) } }
impl From<StartError> for RootError { fn from(error: StartError) -> Self { Self::Start(error) } }
impl From<CloseError> for RootError { fn from(error: CloseError) -> Self { Self::Close(error) } }
impl From<ServiceStatus> for RootError { fn from(status: ServiceStatus) -> Self { Self::Timer(status) } }
impl From<KernelStatus> for RootError { fn from(status: KernelStatus) -> Self { Self::Kernel(status) } }
impl From<TransportError> for RootError { fn from(error: TransportError) -> Self { Self::Transport(error) } }
impl From<XmlError> for RootError { fn from(error: XmlError) -> Self { Self::Print(error) } }

/// The mode-two software stock reaches through +0x404 and the continuation slots +0x42c..+0x468.
pub trait ProtocolApplication: ProtocolFiles {
    fn complete(&mut self, completion: ProtocolCompletion);
}

/// The DMA channel leases of the open USB session, handed over once for its teardown.
pub trait SessionChannels {
    fn take(&mut self) -> [ChannelLease; 2];
}

/// Task-7 and Power services stock calls directly from the cells.
pub trait RootHost {
    /// The flags word stock 0x006a0bb0 stores.
    fn battery_flags(&mut self) -> u32;
    /// Stock 0x00d832a4, 0x00d832dc and 0x00d832f8 forward to task 7.
    fn print_publication(&mut self, publication: Publication);
}

/// The tables stock 0x00d81948 selects: the results of 0x00d85cf0 and 0x00d7f2b0, and the word at 0x01469ccc.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CallbackTables { pub protocol: PublishedRegion, pub service: PublishedRegion, pub unselected: PublishedRegion }

pub struct RootProviders<'r, 's, 'irq, 'd, 'a, K, I, T, M, G, P, C, F, A, H, O> {
    pub start: &'r mut StartRuntime<'s, 'irq, K, I, T>,
    pub machine: &'r M,
    pub domain: scheduler::layout::Domain,
    pub timers: &'r mut G,
    pub platform: &'r mut P,
    pub channels: &'r mut C,
    pub files: &'r mut F,
    pub application: &'r mut A,
    pub host: &'r mut H,
    pub rtos: &'r RefCell<O>,
    pub tables: CallbackTables,
    pub identity: Identity<'r>,
    pub dps: &'r mut DpsTransport<'d>,
    /// The payload a successful provider completion publishes.
    pub prepared: Option<PreparedPayload<'a>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PhaseWrite { Table, Dynamic(ControllerState), Unchanged }

struct Faults(Option<RootError>);
impl Faults {
    fn note<E: Into<RootError>>(&mut self, result: Result<(), E>) {
        if let Err(error) = result { self.0.get_or_insert(error.into()); }
    }
}

/// Stock 0x00d85320 passes payload words 0 and 1 and the text at payload+8; 0x00d954d0 keeps at most 15 bytes of it.
pub fn print_request(payload: &Queue36Payload) -> Result<PrintRequest<'_>, XmlError> {
    let bytes = payload.bytes();
    let text = &bytes[8..23];
    let length = text.iter().position(|byte| *byte == 0).unwrap_or(text.len());
    let date = core::str::from_utf8(&text[..length]).map_err(|_| XmlError::Malformed)?;
    Ok(PrintRequest { file: FileId(word(bytes, 0)), copies: Copies::parse(word(bytes, 4)), date: if length == 0 { None } else { Some(date) } })
}

fn word(bytes: &[u8; 24], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

pub struct ControllerRoot<'a, B> {
    pub runtime: ControllerRuntime<'a, B>,
    pub connection: ConnectionState,
    pub ptp: PtpRuntime,
    pub bridge: PictBridge,
    usb: UsbStart<'a>,
    pump: TransportPump,
    fault: Option<RootError>,
}

impl<'a, B: Backend> ControllerRoot<'a, B> {
    pub fn new(service: Service<'a, B>, producers: &'a TimerProducers, usb: UsbStart<'a>) -> Self {
        Self { runtime: ControllerRuntime::new(service, producers), connection: ConnectionState::new(),
            ptp: PtpRuntime::new(), bridge: PictBridge::new(), usb, pump: TransportPump::new(), fault: None }
    }

    /// The events of the latest 0x00d7d154 pass.
    pub fn pumped(&self) -> crate::transport_pump::Pumped { self.pump.last() }

    /// The first failure of the latest failed step that `run` continued past.
    pub fn take_fault(&mut self) -> Option<RootError> { self.fault.take() }

    /// Executes one stock dispatch (0x00d80e98). Every effect of the cell runs; the first failure is returned afterwards.
    pub fn step<K, I, T, M, G, P, C, F, A, H, O>(
        &mut self, message: Queue36Message, p: &mut RootProviders<'_, '_, '_, '_, 'a, K, I, T, M, G, P, C, F, A, H, O>,
    ) -> Result<(), RootError>
    where K: irq_control::Backend, I: StartIo, T: cpu::DelayCounter, M: scheduler::Machine, G: TimerRegistry, P: TeardownPlatform,
        C: SessionChannels, F: ActiveFileClose, A: ProtocolApplication, H: RootHost, O: ShutdownRtos,
    {
        let state = self.runtime.controller.state();
        let mut cell = Cell { root: self, p, faults: Faults(None) };
        let write = cell.execute(operation(message.event, state), state, message);
        let next = match write {
            PhaseWrite::Table => table_next_state(message.event, state),
            PhaseWrite::Dynamic(next) => Some(next),
            PhaseWrite::Unchanged => None,
        };
        if let Some(next) = next {
            let entered = cell.root.runtime.controller.transition(next);
            cell.faults.note(entered);
        }
        cell.faults.0.map_or(Ok(()), Err)
    }

    /// Stock 0x00d81218. Returns only when the queue receive fails.
    pub fn run<K, I, T, M, G, P, C, F, A, H, O>(
        &mut self, receiver: &mut impl Queue36Receive, p: &mut RootProviders<'_, '_, '_, '_, 'a, K, I, T, M, G, P, C, F, A, H, O>,
    ) -> RootError
    where K: irq_control::Backend, I: StartIo, T: cpu::DelayCounter, M: scheduler::Machine, G: TimerRegistry, P: TeardownPlatform,
        C: SessionChannels, F: ActiveFileClose, A: ProtocolApplication, H: RootHost, O: ShutdownRtos,
    {
        loop {
            let result = match self.runtime.next_message(receiver) {
                Ok(message) => self.step(message, p),
                Err(Queue36ReceiveError::Message(error)) => Err(RootError::Receive(Queue36ReceiveError::Message(error))),
                Err(error) => return RootError::Receive(error),
            };
            if let Err(error) = result { self.fault = Some(error); }
        }
    }
}

/// Stock 0x00d7c790 over the open session: the interrupt handle first, then the DMA channels and the reset pulse.
struct Teardown<'x, 'm, 's, 'irq, K, I, T, P, M, C> {
    usb: &'x mut UsbStart<'m>,
    start: &'x mut StartRuntime<'s, 'irq, K, I, T>,
    channels: &'x mut C,
    platform: &'x mut P,
    machine: &'x M,
    domain: scheduler::layout::Domain,
    open: Option<ControllerTeardown>,
}
impl<K: irq_control::Backend, I, T, P: TeardownPlatform, M: scheduler::Machine, C: SessionChannels> SessionTeardown for Teardown<'_, '_, '_, '_, K, I, T, P, M, C> {
    fn resume(&mut self) -> Result<TeardownProgress, TransportError> {
        if self.open.is_none() {
            self.usb.release_irq(self.start);
            self.open = Some(self.usb.teardown(self.channels.take()));
        }
        let progress = match self.open.as_mut() {
            Some(teardown) => teardown.resume(self.platform, self.machine, self.domain)?,
            None => TeardownProgress::Complete,
        };
        if progress == TeardownProgress::Complete {
            self.open = None;
            self.usb.finish_teardown();
        }
        Ok(progress)
    }
}

struct Cell<'x, 'r, 's, 'irq, 'd, 'a, B, K, I, T, M, G, P, C, F, A, H, R> {
    root: &'x mut ControllerRoot<'a, B>,
    p: &'x mut RootProviders<'r, 's, 'irq, 'd, 'a, K, I, T, M, G, P, C, F, A, H, R>,
    faults: Faults,
}

impl<'a, B, K, I, T, M, G, P, C, F, A, H, R> Cell<'_, '_, '_, '_, '_, 'a, B, K, I, T, M, G, P, C, F, A, H, R>
where B: Backend, K: irq_control::Backend, I: StartIo, T: cpu::DelayCounter, M: scheduler::Machine, G: TimerRegistry, P: TeardownPlatform,
    C: SessionChannels, F: ActiveFileClose, A: ProtocolApplication, H: RootHost, R: ShutdownRtos,
{
    fn kind(&self) -> RequestKind { self.root.runtime.controller.request_handle().kind() }

    /// The 0x340e cell: stock 0x00d7d154 through the state-8 and active cells at 0x00d84174/0x00d84188.
    fn flush_transport(&mut self) {
        let pumped = self.root.pump.read(&mut self.root.usb, &mut self.p.start.io);
        for event in pumped.events() { self.apply_pump_event(event); }
        TransportPump::finish(&mut self.root.usb, &mut self.p.start.io, pumped);
    }

    fn apply_pump_event(&mut self, event: UsbEvent) {
        match event {
            UsbEvent::CompletionReset => {
                self.p.start.io.write32(Register::InterruptReset, 0x8181_0303);
                let result = self.root.runtime.update_connection_handle(CompletionHandle(0), &mut self.p.start.io,
                    &mut SharedRtos(self.p.rtos));
                self.faults.note(result);
            }
            UsbEvent::TransferReset => self.transfer_reset(),
            UsbEvent::BusState | UsbEvent::ControlComplete | UsbEvent::ControlPacket
            | UsbEvent::ControlTransferZero | UsbEvent::ControlTransferOne | UsbEvent::ChannelThree
            | UsbEvent::ChannelOneComplete | UsbEvent::ChannelTwoRequest
            | UsbEvent::ChannelTwoTransfer | UsbEvent::ChannelOneTransfer => {
                // The callback table at 0x01469768+0x0c and the DMA transfers at 0x01469798/0x014697fc
                // are not implemented; `pumped` exposes these events.
            }
        }
    }

    /// Stock 0x00d7d360's register effects; the channel stop at 0x01469768+4/+8 has no owned provider.
    fn transfer_reset(&mut self) {
        self.root.usb.reset_transfer_state();
        let io = &mut self.p.start.io;
        io.write32(Register::OutStart, 0);
        let mask = io.read16(Register::InMaskHigh);
        io.write16(Register::InMaskHigh, mask & 0xfffc);
        let control = io.start_read16(StartRegister::EndpointControl);
        io.start_write16(StartRegister::EndpointControl, control | 0xc);
        io.write32(Register::OutInitialize, 0x101);
        let result = cpu::spin_delay(&mut self.p.start.timer, 1);
        self.faults.note(result);
        let io = &mut self.p.start.io;
        io.write16(Register::EndpointPulse, 0xfff);
        let bus = io.read16(Register::BusSpeed);
        let control = io.read32(Register::OutControl);
        io.write32(Register::OutControl, (control & !0x1000) | (u32::from((bus & 0x7fff) >> 14) << 12));
    }

    fn publish(&mut self, consumer: StatusConsumer, status: i32) {
        let result = self.root.runtime.publish(consumer, ServiceStatus(status), &mut SharedRtos(self.p.rtos));
        self.faults.note(result);
    }

    fn start_primary(&mut self, period: PollPeriod) {
        let result = self.root.runtime.registrations.start_primary(period, self.p.timers);
        self.faults.note(result);
    }

    fn execute(&mut self, operation: ControllerOperation, state: ControllerState, message: Queue36Message) -> PhaseWrite {
        use ControllerOperation as O;
        use ControllerState as S;
        match operation {
            O::Ignore | O::OperationRejected(_) | O::ExecuteOperation => {}
            O::PollConnection => {
                let level = if matches!(state, S::State2 | S::State3 | S::State4) && link_connected(&mut self.p.start.io) {
                    ConnectionLevel::Connected
                } else { ConnectionLevel::Disconnected };
                let result = self.root.runtime.poll_connection(level, &mut self.p.start.io, &mut SharedRtos(self.p.rtos));
                self.faults.note(result);
            }
            O::FlushTransport => self.flush_transport(),
            O::Defer => self.root.runtime.deferred.defer(message),
            O::EnterPending => self.root.runtime.deferred.set_blocked(false),
            O::Complete | O::CompleteUnlessService | O::CompleteAndActivate => { self.complete(state, message); return PhaseWrite::Unchanged; }
            O::Initialize => {
                let result = self.root.usb.initialize(self.p.start);
                self.faults.note(result);
                self.publish(StatusConsumer::Phase, 0);
            }
            O::Report(consumer, status) => self.publish(consumer, status.0),
            O::Shutdown => { self.shutdown(message); return PhaseWrite::Unchanged; }
            O::RegisterCompletion => {
                self.start_primary(PollPeriod::Connection);
                let connected = self.root.runtime.controller.completion_handle().0 & 1 != 0;
                return PhaseWrite::Dynamic(if connected { S::State3 } else { S::State2 });
            }
            O::OpenRequest => {
                if self.kind().opens_files() {
                    self.p.application.start();
                    return PhaseWrite::Dynamic(S::State4);
                }
                self.start_transport();
                return PhaseWrite::Dynamic(S::State8);
            }
            O::CloseProtocol => self.p.application.abort(),
            O::FinishToConnected => return self.finish(ConnectionLevel::Connected, state, message),
            O::FinishToDisconnected => return self.finish(ConnectionLevel::Disconnected, state, message),
            O::BeginConnection => {
                self.begin_selected();
                let table = self.select_table();
                self.begin_connection_request();
                self.bind(table);
                let kind = self.kind();
                let result = self.root.runtime.registrations.start_secondary(kind, &mut self.root.connection, self.p.timers);
                self.faults.note(result);
                self.start_primary(PollPeriod::Vendor);
            }
            O::SignalConnected | O::SignalDisconnected => self.publish(StatusConsumer::Completion, 0),
            O::SignalRequest => self.publish(StatusConsumer::Request, 0),
            O::RefreshRequest => {
                self.begin_selected();
                let table = self.select_table();
                self.bind(table);
            }
            O::StartTransport => self.start_transport(),
            O::ReportConnected => {
                self.root.runtime.deferred.set_blocked(false);
                self.publish(StatusConsumer::Status, 0);
                self.publish(StatusConsumer::Request, 0);
            }
            O::ReportDisconnected => {
                self.root.runtime.deferred.set_blocked(false);
                self.publish(StatusConsumer::Request, -13);
                let runtime = &self.root.runtime;
                // Stock 0x00d85254 passes handle 0.
                let result = publish_status(StatusConsumer::Completion, runtime.consumers, CompletionHandle(0),
                    runtime.controller.request_handle(), ServiceStatus(0), &mut SharedRtos(self.p.rtos));
                self.faults.note(result);
            }
            O::CloseAfterRequest => {
                self.publish(StatusConsumer::Request, 0);
                self.publish(StatusConsumer::Closed, 0);
            }
            O::PollSession => {
                let notification = self.root.connection.update_connection(state == S::PendingVendor, &mut self.root.runtime.registrations);
                // Only the request-2 table (0x00ed0444) has a consumer in slot +0x10.
                if let (Some(notification), RequestKind::Protocol) = (notification, self.kind()) {
                    let result = publish_secondary_status(ServiceStatus(notification.status()), &mut SharedRtos(self.p.rtos));
                    self.faults.note(result);
                }
            }
            O::ShortTransfer => {
                let result = self.root.bridge.service(&self.p.identity, self.p.dps);
                self.faults.note(result);
                if let Some(publication) = self.root.bridge.take_publication() { self.p.host.print_publication(publication); }
            }
            O::SubmitPrint => {
                self.root.runtime.deferred.set_blocked(false);
                match print_request(&message.payload) {
                    Ok(request) => self.root.bridge.set_request(request),
                    Err(error) => self.faults.note(Err::<(), _>(error)),
                }
            }
            O::PrintResponse => {
                self.root.runtime.deferred.set_blocked(false);
                self.root.bridge.abort(AbortStyle::parse(word(message.payload.bytes(), 0)));
            }
            O::FinishPrint => {
                self.root.runtime.deferred.set_blocked(false);
                self.root.bridge.continue_job();
            }
            O::RefreshConnectionByte => {
                let battery = BatteryLevel::from_status_flags(self.p.host.battery_flags());
                self.root.connection.battery_changed(&mut self.root.ptp, battery);
            }
            O::CloseActiveAndDefer => {
                self.close_active(state);
                self.root.runtime.deferred.defer(message);
            }
        }
        PhaseWrite::Table
    }

    /// Stock 0x00d819fc behind the three 0x3408 cells.
    fn complete(&mut self, state: ControllerState, message: Queue36Message) {
        use ControllerState as S;
        let deferred = state == S::ActiveVendor && self.kind() == RequestKind::Service;
        let transport_phase = matches!(state, S::State8 | S::State9 | S::ActiveVendor | S::PendingVendor);
        if !deferred && transport_phase && VendorCompletion::parse(message.payload.bytes()).rearms_transport() {
            let result = self.root.usb.finalize(self.p.start);
            self.faults.note(result);
        }
        match self.root.runtime.completion(message, self.p.prepared) {
            Ok(CompletionDisposition::Protocol(completion)) => self.p.application.complete(completion),
            Ok(_) => {}
            Err(error) => self.faults.note(Err::<(), _>(error)),
        }
    }

    fn usb_shutdown(&mut self) {
        let mut teardown = Teardown { usb: &mut self.root.usb, start: &mut *self.p.start, channels: &mut *self.p.channels,
            platform: &mut *self.p.platform, machine: self.p.machine, domain: self.p.domain, open: None };
        let result = loop {
            match teardown.resume() {
                Ok(TeardownProgress::Pending) => {}
                Ok(TeardownProgress::Complete) => break Ok(()),
                Err(error) => break Err(error),
            }
        };
        self.faults.note(result);
    }

    fn shutdown(&mut self, message: Queue36Message) {
        let kind = self.kind();
        let mut teardown = Teardown { usb: &mut self.root.usb, start: &mut *self.p.start, channels: &mut *self.p.channels,
            platform: &mut *self.p.platform, machine: self.p.machine, domain: self.p.domain, open: None };
        loop {
            let runtime = &mut self.root.runtime;
            let mut parts = runtime.registrations.shutdown_parts(kind, &mut self.root.connection, &mut runtime.deferred, self.p.rtos);
            let mut rtos = SharedRtos(self.p.rtos);
            let mut resources = ShutdownResources {
                registrations: &mut parts.storage, registry: &mut *self.p.timers, teardown: &mut teardown, rtos: &mut rtos,
                files: &mut *self.p.application, transfer: &mut self.root.bridge, short_timer: parts.short, long_timer: parts.long,
            };
            match runtime.controller.shutdown(&mut resources) {
                Ok(ShutdownDisposition::Pending) => {}
                Ok(ShutdownDisposition::Deferred) => { runtime.deferred.defer(message); return; }
                Ok(ShutdownDisposition::Ignored | ShutdownDisposition::Complete) => return,
                Err(error) => { self.faults.note(Err::<(), _>(error)); return; }
            }
        }
    }

    /// Stock 0x00d7e8b4, 0x00d7e8f0, 0x00d7b42c and 0x00d81cdc, shared by 0x00d84af8 and 0x00d851c0.
    fn start_transport(&mut self) {
        self.root.usb.reset_session();
        self.root.usb.set_session(SessionMode::from_abi(self.root.runtime.controller.request_handle().0));
        let started = self.root.usb.start(self.p.start, self.p.machine, self.p.domain);
        if started == Ok(StartProgress::Disconnected) {
            let result = self.root.runtime.update_connection_handle(CompletionHandle(0), &mut self.p.start.io, &mut SharedRtos(self.p.rtos));
            self.faults.note(result);
        }
        let _ = self.root.runtime.registrations.stop_primary(self.p.timers);
        if started.is_err() { self.publish(StatusConsumer::Request, -255); }
    }

    /// Stock 0x00d818e4.
    fn begin_selected(&mut self) {
        match self.kind() {
            RequestKind::Protocol => self.root.ptp.initialize(bulk_packet_size(&mut self.p.start.io)),
            RequestKind::Service => self.root.runtime.service.begin_connection(),
            RequestKind::FileOnly | RequestKind::Unselected => {}
        }
    }

    fn select_table(&mut self) -> PublishedRegion {
        let kind = self.kind();
        let tables = self.p.tables;
        self.root.connection.select_request_buffer(kind, tables.protocol, tables.service, tables.unselected)
    }

    /// Stock 0x00d82210.
    fn begin_connection_request(&mut self) {
        match self.kind() {
            RequestKind::Protocol => {
                let battery = BatteryLevel::from_status_flags(self.p.host.battery_flags());
                let result = self.root.connection.begin_connection_protocol(&mut self.root.ptp, &mut self.root.bridge,
                    &mut self.root.runtime.registrations, self.p.timers, battery);
                self.faults.note(result);
            }
            RequestKind::Service => self.root.runtime.service.open_connection(),
            RequestKind::FileOnly | RequestKind::Unselected => {}
        }
    }

    /// Stock 0x00d7b7c4 then 0x00d7b830.
    fn bind(&mut self, table: PublishedRegion) {
        match CallbackTable::published(table, 0) {
            Ok(table) => self.root.usb.bind_callbacks(table, &mut self.p.start.io),
            Err(error) => self.faults.note(Err::<(), _>(error)),
        }
        let result = self.root.usb.finalize(self.p.start);
        self.faults.note(result);
    }

    /// Stock 0x00d820e4.
    fn close_active(&mut self, state: ControllerState) {
        let result = self.root.connection.close_active(state, &mut self.root.usb, self.p.files, &mut SharedRtos(self.p.rtos));
        self.faults.note(result);
    }

    /// The 0x3404 and 0x340a cells of states 8, 9 and 10.
    fn finish(&mut self, target: ConnectionLevel, state: ControllerState, message: Queue36Message) -> PhaseWrite {
        use ControllerState as S;
        if state == S::ActiveVendor {
            if self.root.connection.active().is_some() {
                self.close_active(state);
                self.root.runtime.deferred.defer(message);
                return PhaseWrite::Unchanged;
            }
            if target == ConnectionLevel::Connected { self.root.runtime.deferred.set_blocked(false); }
            self.usb_shutdown();
            let kind = self.kind();
            if kind == RequestKind::Protocol {
                let _ = self.root.runtime.registrations.stop_short(self.p.timers);
                let _ = self.root.runtime.registrations.stop_long(self.p.timers);
                self.root.bridge.reset_flags();
            }
            let _ = self.root.runtime.registrations.stop_primary(self.p.timers);
            self.start_primary(PollPeriod::Connection);
            let runtime = &mut self.root.runtime;
            let mut parts = runtime.registrations.shutdown_parts(kind, &mut self.root.connection, &mut runtime.deferred, self.p.rtos);
            let _ = unregister_secondary(&mut parts.storage, self.p.timers);
        } else {
            self.usb_shutdown();
            self.start_primary(PollPeriod::Connection);
        }
        match target {
            ConnectionLevel::Connected => {
                if state == S::State8 { self.publish(StatusConsumer::Request, 0); }
                self.publish(StatusConsumer::Status, 0);
            }
            ConnectionLevel::Disconnected => {
                if state == S::State8 { self.publish(StatusConsumer::Request, -13); }
                self.publish(StatusConsumer::Completion, 0);
            }
        }
        if state != S::ActiveVendor { return PhaseWrite::Table; }
        PhaseWrite::Dynamic(match target { ConnectionLevel::Connected => S::State3, ConnectionLevel::Disconnected => S::State2 })
    }
}

#[cfg(target_arch = "arm")]
mod native {
    use crate::controller_registration::{TimerCallbackEntries, TimerEvent, TimerProducers};
    use xm1_power_services::shutdown_rtos::NativeShutdownRtos;

    pub static TIMER_PRODUCERS: TimerProducers = TimerProducers::new();

    fn fire(event: TimerEvent) { let _ = TIMER_PRODUCERS.fire(event, &mut NativeShutdownRtos); }
    /// Stock 0x00d813ac.
    unsafe extern "C" fn connection_poll_entry() { fire(TimerEvent::ConnectionPoll); }
    /// Stock 0x00d81344.
    unsafe extern "C" fn session_poll_entry() { fire(TimerEvent::SessionPoll); }
    /// Stock 0x00d81300.
    unsafe extern "C" fn short_transfer_entry() { fire(TimerEvent::ShortTransfer); }
    /// Stock 0x00d812bc.
    unsafe extern "C" fn long_transfer_entry() { fire(TimerEvent::LongTransfer); }

    pub static TIMER_ENTRIES: TimerCallbackEntries = TimerCallbackEntries {
        connection_poll: connection_poll_entry, session_poll: session_poll_entry,
        short_transfer: short_transfer_entry, long_transfer: long_transfer_entry,
    };
}
#[cfg(target_arch = "arm")]
pub use native::{TIMER_ENTRIES, TIMER_PRODUCERS};
