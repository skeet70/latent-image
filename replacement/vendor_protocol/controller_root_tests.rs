use super::*;
use crate::commands::{BackendStatus, ByteCount, ConfigNamespace, DebugAddress, DebugQuery, PairCompletion};
use crate::controller::RequestHandle;
use crate::controller_registration::TimerEvent;
use crate::controller_runtime::{ConnectionSnapshot, Queue36Event, Queue36Payload, RequestArgument, RequestOffset, RequestRegistration};
use crate::filesystem_file_cpu::{CloseCompletion, FileError, FileHandle};
use crate::pictbridge_runtime::{DpsObject, Phase};
use crate::ptp_application::{open_session, DeviceProperties};
use crate::ptp_types::{PtpCommand, PtpOperationCode, PtpPropertyCode, PtpTransactionId};
use crate::service::ServiceBuffers;
use crate::transport::{Channel, ChannelLease, ControllerIo, PublishedAperture, ReceiveProgramming, Register};
use crate::transport_start_cpu::{ClockLevel, StartRegister, UsbMemory, USB_MEMORY_BYTES};
use irq_control::layout::GuestAddress as IrqAddress;
use irq_control::{Bank, Handler, SavedInterrupts, Table};
use scheduler::layout::{Domain, GuestAddress, KernelContext, LockRecord, Task};
use scheduler::{InterruptState, SwitchReason, TimerEntry};
use std::{cell::{Cell as StdCell, RefCell}, collections::{BTreeMap, VecDeque}};
use xm1_power_services::{
    device_timer_release::TimerPeriod,
    shutdown::{QueueId, RequestId, ServiceMessage, TaskId},
    shutdown_task7_8::RegistryHandle,
    shutdown_task7_8_transport::RegistrationRegistry,
};

struct Idle;
impl Backend for Idle {
    fn configuration_capacity(&self) -> u32 { 0 }
    fn identity_byte(&self, _: u32) -> Option<u8> { None }
    fn config_read(&mut self, _: ConfigNamespace, _: u32, _: &mut [u8]) -> BackendStatus { BackendStatus::Unavailable }
    fn config_write(&mut self, _: ConfigNamespace, _: u32, _: &[u8]) -> BackendStatus { BackendStatus::Unavailable }
    fn debug_query(&mut self, _: &mut [u8]) -> DebugQuery { DebugQuery { status: BackendStatus::Unavailable, actual: ByteCount(0), overflow: 0 } }
    fn debug_read(&mut self, _: DebugAddress, _: &mut [u8]) -> BackendStatus { BackendStatus::Unavailable }
    fn debug_write(&mut self, _: DebugAddress, _: &[u8]) -> BackendStatus { BackendStatus::Unavailable }
    fn debug_execute(&mut self, _: &[u8]) -> BackendStatus { BackendStatus::Unavailable }
}

#[derive(Default)]
struct Interrupts { words: RefCell<BTreeMap<u32, u32>>, masked: RefCell<bool> }
impl irq_control::Backend for Interrupts {
    fn load_word(&self, at: IrqAddress) -> u32 { *self.words.borrow().get(&at.0).unwrap_or(&0) }
    fn store_word(&self, at: IrqAddress, value: u32) { self.words.borrow_mut().insert(at.0, value); }
    fn mask_irq(&self) -> SavedInterrupts { let previous = *self.masked.borrow(); *self.masked.borrow_mut() = true; SavedInterrupts(u32::from(previous)) }
    fn restore_irq(&self, saved: SavedInterrupts) { *self.masked.borrow_mut() = saved.0 != 0; }
    fn mask_irq_fiq(&self) -> SavedInterrupts { self.mask_irq() }
    fn restore_irq_fiq(&self, saved: SavedInterrupts) { self.restore_irq(saved); }
    fn bank(&self) -> Bank { Bank::Primary }
}

#[derive(Default)]
struct Device { words: BTreeMap<u32, u32>, link: bool, clock: Vec<ClockLevel>, interrupt_resets: u32 }
impl ControllerIo for Device {
    fn read32(&mut self, register: Register) -> u32 { *self.words.get(&register.address()).unwrap_or(&0) }
    fn read16(&mut self, register: Register) -> u16 { self.read32(register) as u16 }
    fn write32(&mut self, register: Register, value: u32) {
        if register == Register::InterruptReset { self.interrupt_resets += 1; }
        self.words.insert(register.address(), value);
    }
    fn write16(&mut self, register: Register, value: u16) { self.words.insert(register.address(), u32::from(value)); }
}
impl StartIo for Device {
    fn start_read32(&mut self, register: StartRegister) -> u32 {
        if register == StartRegister::LinkStatus { return if self.link { 0x8000_0000 } else { 0 }; }
        *self.words.get(&(register.address() as u32)).unwrap_or(&0)
    }
    fn start_read16(&mut self, register: StartRegister) -> u16 { self.start_read32(register) as u16 }
    fn start_write32(&mut self, register: StartRegister, value: u32) { self.words.insert(register.address() as u32, value); }
    fn start_write16(&mut self, register: StartRegister, value: u16) { self.words.insert(register.address() as u32, u32::from(value)); }
    fn request_clock(&mut self, level: ClockLevel) -> ServiceStatus { self.clock.push(level); ServiceStatus(0) }
}

