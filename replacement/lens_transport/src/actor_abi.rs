//! Native qualification adapter for the task 99 actor. Fixed addresses belong only to the
//! synthetic stock ABI.
#![cfg(all(feature = "native-actor", target_arch = "arm"))]

use crate::control_actor::ControlActor;
use crate::control_device::{ControlQueue, DeviceCallback, QueueFailure, QueuePacket, QueuePage};
use crate::control_publication::ControlOutbox;
use crate::control_runtime::{dispatch_task99, ControlPublications};
use crate::control_types::*;

const ACTOR: u32 = 0x0096_aff4;
const SHARED: u32 = 0x0095_e1d0;
const RING: u32 = 0x0020_4000;
const CONFIG: u32 = 0x0021_0000;
const FLAGS: u32 = 0x0021_0400;
const RING_CAPACITY: u32 = 64;
const COMPLETION_SHAPE: u32 = 0;

fn read(address: u32) -> u32 {
    unsafe { core::ptr::read_volatile(address as *const u32) }
}
fn write(address: u32, value: u32) {
    unsafe { core::ptr::write_volatile(address as *mut u32, value) }
}
fn read_byte(address: u32) -> u8 {
    unsafe { core::ptr::read_volatile(address as *const u8) }
}
fn write_byte(address: u32, value: u8) {
    unsafe { core::ptr::write_volatile(address as *mut u8, value) }
}

/// The ring is `count` then `count` records of five words.
fn record(tag: u32, a: u32, b: u32, c: u32, d: u32) {
    let count = read(RING);
    if count >= RING_CAPACITY {
        write(RING, count | 0x8000_0000);
        return;
    }
    let base = RING + 4 + count * 20;
    write(base, tag);
    write(base + 4, a);
    write(base + 8, b);
    write(base + 12, c);
    write(base + 16, d);
    write(RING, count + 1);
}

const TAG_SEND: u32 = 0x5345_4e44;
const TAG_RECV: u32 = 0x5245_4356;
const TAG_TIMR: u32 = 0x5449_4d52;
const TAG_KEY: u32 = 0x4b45_5900;
const TAG_DIAG: u32 = 0x4449_4147;
const TAG_FLAG: u32 = 0x464c_4147;
const TAG_DETM: u32 = 0x4445_544d;
const TAG_DETH: u32 = 0x4445_5448;
const TAG_DETO: u32 = 0x4445_544f;
const TAG_GPIO: u32 = 0x4750_494f;
const TAG_DELY: u32 = 0x4445_4c59;
const TAG_CALL: u32 = 0x4341_4c4c;
const TAG_ATTACH: u32 = 0x4154_5443;
const TAG_FINISH: u32 = 0x4649_4e49;

fn flags() -> u32 {
    read(FLAGS)
}

/// Stock 0x006c27a4 attaches the detection tick as timer IRQ source 120; the owned handler must
/// be in the Power leaf's registry before `xm1_power_timer_irq_attach` admits it.
#[cfg(not(feature = "defect-no-attach"))]
fn attach_detection_irq() {
    let handler = detection_irq as *const () as u32;
    if unsafe { xm1_power_owned_callback_register(handler, COMPLETION_SHAPE) } != 0 { return; }
    let mut token = 0;
    if unsafe { xm1_power_timer_irq_attach(120, handler, &mut token) } == 0 {
        record(TAG_ATTACH, 120, 0, 0, 0);
    }
}

/// The suite's deliberately wrong variant: Register without the detection IRQ attach. Only the
/// test-only `actor-defect-attach` ELF compiles this body.
#[cfg(feature = "defect-no-attach")]
fn attach_detection_irq() {}

/// The native suite attaches this word but delivers no interrupt; the vendor provider owns the
/// production detection handler.
extern "C" fn detection_irq() {}

/// The harness model of the Power leaf entries: the stock kernel stubs model the same boundary.
/// Registration is required before the attach succeeds, as in `callback_registry.rs`.
static REGISTERED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

