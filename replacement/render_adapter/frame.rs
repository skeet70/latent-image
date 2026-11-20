//! Owned stage-0 body of VGLIB's `0x5902` frame step `0x6d7920` (reports/display-render-contracts/vglib-frame.md).
//! `frame.S` reproduces the stock prologue and the lock `0x6d9478` before calling `rb_render_frame_apply`, and resumes
//! stock at `0x6d7930` with that frame and lock when it returns `Fallback`. Every refusal happens before the first
//! stock call after the lock, so a fallback leaves stock state as the stock step finds it at `0x6d7930`. On `Owned`,
//! `frame.S` calls `rb_render_frame_commit`, the `0xfefe` wait and `rb_render_frame_complete`.

use crate::render_commands::commands::DrawCommand;
use crate::render_commands::producer::CommandBuffer;
use crate::render_commands::publisher::{BufferIndex, BufferState, ContextId, DrainState, FrameId, Publisher, PublisherError, SubmissionError};
use crate::render_commands::resources::{DataAddress, DataRegion, ResourceMemory};
use crate::render_commands::scanout_latch::{ScanoutAddress, ScanoutLatch};
use crate::render_commands::stock_objects::{ObjectPool, LISTS_BASE, LIST_BYTES, POOL_BASE, POOL_BYTES};
use crate::render_commands::vector_raster::{StrokePrimitive, Vertex};
use crate::render_commands::vglib_scene::{render_osd, Scene, SceneError};
use crate::render_commands::SurfaceId;
use core::ptr::{read_volatile, write_volatile};

/// VGLIB render state `R`.
const RENDER_STATE: u32 = 0x008e_2098;
const STATUS: u32 = RENDER_STATE + 0x4c;
const REQUESTED_MODE: u32 = RENDER_STATE + 0x7c;
const CURRENT_MODE: u32 = RENDER_STATE + 0x80;
/// `S(s,b)` at `R+0x90+8s+4b`; stage 0 only.
const SURFACES: u32 = RENDER_STATE + 0x90;
/// `O(s,b)` at `R+0xc0+8s+4b`; stage 0 only.
const IMAGES: u32 = RENDER_STATE + 0xc0;
/// Frame-completion slots `0x8e21b0..0x8e21c4`.
const FRAME_CALLBACKS: u32 = RENDER_STATE + 0x118;
const CALLBACK_VECTOR: u32 = RENDER_STATE + 0x130;
/// Float `0x6d724c` compares with zero when no draw returned nonzero (`I4:6d72ec-6d730c`).
const RESULT_FLOOR: u32 = RENDER_STATE + 0x154;
const VISITED: u32 = RENDER_STATE + 0x158;
const RENDER_RESULT: u32 = RENDER_STATE + 0x15c;
const FRONT: u32 = RENDER_STATE + 0x160;
/// Option value restored after the frame (`I4:6d7d5c-6d7d70`).
const OPTION_RESTORE: u32 = 0x008e_2094;
/// Stage geometry `G`: 16-byte records, width/height halfwords then the two buffer addresses `+4/+8` that
/// `render_manage_surface` stores at `O(s,b)+0x1c` (`I4:6d7124-6d7170`).
const GEOMETRY: u32 = 0x0082_8d9c;
const IMAGE_ADDRESS: u32 = 0x1c;
const OPTION_GROUP: u32 = 0x30d;
const OPTION_INDEX: u32 = 0x16;
const OPTION_RENDER: u32 = 9;
/// Engine-1 OSD address register `D(1)+0xb4`.
const OSD_SCANOUT_REGISTER: u32 = 0xfff6_e0b4;

#[repr(C)]
struct OneShotRequest { callback: unsafe extern "C" fn(), retention: u32, order: u32, phase: u32 }
/// `0x6d80f0`'s one-shot request `{0x6d7f30, 0, 2, 0}` (`I4:6d8104-6d8118`) with the owned delivery. Registration
/// copies the request (`I4:6c73d0-6c7424`).
static ONE_SHOT: OneShotRequest = OneShotRequest { callback: rb_render_frame_scanout_delivery, retention: 0, order: 2, phase: 0 };
/// Registration's handle output, written under the interrupt lock and never read.
static mut ONE_SHOT_HANDLE: u32 = 0;