struct Counter(u32);
impl cpu::DelayCounter for Counter {
    fn reload(&mut self) -> u32 { 10_000_000 }
    fn current(&mut self) -> u32 { self.0 = if self.0 < 10_000 { 10_000_000 } else { self.0 - 10_000 }; self.0 }
}

struct Clock { memory: RefCell<BTreeMap<u32, u32>>, timer: StdCell<Option<(TimerEntry, u32)>> }
impl Clock {
    fn new() -> Self {
        let clock = Self { memory: RefCell::new(BTreeMap::new()), timer: StdCell::new(None) };
        let words = Domain::A.words();
        let context = words.context(Domain::A);
        let task = Task(words.task_table);
        scheduler::Machine::store_word(&clock, words.current_context, context.0.0);
        scheduler::Machine::store_word(&clock, words.running, task.0.0);
        scheduler::Machine::store_word(&clock, words.dispatch_enabled, 1);
        scheduler::Machine::store_word(&clock, task.definition(), 0x40060000);
        scheduler::Machine::store_word(&clock, GuestAddress(0x40060020), 1);
        scheduler::Machine::store_byte(&clock, task.state(), 1);
        scheduler::Machine::store_word(&clock, context.tick_now(), 0x40061000);
        scheduler::Machine::store_word(&clock, GuestAddress(0x40061000), 0);
        clock
    }
}
impl scheduler::Machine for Clock {
    fn load_word(&self, at: GuestAddress) -> u32 { *self.memory.borrow().get(&at.0).unwrap_or(&0) }
    fn store_word(&self, at: GuestAddress, value: u32) { self.memory.borrow_mut().insert(at.0, value); }
    fn load_byte(&self, at: GuestAddress) -> u8 { self.load_word(at) as u8 }
    fn store_byte(&self, at: GuestAddress, value: u8) { self.store_word(at, value as u32); }
    fn save_and_mask(&self) -> InterruptState { InterruptState(0) }
    fn restore(&self, _: InterruptState) {}
    fn masked(&self) -> bool { false }
    fn irq_enable(&self) {}
    fn irq_disable(&self) {}
    fn lock_acquire(&self, _: LockRecord) -> bool { true }
    fn lock_release(&self, _: LockRecord) {}
    fn remove_ready(&self, _: KernelContext, _: Task) -> bool { false }
    fn insert_ready(&self, _: KernelContext, task: Task) -> bool { self.store_byte(task.state(), 1); false }
    fn reset_task(&self, _: Task) { panic!("unexpected task reset") }
    fn restart_task(&self, _: KernelContext, _: Task) -> bool { panic!("unexpected task restart") }
    fn notify_dispatch(&self, _: KernelContext) -> bool { false }
    fn switch(&self, reason: SwitchReason) {
        assert_eq!(reason, SwitchReason::Delay);
        let (timer, expiry) = self.timer.take().expect("delay arms a timer");
        let task = Task(GuestAddress(self.load_word(timer.argument())));
        self.store_word(GuestAddress(0x40061000), expiry);
        scheduler::timeout::delay(self, Domain::A, task);
    }
    fn timer_insert(&self, _: KernelContext, entry: TimerEntry, expiry: u32) { assert!(self.timer.replace(Some((entry, expiry))).is_none()); }
    fn timer_remove(&self, _: KernelContext, entry: TimerEntry) { assert_eq!(self.timer.take().map(|value| value.0), Some(entry)); }
    fn timer_active(&self, _: KernelContext, entry: TimerEntry) -> bool { self.timer.get().map(|value| value.0) == Some(entry) }
}

#[derive(Default)]
struct Timers { next: u32, started: Vec<(TimerPeriod, TimerEvent)>, released: Vec<RegistryHandle> }
impl RegistrationRegistry for Timers {
    fn release(&mut self, handle: RegistryHandle) -> ServiceStatus { self.released.push(handle); ServiceStatus(0) }
}
impl TimerRegistry for Timers {
    fn start_periodic(&mut self, period: TimerPeriod, event: TimerEvent) -> Result<RegistryHandle, ServiceStatus> {
        self.started.push((period, event));
        self.next += 1;
        Ok(RegistryHandle(self.next))
    }
}

#[derive(Default)]
struct Platform { closed: Vec<Channel> }
impl ControllerIo for Platform {
    fn read32(&mut self, _: Register) -> u32 { 0 }
    fn read16(&mut self, _: Register) -> u16 { 0 }
    fn write32(&mut self, _: Register, _: u32) {}
    fn write16(&mut self, _: Register, _: u16) {}
}
impl TeardownPlatform for Platform {
    fn release_interrupt(&mut self, _: cpu::DriverHandle) -> Result<(), TransportError> { Ok(()) }
    fn release_channel_interrupt(&mut self, _: Channel) -> Result<(), TransportError> { Ok(()) }
    fn release_channel(&mut self, channel: Channel, _: cpu::DriverHandle) -> Result<(), TransportError> { self.closed.push(channel); Ok(()) }
}