#[no_mangle]
unsafe extern "C" fn xm1_power_owned_callback_register(_word: u32, shape: u32) -> i32 {
    if shape != COMPLETION_SHAPE { return -30; }
    REGISTERED.store(true, core::sync::atomic::Ordering::Release);
    0
}

#[no_mangle]
unsafe extern "C" fn xm1_power_timer_irq_attach(source: u32, _handler: u32, output: *mut u32) -> i32 {
    if source != 120 || !REGISTERED.load(core::sync::atomic::Ordering::Acquire) { return -255; }
    unsafe { *output = 1; }
    0
}

struct ActorIo;
impl ControlServices for ActorIo {
    fn delay_ticks(&mut self, ticks: TickCount) {
        record(TAG_DELY, ticks.0, 0, 0, 0);
    }
    fn post(&mut self, message: ControlMessage) -> Result<(), ControlError> {
        let (opcode, a, b) = encode(message);
        record(TAG_SEND, opcode, 99, a, b);
        Ok(())
    }
    fn complete(&mut self, _callback: CallbackToken, status: CompletionStatus, _data: &[u8]) {
        record(TAG_CALL, u32::from(status.0 < 0), 0, 0, 0);
    }
    fn notify(&mut self, target: WaitTarget) {
        record(TAG_FLAG, u32::from(target.task), target.bits, 0, 0);
    }
    fn key_release(&mut self, event: KeyEvent) {
        record(TAG_KEY, event.0, 0, 0, 0);
    }
    fn diagnostic(&mut self, category: u32, event: u32) {
        record(TAG_DIAG, category, event, 0, 0);
    }
    fn start_timer(&mut self, ticks: TickCount, repeating: bool, kind: ControlTimer) -> TimerToken {
        record(TAG_TIMR, ticks.0, u32::from(repeating), 0, 0);
        TimerToken(kind as u32 + 1)
    }
    fn cancel_timer(&mut self, timer: TimerToken) {
        record(TAG_TIMR, 0, 0, 0, 0);
    }
    fn set_detection_callback(&mut self, edge: DetectionEdge, enabled: bool) {
        match edge {
            DetectionEdge::Attach => if enabled { attach_detection_irq(); },
            DetectionEdge::Probe => record(TAG_DETH, 2, 0, 0, 0),
            DetectionEdge::Detach => {}
        }
    }
    fn reset_gpio(&mut self) {
        record(TAG_GPIO, 0, 0, 0, 0);
    }
}
impl ControlPower for ActorIo {
    fn set_detection_mode(&mut self, mode: DetectionMode) {
        record(TAG_DETM, mode as u32, 0, 0, 0);
    }
    fn detection_output(&mut self, mode: DetectionMode, completion: Option<DetectionCompletion>) {
        record(TAG_DETO, mode as u32, completion.map_or(3, |value| value as u32), 0, 0);
    }
}
impl ControlProfiles for ActorIo {
    fn calibration_profiles(&self) -> &[crate::control_device::CalibrationProfile] {
        &[]
    }
}
impl KeyPublicationSink for ActorIo {
    fn publish_key(&mut self, publication: crate::control_device::KeyPublication) {
        record(TAG_KEY, publication.key.0, publication.argument, 1, 0);
    }
}
impl ControlIo for ActorIo {
    fn read_mmio(&mut self, address: MmioAddress) -> u32 {
        read(address.0)
    }
    fn write_mmio(&mut self, address: MmioAddress, value: u32) {
        // The suite's deliberately wrong variant: the stop sequence without its 0xfff89340
        // writes. Only the test-only `actor-defect-stop` ELF compiles this refusal.
        #[cfg(feature = "defect-stop-mmio")]
        if address.0 == 0xfff8_9340 { return; }
        write(address.0, value)
    }
    fn update_mmio(&mut self, address: MmioAddress, clear: u32, set: u32) {
        #[cfg(feature = "defect-stop-mmio")]
        if address.0 == 0xfff8_9340 { return; }
        let value = read(address.0);
        write(address.0, (value & !clear) | set);
    }
}