/// Guest RAM and its `0x40000000` alias as resource regions; only addresses the scene walk follows are read.
const RAM_LOW: (u32, usize) = (0x0000_1000, 0x0400_0000 - 0x1000);
const RAM_ALIAS: (u32, usize) = (0x4000_0000, 0x0400_0000);

const COMMAND_CAPACITY: usize = 96;
const VERTEX_CAPACITY: usize = 512;
/// The Run-3 shooting screen's largest shape item needs 212 stroke primitives.
const STROKE_CAPACITY: usize = 256;
/// Bytes of the frame statics in the bank engine region, excluding code.
const FRAME_STATIC_BYTES: usize = COMMAND_CAPACITY * core::mem::size_of::<DrawCommand<'static>>()
    + VERTEX_CAPACITY * core::mem::size_of::<Vertex>()
    + STROKE_CAPACITY * core::mem::size_of::<StrokePrimitive>()
    + core::mem::size_of::<ObjectPool<'static>>()
    + core::mem::size_of::<FrameState>()
    + core::mem::size_of::<FrameRecord>();
const _: () = assert!(FRAME_STATIC_BYTES <= 0x10000, "frame statics exceed their engine-region budget");

#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameOutcome { Fallback = 0, Owned = 1 }

/// Index into `FrameRecord::fallbacks`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FallbackReason {
    /// Requested mode differs from current: stock performs the surface transition.
    ModeTransition = 0,
    /// Current mode 8 or 10 renders two passes.
    DualOutput = 1,
    /// `render_stage_index(current)` is not 0.
    Stage = 2,
    /// A stage-0 surface or image object is zero.
    MissingSurface = 3,
    /// Stage-0 geometry is not 720 x 480, or an image's display address is not its `G` buffer word.
    Geometry = 4,
    /// The display addresses differ from the publisher's buffer tokens.
    BufferMoved = 5,
    ObjectPool = 6,
    /// The scene did not decode or lower completely.
    Scene = 7,
    CommandCapacity = 8,
    /// The back buffer is not `Free` in the publisher.
    PublisherBusy = 9,
    Raster = 10,
    Publisher = 11,
}
const FALLBACK_REASONS: usize = 12;

const RECORD_MAGIC: u32 = u32::from_le_bytes(*b"RBFR");
const RECORD_LAYOUT: u32 = 1;
const STATE_FREE: u32 = 0;
const STATE_DRAWING: u32 = 1;
const STATE_QUEUED: u32 = 2;
const STATE_SCANNING: u32 = 3;

/// Harness-readable publication record; layout in README.md. Sequences and context are 0 when absent.
#[repr(C)]
pub struct FrameRecord {
    magic: u32,
    layout: u32,
    context: u64,
    last_sequence: u64,
    acknowledged_sequence: u64,
    buffer_sequence: [u64; 2],
    buffer_state: [u32; 2],
    drain: u32,
    owned_frames: u32,
    wait_failures: u32,
    fallbacks: [u32; FALLBACK_REASONS],
}
const FRAME_RECORD_BYTES: usize = 0x78;
const _: () = assert!(core::mem::size_of::<FrameRecord>() == FRAME_RECORD_BYTES
    && core::mem::offset_of!(FrameRecord, context) == 0x08
    && core::mem::offset_of!(FrameRecord, acknowledged_sequence) == 0x18
    && core::mem::offset_of!(FrameRecord, buffer_sequence) == 0x20
    && core::mem::offset_of!(FrameRecord, buffer_state) == 0x30
    && core::mem::offset_of!(FrameRecord, drain) == 0x38
    && core::mem::offset_of!(FrameRecord, wait_failures) == 0x40
    && core::mem::offset_of!(FrameRecord, fallbacks) == 0x44);