#[derive(Default)]
struct Files { closed: Vec<FileHandle> }
impl ActiveFileClose for Files {
    fn close(&mut self, handle: FileHandle) -> Result<CloseCompletion, FileError> {
        self.closed.push(handle);
        Ok(CloseCompletion { handle, status: 0 })
    }
}

#[derive(Debug, Eq, PartialEq)]
enum Call { Start, Abort, Complete(ProtocolCompletion) }
#[derive(Default)]
struct Application { calls: Vec<Call> }
impl ProtocolFiles for Application {
    fn start(&mut self) { self.calls.push(Call::Start); }
    fn abort(&mut self) { self.calls.push(Call::Abort); }
}
impl ProtocolApplication for Application {
    fn complete(&mut self, completion: ProtocolCompletion) { self.calls.push(Call::Complete(completion)); }
}

#[derive(Default)]
struct Host { flags: u32, publications: Vec<Publication> }
impl RootHost for Host {
    fn battery_flags(&mut self) -> u32 { self.flags }
    fn print_publication(&mut self, publication: Publication) { self.publications.push(publication); }
}

#[derive(Default)]
struct Rtos { queue36: VecDeque<(RequestId, Vec<u8>)>, task7: Vec<(RequestId, Vec<u8>)>, fail_task7: bool }
impl ShutdownRtos for Rtos {
    fn current_task(&mut self) -> TaskId { TaskId(36) }
    fn post(&mut self, message: ServiceMessage<'_>) -> Result<(), KernelStatus> {
        if message.destination == QueueId(36) {
            self.queue36.push_back((message.operation, message.payload.to_vec()));
            return Ok(());
        }
        if self.fail_task7 { return Err(KernelStatus(-9)); }
        self.task7.push((message.operation, message.payload.to_vec()));
        Ok(())
    }
    fn set_event_flag(&mut self, _: u32, _: u32) -> Result<(), KernelStatus> { Ok(()) }
}

struct Feed<'a>(&'a RefCell<Rtos>);
impl Queue36Receive for Feed<'_> {
    fn receive(&mut self) -> Result<Option<Queue36Message>, Queue36ReceiveError> {
        let Some((operation, payload)) = self.0.borrow_mut().queue36.pop_front() else {
            return Err(Queue36ReceiveError::Kernel(KernelStatus(-50)));
        };
        Queue36Message::parse(operation.0, &payload).map_err(Queue36ReceiveError::Message)
    }
}

const REGISTRATION: RequestRegistration = RequestRegistration { argument: RequestArgument(0x015c_22a0), offset: RequestOffset(0x015c_22e4) };

fn tables() -> CallbackTables {
    CallbackTables {
        protocol: PublishedRegion::new(0x4100_0000, 32).unwrap(),
        service: PublishedRegion::new(0x4100_0100, 32).unwrap(),
        unselected: PublishedRegion::new(0x4100_0200, 32).unwrap(),
    }
}

#[derive(Default)]
struct Channels { taken: u32 }
impl SessionChannels for Channels {
    fn take(&mut self) -> [ChannelLease; 2] { self.taken += 1; leases() }
}

fn leases() -> [ChannelLease; 2] {
    let mut entries = [None; 4];
    let mut handles = cpu::HandleTable::new(&mut entries);
    let mut sequence = cpu::HandleSequence::new(0);
    let mut io = Platform::default();
    let (zero, first) = cpu::open_channel(Channel::Zero, &mut handles, &mut sequence, &mut io).unwrap();
    let (one, second) = cpu::open_channel(Channel::One, &mut handles, &mut sequence, &mut io).unwrap();
    [ChannelLease::new(zero, first), ChannelLease::new(one, second)]
}

struct Bench<'b, 'irq> {
    start: StartRuntime<'b, 'irq, Interrupts, Device, Counter>,
    timers: Timers,
    clock: Clock,
    channels: Channels,
    platform: Platform,
    files: Files,
    application: Application,
    host: Host,
    rtos: &'b RefCell<Rtos>,
    dps: DpsTransport<'b>,
}
impl<'b, 'irq> Bench<'b, 'irq> {
    fn providers<'a>(&mut self) -> RootProviders<'_, 'b, 'irq, 'b, 'a, Interrupts, Device, Counter, Clock, Timers, Platform, Channels, Files, Application, Host, Rtos> {
        RootProviders {
            start: &mut self.start, machine: &self.clock, domain: Domain::A, timers: &mut self.timers, channels: &mut self.channels,
            platform: &mut self.platform, files: &mut self.files, application: &mut self.application,
            host: &mut self.host, rtos: self.rtos, tables: tables(),
            identity: Identity { vendor: "FUJIFILM", product: "X-M1", serial: "1" }, dps: &mut self.dps, prepared: None,
        }
    }
    fn post(&self, event: Queue36Event, words: &[u32]) {
        let mut payload = [0; 24];
        for (index, word) in words.iter().enumerate() { payload[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes()); }
        Queue36Message { event, payload: Queue36Payload::parse(&payload).unwrap() }.publish(&mut *self.rtos.borrow_mut()).unwrap();
    }
    fn take_task7(&self) -> Vec<(RequestId, Vec<u8>)> { core::mem::take(&mut self.rtos.borrow_mut().task7) }
}