/// The stock device posts every queue-98 transaction through the kernel send call.
struct ActorQueue;
impl ActorQueue {
    fn transaction(opcode: u32) {
        record(TAG_SEND, opcode, 98, 0, 0);
    }
}
impl ControlQueue for ActorQueue {
    fn initialize(&mut self, _completion: DeviceCallback, _observer: DeviceCallback) -> Result<(), QueueFailure> {
        Self::transaction(0x7200);
        Ok(())
    }
    fn submit(&mut self, packet: QueuePacket) -> Result<(), QueueFailure> {
        Self::transaction(packet.operation as u32);
        Ok(())
    }
    fn stop(&mut self, _completion: DeviceCallback) -> Result<(), QueueFailure> {
        Self::transaction(0x7201);
        Ok(())
    }
    fn write_configuration(&mut self, _page: QueuePage, configuration: &[u8; 128], _completion: DeviceCallback) -> Result<(), QueueFailure> {
        Self::transaction(0x7203);
        for (index, byte) in configuration.iter().enumerate() {
            write_byte(CONFIG + 0x200 + index as u32, *byte);
        }
        Ok(())
    }
    fn accept_page(&mut self, _accept: bool, _completion: DeviceCallback) -> Result<(), QueueFailure> {
        Self::transaction(0x7204);
        Ok(())
    }
    fn finish(&mut self, _model: crate::control_types::LensModelIdentifier) -> Result<(), QueueFailure> {
        record(TAG_FINISH, 0, 0, 0, 0);
        Ok(())
    }
    fn resume(&mut self, _completion: DeviceCallback) -> Result<(), QueueFailure> {
        Self::transaction(0x7205);
        Ok(())
    }
    fn suspend(&mut self, _completion: DeviceCallback) -> Result<(), QueueFailure> {
        Self::transaction(0x7206);
        Ok(())
    }
    fn poll(&mut self, _completion: DeviceCallback) -> Result<(), QueueFailure> {
        Self::transaction(0x720c);
        Ok(())
    }
    fn read_two(&mut self, _completion: DeviceCallback) -> Result<(), QueueFailure> {
        Self::transaction(0x7211);
        Ok(())
    }
}

/// The message record the stock actor receives: opcode, sender, destination, payload length and
/// payload pointer, followed by the payload words.
fn payload_word(index: usize) -> u32 {
    read(ACTOR + 0x10 + index as u32 * 4)
}

fn callback(index: usize) -> Option<CallbackToken> {
    let address = payload_word(index);
    if address == 0 { None } else { Some(CallbackToken(address)) }
}

fn response_kind(word: u32) -> ResponseKind {
    match word {
        1 => ResponseKind::Reply01, 2 => ResponseKind::Reply02, 3 => ResponseKind::Reply03,
        4 => ResponseKind::Reply04, 5 => ResponseKind::Reply05, 6 => ResponseKind::Reply06,
        7 => ResponseKind::Reply07, 8 => ResponseKind::Reply08, 9 => ResponseKind::Reply09,
        10 => ResponseKind::Reply10, 11 => ResponseKind::Reply11, 12 => ResponseKind::Reply12,
        13 => ResponseKind::Reply13, 14 => ResponseKind::Reply14, 15 => ResponseKind::Reply15,
        16 => ResponseKind::Reply16, 17 => ResponseKind::Reply17, 18 => ResponseKind::Reply18,
        19 => ResponseKind::Reply19, 20 => ResponseKind::Reply20, 21 => ResponseKind::Reply21,
        22 => ResponseKind::Reply22, 23 => ResponseKind::Reply23, 24 => ResponseKind::Reply24,
        25 => ResponseKind::Reply25, 26 => ResponseKind::Reply26, _ => ResponseKind::Reply27,
    }
}