#[no_mangle]
#[allow(non_upper_case_globals)]
pub static mut rb_render_frame_record: FrameRecord = FrameRecord {
    magic: RECORD_MAGIC,
    layout: RECORD_LAYOUT,
    context: 0,
    last_sequence: 0,
    acknowledged_sequence: 0,
    buffer_sequence: [0; 2],
    buffer_state: [STATE_FREE; 2],
    drain: 0,
    owned_frames: 0,
    wait_failures: 0,
    fallbacks: [0; FALLBACK_REASONS],
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BufferAddresses([u32; 2]);

struct FrameState {
    publisher: Option<Publisher>,
    buffers: Option<BufferAddresses>,
    /// Front byte after the last owned frame.
    owned_front: Option<u8>,
    /// A stock frame step ran since the last owned frame.
    stock_since: bool,
    next_context: u64,
    /// The frame `rb_render_frame_apply` admitted for `rb_render_frame_commit`.
    prepared: Option<Prepared>,
}

static mut STATE: FrameState = FrameState { publisher: None, buffers: None, owned_front: None, stock_since: false, next_context: 1, prepared: None };
static mut COMMANDS: [DrawCommand<'static>; COMMAND_CAPACITY] = [DrawCommand::ResetClip; COMMAND_CAPACITY];
static mut VERTICES: [Vertex; VERTEX_CAPACITY] = [Vertex::EMPTY; VERTEX_CAPACITY];
static mut STROKES: [StrokePrimitive; STROKE_CAPACITY] = [StrokePrimitive::EMPTY; STROKE_CAPACITY];
static mut POOL: ObjectPool<'static> = ObjectPool::empty();
static mut REGIONS: [DataRegion<'static>; 2] = [DataRegion { base: DataAddress(0), bytes: &[] }; 2];
/// Accessed only through `with_latch`.
static mut LATCH: ScanoutLatch = ScanoutLatch::new();

unsafe extern "C" {
    fn rb_render_call_6d9494();
    fn rb_render_call_6499d0(group: u32, index: u32, value: u32) -> i32;
    fn rb_render_call_6d76ec(geometry: u32, address: u32, mode: u32, result: u32);
    fn rb_render_call_6cfc68(vector: u32);
    fn xm1_kernel_critical_enter() -> u32;
    fn xm1_kernel_critical_exit(saved: u32);
    /// Called directly, without a trampoline frame, from the interrupt-context bridge and delivery.
    fn rb_render_stock_6c7318(request: *const u32, handle: *mut u32) -> i32;
    fn rb_render_stock_6d7f30() -> i32;
}

/// Runs `f` under the stock interrupt lock `0x7235d8`/`0x7236b0`, which serializes the latch's three contexts.
fn with_latch<R>(f: impl FnOnce(&mut ScanoutLatch) -> R) -> R {
    let token = unsafe { xm1_kernel_critical_enter() };
    let result = f(unsafe { &mut *(&raw mut LATCH) });
    unsafe { xm1_kernel_critical_exit(token); }
    result
}

fn word(address: u32) -> u32 { unsafe { read_volatile(address as *const u32) } }
fn half(address: u32) -> u16 { unsafe { read_volatile(address as *const u16) } }
fn set_word(address: u32, value: u32) { unsafe { write_volatile(address as *mut u32, value) } }

/// `render_stage_index` (`I4:6d7048-6d7098`).
fn render_stage_index(mode: u32) -> u32 {
    match mode { 2 => 1, 3 => 2, 5 => 3, 6 => 4, 7 => 5, _ => 0 }
}

/// `render_next_buffer` (`I4:6d701c-6d7030`): integer `front+1` when at most 1, else 0.
fn render_next_buffer(front: u8) -> BufferIndex {
    if u32::from(front) + 1 <= 1 { BufferIndex::Second } else { BufferIndex::First }
}

/// Front-byte advance after the hand-off (`I4:6d7d38-6d7d58`).
fn advanced_front(front: u8) -> u8 {
    let next = front.wrapping_add(1);
    if next > 1 { 0 } else { next }
}

/// A begun frame whose commands occupy `COMMANDS[..commands]`.
struct Decided {
    frame: FrameId,
    buffer: ScanoutAddress,
    commands: usize,
    visible: u32,
    render_result: u32,
}

struct Prepared {
    frame: FrameId,
    buffer: ScanoutAddress,
    visible: u32,
    render_result: u32,
}

/// Owned work before the first stock call after the lock; `Owned` leaves the frame for `rb_render_frame_commit`.
#[no_mangle]
pub unsafe extern "C" fn rb_render_frame_apply() -> FrameOutcome {
    let state = unsafe { &mut *(&raw mut STATE) };
    match decide(state).and_then(|decided| rasterize(state, decided)) {
        Ok(prepared) => {
            state.prepared = Some(prepared);
            FrameOutcome::Owned
        }
        Err(reason) => {
            state.stock_since = true;
            let record = unsafe { &mut *(&raw mut rb_render_frame_record) };
            record.fallbacks[reason as usize] = record.fallbacks[reason as usize].wrapping_add(1);
            publish_record(state);
            FrameOutcome::Fallback
        }
    }
}

/// Called by `frame.S` after `Owned`, from the 48-byte stock frame, so the stock services run above that frame only.
#[no_mangle]
pub unsafe extern "C" fn rb_render_frame_commit() {
    let state = unsafe { &mut *(&raw mut STATE) };
    if let Some(prepared) = state.prepared.take() { commit(state, prepared); }
}

/// Called by `frame.S` with the `0xfefe` wait's result: the stock sequence after the wait (`I4:6d7da4-6d7df0`).
#[no_mangle]
pub unsafe extern "C" fn rb_render_frame_complete(waited: i32) {
    let state = unsafe { &mut *(&raw mut STATE) };
    let record = unsafe { &mut *(&raw mut rb_render_frame_record) };
    if waited != 0 { record.wait_failures = record.wait_failures.wrapping_add(1); }
    apply_latch(state);
    for slot in 0..6 {
        let address = FRAME_CALLBACKS + 4 * slot;
        let callback = word(address);
        if callback != 0 {
            set_word(address, 0);
            let callback: extern "C" fn() = unsafe { core::mem::transmute(callback as usize) };
            callback();
        }
    }
    set_word(STATUS, 0);
    publish_record(state);
    unsafe { rb_render_call_6d9494(); }
}

/// Queued at `R+0x130` in place of `0x6d80f0`; the engine-1 commit drain calls it. Registration and the latch update
/// share one interrupt-lock interval, so the delivery cannot run between them.
#[no_mangle]
pub unsafe extern "C" fn rb_render_frame_scanout_bridge() {
    let register = ScanoutAddress::new(word(OSD_SCANOUT_REGISTER));
    let token = unsafe { xm1_kernel_critical_enter() };
    let registered = unsafe { rb_render_stock_6c7318(&raw const ONE_SHOT as *const u32, &raw mut ONE_SHOT_HANDLE) } >= 0;
    unsafe { (*(&raw mut LATCH)).commit_pass(register, registered); }
    unsafe { xm1_kernel_critical_exit(token); }
}

/// The one-shot the bridge registers; posts `0xfefe` through `0x6d7f30` after the latch records the delivery.
#[no_mangle]
pub unsafe extern "C" fn rb_render_frame_scanout_delivery() {
    let token = unsafe { xm1_kernel_critical_enter() };
    unsafe { (*(&raw mut LATCH)).delivered(); }
    unsafe { xm1_kernel_critical_exit(token); }
    unsafe { rb_render_stock_6d7f30(); }
}

/// Separate frames keep the scene decode and the raster off each other's stack.
#[inline(never)]
fn decide(state: &mut FrameState) -> Result<Decided, FallbackReason> {
    let current = word(CURRENT_MODE);
    if word(REQUESTED_MODE) != current { return Err(FallbackReason::ModeTransition); }
    if matches!(current, 8 | 10) { return Err(FallbackReason::DualOutput); }
    if render_stage_index(current) != 0 { return Err(FallbackReason::Stage); }
    let images = [word(IMAGES), word(IMAGES + 4)];
    if word(SURFACES) == 0 || word(SURFACES + 4) == 0 || images.contains(&0) { return Err(FallbackReason::MissingSurface); }
    let (width, height) = SurfaceId::Osd.dimensions();
    if usize::from(half(GEOMETRY)) != width || usize::from(half(GEOMETRY + 2)) != height { return Err(FallbackReason::Geometry); }
    let addresses = BufferAddresses([word(images[0] + IMAGE_ADDRESS), word(images[1] + IMAGE_ADDRESS)]);
    if addresses.0 != [word(GEOMETRY + 4), word(GEOMETRY + 8)] || !disjoint(addresses) { return Err(FallbackReason::Geometry); }
    if state.buffers.is_some_and(|held| held != addresses) { return Err(FallbackReason::BufferMoved); }

    let pool = unsafe { &mut *(&raw mut POOL) };
    let banks = unsafe { core::slice::from_raw_parts(POOL_BASE.0 as *const u8, POOL_BYTES) };
    let lists = unsafe { core::slice::from_raw_parts(LISTS_BASE.0 as *const u8, LIST_BYTES) };
    pool.reparse(banks, lists).map_err(|_| FallbackReason::ObjectPool)?;
    let regions = unsafe { &mut *(&raw mut REGIONS) };
    *regions = [
        DataRegion { base: DataAddress(RAM_LOW.0), bytes: unsafe { core::slice::from_raw_parts(RAM_LOW.0 as *const u8, RAM_LOW.1) } },
        DataRegion { base: DataAddress(RAM_ALIAS.0), bytes: unsafe { core::slice::from_raw_parts(RAM_ALIAS.0 as *const u8, RAM_ALIAS.1) } },
    ];
    let regions: &'static [DataRegion<'static>; 2] = regions;
    let memory = ResourceMemory::new(regions).map_err(|_| FallbackReason::Scene)?;
    let pool: &'static ObjectPool<'static> = pool;
    let scene = Scene::decode(&memory, pool).map_err(|_| FallbackReason::Scene)?;
    let mut commands = CommandBuffer::new(unsafe { &mut *(&raw mut COMMANDS) });
    let report = scene.lower(&mut commands).map_err(|error| match error {
        SceneError::Capacity => FallbackReason::CommandCapacity,
        _ => FallbackReason::Scene,
    })?;

    apply_latch(state);
    let front = unsafe { read_volatile(FRONT as *const u8) };
    let back = render_next_buffer(front);
    if state.stock_since || state.owned_front.is_some_and(|owned| owned != front) { hand_over(state)?; }
    let publisher = publisher(state, addresses)?;
    let frame = publisher.begin_into(back).map_err(|error| match error {
        PublisherError::Busy => FallbackReason::PublisherBusy,
        _ => FallbackReason::Publisher,
    })?;
    let buffer = ScanoutAddress::new(addresses.0[match back { BufferIndex::First => 0, BufferIndex::Second => 1 }]);
    let reached = report.drawn + report.clipped;
    let floor = f32::from_bits(word(RESULT_FLOOR));
    let render_result = if reached > 0 || floor > 0.0 { 1 } else { 0 };
    Ok(Decided { frame, buffer, commands: commands.draw_list().commands().len(), visible: report.visible as u32, render_result })
}

#[inline(never)]
fn rasterize(state: &mut FrameState, decided: Decided) -> Result<Prepared, FallbackReason> {
    let Decided { frame, buffer, commands, visible, render_result } = decided;
    let publisher = state.publisher.as_mut().ok_or(FallbackReason::Publisher)?;
    let all: &'static [DrawCommand<'static>; COMMAND_CAPACITY] = unsafe { &*(&raw const COMMANDS) };
    let commands = &all[..commands];
    let drawn = publisher.drawing_pixels(frame).map_err(|_| ()).and_then(|pixels| {
        render_osd(commands, pixels, unsafe { &mut *(&raw mut VERTICES) }, unsafe { &mut *(&raw mut STROKES) }).map_err(|_| ())
    });
    if drawn.is_err() || publisher.publish(frame).is_err() || publisher.queued() != Some(frame) {
        let _ = publisher.cancel(frame);
        return Err(if drawn.is_err() { FallbackReason::Raster } else { FallbackReason::Publisher });
    }
    Ok(Prepared { frame, buffer, visible, render_result })
}

fn disjoint(addresses: BufferAddresses) -> bool {
    let [first, second] = addresses.0;
    let length = SurfaceId::Osd.byte_len() as u32;
    first.checked_add(length).is_some() && second.checked_add(length).is_some()
        && (first.wrapping_add(length) <= second || second.wrapping_add(length) <= first)
}

/// Closes the current context after a stock frame step: its buffers were redrawn and republished by stock.
fn hand_over(state: &mut FrameState) -> Result<(), FallbackReason> {
    if let Some(mut old) = state.publisher.take() {
        old.close();
        let context = old.context();
        old.acknowledge_detached(context).map_err(|_| FallbackReason::Publisher)?;
        match old.release() {
            Ok([first, second]) => state.publisher = Some(next_publisher(state, first, second)?),
            Err(old) => { state.publisher = Some(old); return Err(FallbackReason::Publisher); }
        }
    }
    state.stock_since = false;
    Ok(())
}

fn next_publisher(state: &mut FrameState, first: &'static mut [u8], second: &'static mut [u8]) -> Result<Publisher, FallbackReason> {
    let context = ContextId::new(state.next_context).ok_or(FallbackReason::Publisher)?;
    state.next_context = state.next_context.checked_add(1).ok_or(FallbackReason::Publisher)?;
    Publisher::new(context, SurfaceId::Osd, first, second).map_err(|_| FallbackReason::Publisher)
}

fn publisher(state: &mut FrameState, addresses: BufferAddresses) -> Result<&mut Publisher, FallbackReason> {
    if state.publisher.is_none() {
        let length = SurfaceId::Osd.byte_len();
        // The two stage-0 display buffers, verified disjoint; stock VGLIB draws them only on fallback frames, after
        // which `hand_over` closes and detaches the context that held them.
        let first = unsafe { core::slice::from_raw_parts_mut(addresses.0[0] as *mut u8, length) };
        let second = unsafe { core::slice::from_raw_parts_mut(addresses.0[1] as *mut u8, length) };
        let created = next_publisher(state, first, second)?;
        state.publisher = Some(created);
        state.buffers = Some(addresses);
    }
    state.publisher.as_mut().ok_or(FallbackReason::Publisher)
}

/// The stock stage-0 sequence from the lock to the wait (`I4:6d7938-6d7da0`) without surface selection and the
/// OpenVG render, with the scanout bridge queued in place of `0x6d80f0`.
#[inline(never)]
fn commit(state: &mut FrameState, prepared: Prepared) {
    let Prepared { frame, buffer, visible, render_result } = prepared;
    unsafe { rb_render_call_6499d0(OPTION_GROUP, OPTION_INDEX, OPTION_RENDER); }
    set_word(VISITED, visible);
    set_word(RENDER_RESULT, render_result);
    if let Some(publisher) = state.publisher.as_mut() {
        let _ = publisher.submit(frame, |_, _, pixels| {
            unsafe { rb_render_call_6d76ec(GEOMETRY, pixels.as_ptr() as u32, 0, render_result & 0xff); }
            Ok::<(), SubmissionError<core::convert::Infallible>>(())
        });
    }
    let front = advanced_front(unsafe { read_volatile(FRONT as *const u8) });
    unsafe { write_volatile(FRONT as *mut u8, front); }
    state.owned_front = Some(front);
    unsafe { rb_render_call_6499d0(OPTION_GROUP, OPTION_INDEX, word(OPTION_RESTORE)); }
    set_word(CALLBACK_VECTOR, rb_render_frame_scanout_bridge as usize as u32);
    with_latch(|latch| {
        latch.arm(frame, buffer, ScanoutAddress::new(word(OSD_SCANOUT_REGISTER)));
        unsafe { rb_render_call_6cfc68(CALLBACK_VECTOR); }
    });
    let record = unsafe { &mut *(&raw mut rb_render_frame_record) };
    record.owned_frames = record.owned_frames.wrapping_add(1);
    record.last_sequence = frame.sequence();
}

/// Acknowledges the latest latched frame (`scanout_latch`). The publisher ignores a frame of another context and a
/// frame at or below its last acknowledgment.
fn apply_latch(state: &mut FrameState) {
    let Some(frame) = with_latch(ScanoutLatch::take) else { return };
    let Some(publisher) = state.publisher.as_mut() else { return };
    if let Ok(presented) = publisher.acknowledge(frame) {
        let record = unsafe { &mut *(&raw mut rb_render_frame_record) };
        record.acknowledged_sequence = presented.frame.sequence();
    }
}

fn publish_record(state: &FrameState) {
    let record = unsafe { &mut *(&raw mut rb_render_frame_record) };
    match state.publisher.as_ref() {
        None => {
            record.context = 0;
            record.buffer_state = [STATE_FREE; 2];
            record.buffer_sequence = [0; 2];
            record.drain = 0;
        }
        Some(publisher) => {
            record.context = publisher.context().value();
            for (slot, buffer) in publisher.states().into_iter().enumerate() {
                let (tag, frame) = match buffer {
                    BufferState::Free => (STATE_FREE, None),
                    BufferState::Drawing(frame) => (STATE_DRAWING, Some(frame)),
                    BufferState::Queued(frame) => (STATE_QUEUED, Some(frame)),
                    BufferState::Scanning(frame) => (STATE_SCANNING, Some(frame)),
                };
                record.buffer_state[slot] = tag;
                record.buffer_sequence[slot] = frame.map_or(0, FrameId::sequence);
            }
            record.drain = match publisher.drain_state() { DrainState::Drained => 0, DrainState::Pending => 1 };
        }
    }
}