fn with_bench(test: impl FnOnce(&mut ControllerRoot<'_, Idle>, &mut Bench<'_, '_>)) {
    let backend = Interrupts::default();
    let table = Table::new(IrqAddress(0x4007_0000), Handler::from_static(0x42000));
    let mut entries = [None; 128];
    let mut sources = [u32::MAX; 128];
    let mut sequence = cpu::HandleSequence::new(0);
    let mut irq = cpu::IrqContext {
        backend: &backend, table, handles: cpu::HandleTable::new(&mut entries), sequence: &mut sequence,
        handlers: cpu::PublishedInterruptHandlers::from_handlers(Handler::from_static(0x43000), Handler::from_static(0x44000)),
        sources: &mut sources, channel_handles: [None, None],
    };
    let rtos = RefCell::new(Rtos::default());
    let mut objects: [DpsObject; 0] = [];
    let mut bench = Bench {
        start: StartRuntime { io: Device::default(), irq: &mut irq, timer: Counter(9_999_999), handler: Handler::from_static(0x45000) },
        timers: Timers::default(), clock: Clock::new(), channels: Channels::default(), platform: Platform::default(), files: Files::default(),
        application: Application::default(), host: Host::default(), rtos: &rtos,
        dps: DpsTransport::new(&mut objects, &[]),
    };
    let mut request = [0; 512];
    let mut reply = [0; 512];
    let mut control = [0; 256];
    let mut workspace = [0; 64];
    let aperture = PublishedAperture::publish(PublishedRegion::new(0x4104_0000, 64).unwrap(), &mut workspace).unwrap();
    let buffers = ServiceBuffers::new(
        (&mut request, PublishedRegion::new(0x4102_4000, 512).unwrap()), (&mut reply, PublishedRegion::new(0x4102_5000, 512).unwrap()),
        (&mut control, PublishedRegion::new(0x4102_6000, 256).unwrap()), aperture).unwrap();
    let producers = TimerProducers::new();
    let mut memory = [0; USB_MEMORY_BYTES];
    let mut root = ControllerRoot::new(Service::new(buffers, Idle, ReceiveProgramming::new(0, 0, 0)), &producers, UsbStart::new(UsbMemory::owned(&mut memory)));
    test(&mut root, &mut bench);
}

fn drive(root: &mut ControllerRoot<'_, Idle>, bench: &mut Bench<'_, '_>) {
    let rtos = bench.rtos;
    let stopped = root.run(&mut Feed(rtos), &mut bench.providers());
    assert!(matches!(stopped, RootError::Receive(Queue36ReceiveError::Kernel(KernelStatus(-50)))));
}

fn open(root: &mut ControllerRoot<'_, Idle>, bench: &mut Bench<'_, '_>, handle: u32) {
    bench.start.io.link = true;
    root.runtime.initialize_phase(&mut *bench.rtos.borrow_mut()).unwrap();
    root.runtime.initialize_completion(ConnectionSnapshot::Connected, &mut *bench.rtos.borrow_mut()).unwrap();
    root.runtime.request_open(RequestHandle(handle), REGISTRATION, &mut *bench.rtos.borrow_mut()).unwrap();
    drive(root, bench);
}

fn activate(root: &mut ControllerRoot<'_, Idle>, bench: &mut Bench<'_, '_>, handle: u32) {
    open(root, bench, handle);
    if handle == 2 {
        root.runtime.class_error(ServiceStatus(0), &mut *bench.rtos.borrow_mut()).unwrap();
        drive(root, bench);
    }
    assert_eq!(root.runtime.controller.state(), ControllerState::State8);
    root.runtime.class_initialized(RequestHandle(handle), &mut *bench.rtos.borrow_mut()).unwrap();
    bench.post(Queue36Event::BeginConnection, &[]);
    drive(root, bench);
    assert_eq!(root.runtime.controller.state(), ControllerState::ActiveVendor);
    bench.take_task7();
}

#[test]
fn service_request_starts_the_transport_binds_its_table_and_closes() {
    with_bench(|root, bench| {
        open(root, bench, 3);
        assert_eq!(root.runtime.controller.state(), ControllerState::State8);
        assert_eq!(bench.take_task7(), [
            (RequestId(0x1714), vec![21, 0, 0, 0, 0, 0, 0, 0]),
            (RequestId(0xb400), vec![0, 0, 0, 0]),
        ]);
        assert_eq!(bench.timers.started, [(TimerPeriod(25), TimerEvent::ConnectionPoll)]);
        assert_eq!(bench.timers.released, [RegistryHandle(1)]);
        assert_eq!(bench.start.io.clock, [ClockLevel::Active]);
        assert!(bench.application.calls.is_empty());

        root.runtime.class_initialized(RequestHandle(3), &mut *bench.rtos.borrow_mut()).unwrap();
        bench.post(Queue36Event::BeginConnection, &[]);
        drive(root, bench);
        assert_eq!(root.runtime.controller.state(), ControllerState::ActiveVendor);
        assert_eq!(bench.take_task7(), [(RequestId(0xb403), vec![3, 0, 0, 0, 0, 0, 0, 0])]);
        assert_eq!(root.connection.selected_buffer(), Some(tables().service));
        assert_eq!(bench.timers.started[1..], [(TimerPeriod(500), TimerEvent::ConnectionPoll)]);

        bench.post(Queue36Event::TransportStatus, &[]);
        root.runtime.request_close(&mut *bench.rtos.borrow_mut()).unwrap();
        drive(root, bench);
        assert!(root.pumped().is_empty());
        assert_eq!(bench.start.io.read16(Register::OutMaskMiddle), 0xfffb);
        assert_eq!(root.runtime.controller.state(), ControllerState::Stopped);
        assert_eq!(bench.platform.closed, [Channel::Zero, Channel::One]);
        assert_eq!(bench.timers.released, [RegistryHandle(1), RegistryHandle(2)]);
        assert_eq!(bench.take_task7(), [(RequestId(0xb401), vec![])]);
    });
}

#[test]
fn service_completion_before_its_pending_notice_is_replayed_once_pending_arrives() {
    with_bench(|root, bench| {
        activate(root, bench, 3);
        bench.post(Queue36Event::Completed, &[4, 0]);
        drive(root, bench);
        assert_eq!(root.runtime.controller.state(), ControllerState::ActiveVendor);
        bench.post(Queue36Event::Pending, &[]);
        drive(root, bench);
        assert_eq!(root.runtime.controller.state(), ControllerState::ActiveVendor);
        assert!(bench.application.calls.is_empty());
    });
}

#[test]
fn close_requested_during_the_file_abort_is_acknowledged_after_the_abort_completes() {
    with_bench(|root, bench| {
        open(root, bench, 2);
        assert_eq!(root.runtime.controller.state(), ControllerState::State4);
        assert!(bench.timers.released.is_empty());
        root.runtime.request_status(&mut *bench.rtos.borrow_mut()).unwrap();
        root.runtime.request_close(&mut *bench.rtos.borrow_mut()).unwrap();
        drive(root, bench);
        assert_eq!(root.runtime.controller.state(), ControllerState::State5);
        assert_eq!(bench.application.calls, [Call::Start, Call::Abort]);
        bench.take_task7();

        root.runtime.class_error(ServiceStatus(0), &mut *bench.rtos.borrow_mut()).unwrap();
        drive(root, bench);
        assert_eq!(root.runtime.controller.state(), ControllerState::Stopped);
        assert_eq!(bench.timers.released, [RegistryHandle(1)]);
        assert_eq!(bench.take_task7(), [
            (RequestId(0xb404), vec![0, 0, 0, 0]),
            (RequestId(0xb403), vec![2, 0, 0, 0, 0, 0, 0, 0]),
            (RequestId(0xb401), vec![]),
        ]);
    });
}

#[test]
fn handle_four_opens_the_file_phase_and_a_disconnect_there_aborts_into_state_six() {
    with_bench(|root, bench| {
        open(root, bench, 4);
        assert_eq!(root.runtime.controller.state(), ControllerState::State4);
        assert_eq!(bench.application.calls, [Call::Start]);
        bench.take_task7();

        root.runtime.update_connection_handle(CompletionHandle(20), &mut bench.start.io, &mut *bench.rtos.borrow_mut()).unwrap();
        drive(root, bench);
        assert_eq!(root.runtime.controller.state(), ControllerState::State6);
        assert_eq!(bench.application.calls, [Call::Start, Call::Abort]);
        assert_eq!(bench.start.io.clock, [ClockLevel::Active, ClockLevel::Idle]);
        assert!(bench.take_task7().is_empty());

        root.runtime.class_error(ServiceStatus(0), &mut *bench.rtos.borrow_mut()).unwrap();
        drive(root, bench);
        assert_eq!(root.runtime.controller.state(), ControllerState::State2);
        assert_eq!(bench.take_task7(), [
            (RequestId(0xb403), vec![4, 0, 0, 0, 0xf3, 0xff, 0xff, 0xff]),
            (RequestId(0x1714), vec![0, 0, 0, 0, 0, 0, 0, 0]),
        ]);
    });
}

#[test]
fn protocol_connection_owns_the_transfer_state_battery_and_timers_until_disconnect() {
    with_bench(|root, bench| {
        bench.host.flags = 16;
        activate(root, bench, 2);
        assert_eq!(root.bridge.phase(), Phase::Configure);
        assert_eq!(root.ptp.battery(), BatteryLevel::Medium);
        assert_eq!(root.connection.selected_buffer(), Some(tables().protocol));
        assert_eq!(bench.timers.started, [
            (TimerPeriod(25), TimerEvent::ConnectionPoll), (TimerPeriod(250), TimerEvent::ShortTransfer),
            (TimerPeriod(60_000), TimerEvent::LongTransfer), (TimerPeriod(50), TimerEvent::SessionPoll),
            (TimerPeriod(500), TimerEvent::ConnectionPoll),
        ]);

        let command = PtpCommand { operation: PtpOperationCode(0x1002), transaction: PtpTransactionId(1), parameters: [1, 0, 0, 0, 0], parameter_count: 1 };
        open_session(&mut root.ptp, &command);
        bench.host.flags = 4;
        bench.post(Queue36Event::LongTransfer, &[]);
        drive(root, bench);
        let mut value = [0; 1];
        DeviceProperties::new().value(PtpPropertyCode(0x5001), root.ptp.battery(), &mut value).unwrap();
        assert_eq!(value, [1]);
        assert!(root.ptp.take_event().is_some());

        bench.post(Queue36Event::Completed, &[7, 0, 512]);
        drive(root, bench);
        assert_eq!(root.runtime.controller.state(), ControllerState::ActiveVendor);
        assert_eq!(bench.application.calls[1..], [Call::Complete(ProtocolCompletion::Pair { kind: PairCompletion::Read, first: 0, second: 512 })]);

        bench.post(Queue36Event::Pending, &[]);
        bench.post(Queue36Event::SessionPoll, &[]);
        bench.post(Queue36Event::Completed, &[11, 0]);
        drive(root, bench);
        assert_eq!(root.runtime.controller.state(), ControllerState::ActiveVendor);
        assert_eq!(bench.take_task7(), [(RequestId(0x171a), (-12i32).to_le_bytes().to_vec())]);

        root.runtime.update_connection_handle(CompletionHandle(0), &mut bench.start.io, &mut *bench.rtos.borrow_mut()).unwrap();
        drive(root, bench);
        assert_eq!(root.runtime.controller.state(), ControllerState::State2);
        assert_eq!(root.bridge.phase(), Phase::Disabled);
        assert_eq!(bench.platform.closed, [Channel::Zero, Channel::One]);
        assert_eq!(bench.timers.released, [RegistryHandle(1), RegistryHandle(2), RegistryHandle(3), RegistryHandle(5), RegistryHandle(4)]);
        assert_eq!(bench.timers.started[5..], [(TimerPeriod(25), TimerEvent::ConnectionPoll)]);
        assert_eq!(bench.take_task7(), [
            (RequestId(0x171a), (-11i32).to_le_bytes().to_vec()),
            (RequestId(0x1714), vec![0, 0, 0, 0, 0, 0, 0, 0]),
        ]);
    });
}

#[test]
fn status_query_with_an_open_file_closes_it_rearms_the_transport_and_then_finishes() {
    with_bench(|root, bench| {
        activate(root, bench, 3);
        let handle = FileHandle::from_raw(7).unwrap();
        root.connection.set_active(handle);
        let rearmed = bench.start.io.interrupt_resets;
        root.runtime.request_status(&mut *bench.rtos.borrow_mut()).unwrap();
        drive(root, bench);
        assert_eq!(bench.files.closed, [handle]);
        assert_eq!(root.connection.active(), None);
        assert_eq!(bench.start.io.interrupt_resets, rearmed + 1);
        assert_eq!(root.runtime.controller.state(), ControllerState::State3);
        assert_eq!(bench.take_task7(), [(RequestId(0xb404), vec![0, 0, 0, 0])]);
    });
}

#[test]
fn print_cells_decode_their_payloads_and_release_deferred_messages() {
    with_bench(|root, bench| {
        activate(root, bench, 2);
        bench.post(Queue36Event::SubmitPrint, &[0x1234, 0x0d00_0000]);
        bench.post(Queue36Event::PrintResponse, &[0x9001_0000]);
        bench.post(Queue36Event::ShortTransfer, &[]);
        drive(root, bench);
        assert!(root.bridge.immediate_abort_requested());
        assert!(bench.host.publications.is_empty());

        bench.post(Queue36Event::Pending, &[]);
        bench.post(Queue36Event::PrintResponse, &[0]);
        bench.post(Queue36Event::Completed, &[11, 0]);
        drive(root, bench);
        assert_eq!(root.runtime.controller.state(), ControllerState::ActiveVendor);
        assert!(!root.bridge.immediate_abort_requested());
    });
}

#[test]
fn sixteen_differing_link_polls_publish_the_connection_and_vote_the_clock() {
    with_bench(|root, bench| {
        root.runtime.initialize_phase(&mut *bench.rtos.borrow_mut()).unwrap();
        root.runtime.initialize_completion(ConnectionSnapshot::Disconnected, &mut *bench.rtos.borrow_mut()).unwrap();
        drive(root, bench);
        assert_eq!(root.runtime.controller.state(), ControllerState::State2);
        bench.take_task7();
        bench.start.io.link = true;
        for _ in 0..16 {
            assert_eq!(root.runtime.controller.state(), ControllerState::State2);
            root.runtime.registrations.fire(TimerEvent::ConnectionPoll, &mut *bench.rtos.borrow_mut()).unwrap();
            drive(root, bench);
        }
        assert_eq!(root.runtime.controller.state(), ControllerState::State3);
        assert_eq!(bench.start.io.clock, [ClockLevel::Active, ClockLevel::Idle, ClockLevel::Active]);
        assert_eq!(bench.take_task7(), [(RequestId(0x1714), vec![1, 0, 0, 0, 0, 0, 0, 0])]);
    });
}

#[test]
fn class_registrars_carry_the_mode_and_status_into_queue36() {
    with_bench(|root, bench| {
        root.runtime.class_initialized(RequestHandle(4), &mut *bench.rtos.borrow_mut()).unwrap();
        root.runtime.class_error(ServiceStatus(-3), &mut *bench.rtos.borrow_mut()).unwrap();
        assert_eq!(root.runtime.controller.request_handle(), RequestHandle(4));
        let mut status = vec![0; 24];
        status[..4].copy_from_slice(&(-3i32).to_le_bytes());
        let queued: Vec<_> = bench.rtos.borrow().queue36.iter().cloned().collect();
        assert_eq!(queued, [(RequestId(0x340b), vec![0; 24]), (RequestId(0x340c), status)]);
    });
}

#[test]
fn print_submission_reads_the_file_copies_and_inline_date_text() {
    let mut bytes = [0; 24];
    bytes[..4].copy_from_slice(&0x1234u32.to_le_bytes());
    bytes[4..8].copy_from_slice(&3u32.to_le_bytes());
    bytes[8..18].copy_from_slice(b"2013/01/02");
    let payload = Queue36Payload::parse(&bytes).unwrap();
    let request = print_request(&payload).unwrap();
    assert_eq!((request.file, request.copies, request.date), (FileId(0x1234), Copies::Count(3), Some("2013/01/02")));
    bytes[4..8].copy_from_slice(&0x0d00_0000u32.to_le_bytes());
    bytes[8..23].copy_from_slice(b"123456789012345");
    let payload = Queue36Payload::parse(&bytes).unwrap();
    let request = print_request(&payload).unwrap();
    assert_eq!((request.copies, request.date), (Copies::Mpo, Some("123456789012345")));
    bytes[8] = 0;
    let payload = Queue36Payload::parse(&bytes).unwrap();
    assert_eq!(print_request(&payload).unwrap().date, None);
}

#[test]
fn a_second_session_starts_after_the_first_was_torn_down() {
    with_bench(|root, bench| {
        activate(root, bench, 3);
        root.runtime.request_status(&mut *bench.rtos.borrow_mut()).unwrap();
        drive(root, bench);
        assert_eq!(root.runtime.controller.state(), ControllerState::State3);
        assert_eq!(bench.channels.taken, 1);
        bench.take_task7();
        root.runtime.request_open(RequestHandle(3), REGISTRATION, &mut *bench.rtos.borrow_mut()).unwrap();
        drive(root, bench);
        assert_eq!(root.runtime.controller.state(), ControllerState::State8);
        assert!(bench.take_task7().is_empty());
        assert!(root.take_fault().is_none());
    });
}

#[test]
fn a_failed_publication_is_reported_and_the_cell_still_completes() {
    with_bench(|root, bench| {
        bench.rtos.borrow_mut().fail_task7 = true;
        root.runtime.initialize_phase(&mut *bench.rtos.borrow_mut()).unwrap();
        root.runtime.initialize_completion(ConnectionSnapshot::Disconnected, &mut *bench.rtos.borrow_mut()).unwrap();
        drive(root, bench);
        assert_eq!(root.runtime.controller.state(), ControllerState::State2);
        assert!(matches!(root.take_fault(), Some(RootError::Kernel(KernelStatus(-9)))));
        assert!(root.take_fault().is_none());
    });
}

fn set_pending(bench: &mut Bench<'_, '_>, bit: u8) {
    let (register, shift) = match bit {
        0..=15 => (StartRegister::PendingLow, bit),
        16..=31 => (StartRegister::PendingMiddle, bit - 16),
        _ => (StartRegister::PendingHigh, bit - 32),
    };
    let value = bench.start.io.start_read16(register);
    bench.start.io.start_write16(register, value | (1u16 << shift));
}

fn clear_pending(bench: &mut Bench<'_, '_>) {
    bench.start.io.start_write16(StartRegister::PendingLow, 0);
    bench.start.io.start_write16(StartRegister::PendingMiddle, 0);
    bench.start.io.start_write16(StartRegister::PendingHigh, 0);
}

#[test]
fn pump_reports_each_distinguished_bit_in_stock_order() {
    with_bench(|root, bench| {
        activate(root, bench, 3);
        for (bit, expected) in [
            (0u8, UsbEvent::ControlTransferZero), (1, UsbEvent::ControlTransferOne),
            (2, UsbEvent::ChannelTwoTransfer), (3, UsbEvent::ChannelTwoRequest),
            (4, UsbEvent::ChannelThree), (13, UsbEvent::ControlPacket),
            (18, UsbEvent::ChannelOneTransfer), (31, UsbEvent::BusState),
            (33, UsbEvent::ControlComplete), (38, UsbEvent::ChannelOneComplete),
            (44, UsbEvent::TransferReset),
        ] {
            root.usb.set_event_mask([0, 0, 0]);
            set_pending(bench, bit);
            bench.post(Queue36Event::TransportStatus, &[]);
            drive(root, bench);
            assert_eq!(root.pumped().events().collect::<Vec<_>>(), [expected], "pending bit {bit}");
            assert_eq!(root.runtime.controller.state(), ControllerState::ActiveVendor);
            clear_pending(bench);
        }
    });
}

#[test]
fn pump_completion_reset_disconnects_the_active_session() {
    with_bench(|root, bench| {
        activate(root, bench, 3);
        bench.take_task7();
        root.usb.set_event_mask([0, 0, 0]);
        set_pending(bench, 46);
        bench.post(Queue36Event::TransportStatus, &[]);
        drive(root, bench);
        assert_eq!(root.pumped().events().collect::<Vec<_>>(), [UsbEvent::CompletionReset]);
        assert_eq!(root.runtime.controller.state(), ControllerState::State2);
        assert_eq!(bench.start.io.clock.last(), Some(&ClockLevel::Idle));
        assert_eq!(bench.take_task7(), [(RequestId(0x1714), vec![0; 8])]);
    });
}

#[test]
fn pump_reports_all_set_bits_in_the_stock_check_order() {
    with_bench(|root, bench| {
        activate(root, bench, 3);
        root.usb.set_event_mask([0, 0, 0]);
        for bit in [46, 44, 31, 33, 13, 0, 1, 4, 38, 3, 2, 18] { set_pending(bench, bit); }
        bench.post(Queue36Event::TransportStatus, &[]);
        drive(root, bench);
        assert_eq!(root.pumped().events().collect::<Vec<_>>(), UsbEvent::ORDER);
    });
}

#[test]
fn pump_acknowledges_every_reported_bit_and_bit_eighteen() {
    with_bench(|root, bench| {
        activate(root, bench, 3);
        root.usb.set_event_mask([0, 0, 0]);
        set_pending(bench, 46);
        set_pending(bench, 13);
        bench.post(Queue36Event::TransportStatus, &[]);
        drive(root, bench);
        assert_eq!(bench.start.io.read16(Register::OutMaskLow), 0xdfff);
        assert_eq!(bench.start.io.read16(Register::OutMaskMiddle), 0xfffb);
        assert_eq!(bench.start.io.read16(Register::OutMaskHigh), 0xbfff);
    });
}

#[test]
fn pump_ignores_bits_the_shadow_mask_suppresses() {
    with_bench(|root, bench| {
        activate(root, bench, 3);
        set_pending(bench, 3);
        bench.post(Queue36Event::TransportStatus, &[]);
        drive(root, bench);
        assert!(root.pumped().is_empty());
        assert_eq!(root.runtime.controller.state(), ControllerState::ActiveVendor);
        assert_eq!(bench.start.io.read16(Register::OutMaskMiddle), 0xfffb);
    });
}

#[test]
fn pump_transfer_reset_reinitializes_the_endpoint_registers() {
    with_bench(|root, bench| {
        activate(root, bench, 3);
        root.usb.set_event_mask([0, 0, 0]);
        bench.start.io.write16(Register::InMaskHigh, 0xffff);
        bench.start.io.start_write16(StartRegister::EndpointControl, 1);
        bench.start.io.write16(Register::BusSpeed, 0x4000);
        bench.start.io.write32(Register::OutControl, 0x2000);
        set_pending(bench, 44);
        bench.post(Queue36Event::TransportStatus, &[]);
        drive(root, bench);
        assert_eq!(root.pumped().events().collect::<Vec<_>>(), [UsbEvent::TransferReset]);
        assert_eq!(bench.start.io.read32(Register::OutStart), 0);
        assert_eq!(bench.start.io.read16(Register::InMaskHigh), 0xfffc);
        assert_eq!(bench.start.io.start_read16(StartRegister::EndpointControl), 0xd);
        assert_eq!(bench.start.io.read32(Register::OutInitialize), 0x101);
        assert_eq!(bench.start.io.read16(Register::EndpointPulse), 0xfff);
        assert_eq!(bench.start.io.read32(Register::OutControl), 0x3000);
        assert_eq!(root.runtime.controller.state(), ControllerState::ActiveVendor);
    });
}