fn decode(opcode: u16) -> Result<ControlMessage, ControlError> {
    let opcode = ControlOpcode::try_from(opcode)?;
    use ControlOpcode::*;
    let word = payload_word;
    let payload = match opcode {
        Register => ControlPayload::Register { enabled: word(0) != 0, completion: callback(1) },
        Setup => ControlPayload::Setup(SetupRequest {
            completion: callback(4),
            polling: word(2) != 0,
            mode: if word(1) != 0 { SetupMode::Primary } else if word(3) != 0 { SetupMode::Secondary } else { SetupMode::Other },
            owner: ControlOwner(word(0)),
        }),
        Teardown | Synchronize | StartPolling | Resume | Suspend => ControlPayload::Callback(callback(0)),
        Response => {
            let kind = response_kind(word(0));
            let status = CompletionStatus(word(1) as i32);
            let mut data = [0; 16];
            for (index, slot) in data.iter_mut().enumerate() {
                *slot = read_byte(ACTOR + 0x18 + index as u32);
            }
            let page = if matches!(kind, ResponseKind::Reply19 | ResponseKind::Reply21)
                && status == CompletionStatus::SUCCESS && u16::from_le_bytes([data[0], data[1]]) == 0
            {
                let mut bytes = [0; 128];
                let source = word(3);
                for (index, slot) in bytes.iter_mut().enumerate() {
                    *slot = read_byte(source + index as u32);
                }
                Some(CalibrationPage(bytes))
            } else { None };
            ControlPayload::Response(ControlResponse { kind, status, data, page })
        }
        ModeComplete => ControlPayload::ModeComplete(match word(0) {
            0xf => ModeCompletion::Resume, 0x10 => ModeCompletion::Suspend, _ => return Err(ControlError::InvalidPayload),
        }),
        InitializationFinished => ControlPayload::InitializationFinished(word(0) == 0),
        Poll => ControlPayload::Poll,
        BeginDetection | Ready | Finish | Attached | Detached | TeardownComplete => ControlPayload::Empty,
    };
    Ok(ControlMessage { opcode, payload })
}

/// Only the opcode and the first two payload words are encoded.
fn encode(message: ControlMessage) -> (u32, u32, u32) {
    let opcode = message.opcode as u32;
    match message.payload {
        ControlPayload::Register { enabled, completion } => (opcode, u32::from(enabled), completion.map_or(0, |token| token.0)),
        ControlPayload::Setup(request) => (opcode, request.owner.0, match request.mode { SetupMode::Primary => 1, SetupMode::Secondary => 3, SetupMode::Other => 0 }),
        ControlPayload::Callback(callback) => (opcode, callback.map_or(0, |token| token.0), 0),
        _ => (opcode, 0, 0),
    }
}

fn context_state() -> ControlState {
    match read(ACTOR) {
        1 => ControlState::Initializing, 2 => ControlState::Active, 3 => ControlState::Stopping,
        4 => ControlState::Suspended, 5 => ControlState::Detached, _ => ControlState::Idle,
    }
}

fn configured() -> ControlActor<'static> {
    static READINESS: SharedReadiness = SharedReadiness::new();
    let mut configuration = [0u8; 128];
    for (index, slot) in configuration.iter_mut().enumerate() {
        *slot = read_byte(CONFIG + index as u32);
    }
    let warm = flags() & 8 != 0;
    let mut actor = ControlActor::new(configuration, warm, &READINESS);
    let flag = flags();
    let context = actor.native_context();
    context.state = context_state();
    context.public_status = (flag >> 8) & 0xff;
    context.connected = flag & 1 != 0;
    context.attached = flag & 2 != 0;
    context.registered = flag & 4 != 0;
    context.polling = flag & 16 != 0;
    context.polling_supported = flag & 32 != 0;
    context.mode_resumed = flag & 64 != 0;
    context.first_response_received = flag & 128 != 0;
    context.warm_start = warm;
    context.gpio_preserved = flag & 0x200 != 0;
    context.capability = Capability(flag >> 16);
    if flag & 0x100 != 0 {
        context.readiness.publish();
    }
    actor
}

fn publication(actor: &ControlActor<'_>, publications: &ControlPublications) {
    let published = publications.native_snapshot(actor);
    for (index, word) in published.iter().enumerate() {
        write(SHARED + index as u32 * 4, *word);
    }
}

#[no_mangle]
pub extern "C" fn lens_actor_request() -> u32 {
    let opcode = read(ACTOR + 4) as u16;
    let Ok(message) = decode(opcode) else { return 0xffff_ffff };
    let mut actor = configured();
    let mut io = ActorIo;
    let mut queue = ActorQueue;
    let publications = ControlPublications::new();
    let mut outbox = ControlOutbox::new();
    let result = dispatch_task99(&mut actor, message, &mut io, &mut queue, &publications, &mut outbox);
    publication(&actor, &publications);
    write(ACTOR, actor.state() as u32);
    match result {
        Ok(()) => 0,
        Err(error) => error as u32 + 1,
    }
}

#[no_mangle]
pub extern "C" fn lens_actor_wake() -> u32 {
    record(TAG_RECV, 99, 0xffff_ffff, 0, 0);
    0
}

/// The timer continuation: dispatch the timer message the kernel delivered.
#[no_mangle]
pub extern "C" fn lens_actor_timer(kind: u32, token: u32) -> u32 {
    let mut actor = configured();
    let mut io = ActorIo;
    let mut queue = ActorQueue;
    let publications = ControlPublications::new();
    let mut outbox = ControlOutbox::new();
    let timer = match kind {
        0 => ControlTimer::Poll, 1 => ControlTimer::Detection,
        2 => ControlTimer::InitializationTimeoutOff, 3 => ControlTimer::InitializationTimeoutOn,
        _ => ControlTimer::RestrictedWait,
    };
    let result = dispatch_task99(
        &mut actor,
        ControlMessage { opcode: ControlOpcode::Poll, payload: ControlPayload::Timer { kind: timer, token: TimerToken(token) } },
        &mut io, &mut queue, &publications, &mut outbox);
    publication(&actor, &publications);
    write(ACTOR, actor.state() as u32);
    match result { Ok(()) => 0, Err(error) => error as u32 + 1 }
}

/// A queue-98 transaction the device posted is completed by the vendor adapter; the completion
/// re-enters the actor as a device callback.
#[no_mangle]
pub extern "C" fn lens_actor_device_callback(kind: u32, status: i32, word: u32) -> u32 {
    let mut actor = configured();
    let mut io = ActorIo;
    let mut queue = ActorQueue;
    let publications = ControlPublications::new();
    let mut outbox = ControlOutbox::new();
    let callback = match kind {
        0 => DeviceCallback::Initialized,
        1 => DeviceCallback::Observer,
        2 => DeviceCallback::DetectionOff,
        3 => DeviceCallback::DetectionOn,
        4 => DeviceCallback::Poll,
        5 => DeviceCallback::Teardown,
        6 => DeviceCallback::ModeResume,
        7 => DeviceCallback::ModeSuspend,
        8 => DeviceCallback::Configuration,
        9 => DeviceCallback::CapabilityPage,
        10 => DeviceCallback::Snapshot(ObserverKind::TwoBytes),
        11 => DeviceCallback::Snapshot(ObserverKind::ThreeBytes),
        _ => DeviceCallback::Snapshot(ObserverKind::NineBytes),
    };
    let mut data = [0; 16];
    data[..4].copy_from_slice(&word.to_le_bytes());
    let result = dispatch_task99(
        &mut actor,
        ControlMessage { opcode: ControlOpcode::Poll, payload: ControlPayload::DeviceCompletion { callback, status: CompletionStatus(status), data } },
        &mut io, &mut queue, &publications, &mut outbox);
    publication(&actor, &publications);
    write(ACTOR, actor.state() as u32);
    match result { Ok(()) => 0, Err(error) => error as u32 + 1 }
}
