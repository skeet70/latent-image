//! Owned stage-0 body of VGLIB's `0x5902` frame step `0x6d7920` (reports/display-render-contracts/vglib-frame.md).
//! `frame.S` reproduces the stock prologue and the lock `0x6d9478` before calling `rb_render_frame_apply`, and resumes
//! stock at `0x6d7930` with that frame and lock when it returns `Fallback`. Every refusal happens before the first
//! stock call after the lock, so a fallback leaves stock state as the stock step finds it at `0x6d7930`. On `Owned`,
//! `frame.S` calls `rb_render_frame_commit`, which submits the frame's OpenVG calls to the retained OpenVG client
//! (`render_commands::vglib_scene::render_stage`, which decodes each object when it visits it and hands objects and
//! groups it does not own to stock's visitor `0x6d7324` and group walk `0x6d909c`); a nonzero commit result is the
//! step's return value, otherwise the `0xfefe` wait and `rb_render_frame_complete` follow.

use crate::render_commands::publisher::{BufferIndex, BufferState, ContextId, DrainState, FrameId, Publisher, PublisherError, SubmissionError};
use crate::render_commands::resources::{DataAddress, DataRegion, ResourceMemory};
use crate::render_commands::scanout_latch::{ScanoutAddress, ScanoutLatch};
use crate::render_commands::stock_objects::VglibObject;
use crate::render_commands::vg_submission::{Matrix, StageDevice, VgCall, VgWord, SURFACE_ATTRIBUTES};
use crate::render_commands::vglib_scene::{render_stage, GroupIndex, RenderOutcome, Scene};
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
/// EGL display and context words `vglib_select_surface` passes (`I4:6d78c8-6d78cc`).
const EGL_DISPLAY: u32 = RENDER_STATE + 0x84;
const EGL_CONTEXT: u32 = RENDER_STATE + 0x88;
/// `0x6d7230` stores `0x887280()` here (`I4:6d7238-6d7244`).
const EGL_ERROR: u32 = RENDER_STATE + 0x50;
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
/// copies the request and rewrites its phase in place, so the request must be writable.
static mut ONE_SHOT: OneShotRequest = OneShotRequest { callback: rb_render_frame_scanout_delivery, retention: 0, order: 2, phase: 0 };
/// Registration's handle output, written under the interrupt lock and never read.
static mut ONE_SHOT_HANDLE: u32 = 0;

/// Guest RAM and its `0x40000000` alias as resource regions; only addresses the scene walk follows are read.
const RAM_LOW: (u32, usize) = (0x0000_1000, 0x0400_0000 - 0x1000);
const RAM_ALIAS: (u32, usize) = (0x4000_0000, 0x0400_0000);

/// Objects of one group list the render copies; a longer list goes to stock's group walk.
const GROUP_SNAPSHOT: usize = 128;
/// Bytes of the frame statics in the bank engine region, excluding code.
const FRAME_STATIC_BYTES: usize = GROUP_SNAPSHOT * core::mem::size_of::<VglibObject>()
    + core::mem::size_of::<FrameState>()
    + core::mem::size_of::<FrameRecord>();
const _: () = assert!(FRAME_STATIC_BYTES <= 0x10000, "frame statics exceed their engine-region budget");
/// The stock visitor; `0x6d724c` passes it to the group walk (`I4:6d72a4-6d72a8`).
const STOCK_VISITOR: u32 = 0x006d_7324;
/// The visitor's output-mode filter word away from frame modes 8 and 10, which draws every visible object
/// (`I4:6d7a90-6d7aa0`, `6d7368-6d7378`).
static VISIT_FILTER: i32 = -1;
/// The frame step's return for a refused surface selection (`I4:6d7ac0-6d7ac8`, `6d7ddc-6d7dfc`).
const SURFACE_REFUSED: i32 = -255;

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
    /// The scene did not decode completely.
    Scene = 7,
    /// The back buffer is not `Free` in the publisher.
    PublisherBusy = 9,
    /// Counted when `vglib_select_surface` fails after the frame was owned; slots 6 and 8 are unused.
    SurfaceRefused = 10,
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
static mut SNAPSHOT: [VglibObject; GROUP_SNAPSHOT] = [VglibObject(DataAddress(0)); GROUP_SNAPSHOT];
static mut REGIONS: [DataRegion<'static>; 2] = [DataRegion { base: DataAddress(0), bytes: &[] }; 2];
/// Accessed only through `with_latch`.
static mut LATCH: ScanoutLatch = ScanoutLatch::new();

unsafe extern "C" {
    fn rb_render_call_6d9494();
    fn rb_render_call_6499d0(group: u32, index: u32, value: u32) -> i32;
    fn rb_render_call_6d76ec(geometry: u32, address: u32, mode: u32, result: u32);
    fn rb_render_frame_advance_front() -> u8;
    fn rb_render_call_6cfc68(vector: u32);
    fn rb_render_call_723ce8(ticks: u32) -> i32;
    fn rb_render_call_6d7324(filter: *const i32, object: u32) -> u32;
    fn rb_render_call_6d909c(sentinel: u32, visitor: u32, filter: *const i32) -> u32;
    fn rb_render_call_88bf94(display: u32, draw: u32, read: u32, context: u32, attributes: *const u32) -> u32;
    fn rb_render_call_88d620(display: u32, surface: u32, object: u32) -> u32;
    fn rb_render_call_887280() -> u32;
    fn rb_render_call_858e60(parameter: u32) -> i32;
    fn rb_render_call_858c04(parameter: u32, value: i32);
    fn rb_render_call_858b78(parameter: u32, value: f32);
    fn rb_render_call_858c90(parameter: u32, count: i32, values: u32);
    fn rb_render_call_858d24(parameter: u32, count: i32, values: *const i32);
    fn rb_render_call_85a1b0(x: i32, y: i32, width: i32, height: i32);
    fn rb_render_call_858748();
    fn rb_render_call_8587c4(matrix: u32);
    fn rb_render_call_8588cc(matrix: *const f32);
    fn rb_render_call_858950(x: f32, y: f32);
    fn rb_render_call_858af4(degrees: f32);
    fn rb_render_call_8589dc(x: f32, y: f32);
    fn rb_render_call_85afbc(paint: u32, modes: u32);
    fn rb_render_call_85ab88(path: u32, modes: u32);
    fn rb_render_call_85b930(image: u32);
    fn rb_render_call_8586d8();
    fn xm1_kernel_critical_enter() -> u32;
    fn xm1_kernel_critical_exit(saved: u32);
    /// Called directly, without a trampoline frame, from the interrupt-context bridge.
    fn rb_render_stock_6c7318(request: *mut u32, handle: *mut u32) -> i32;
    /// The bridge's one-shot (`frame.S`).
    fn rb_render_frame_scanout_delivery();
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

/// A begun frame for `rb_render_frame_commit`.
struct Prepared {
    frame: FrameId,
    back: BufferIndex,
    address: u32,
    buffer: ScanoutAddress,
}

/// The retained OpenVG client and VGLIB state of the stage-0 surface `S(0,b)` and image object `O(0,b)`.
struct StockStage { surface: u32, object: u32 }
impl StageDevice for StockStage {
    fn call(&mut self, call: VgCall) {
        unsafe {
            match call {
                VgCall::SetInteger { parameter, value } => rb_render_call_858c04(parameter.0, value),
                VgCall::SetFloat { parameter, value } => rb_render_call_858b78(parameter.0, value),
                VgCall::SetVector { parameter, count, values } => rb_render_call_858c90(parameter.0, count, values.0),
                VgCall::SetIntegers { parameter, values } => rb_render_call_858d24(parameter.0, 4, values.as_ptr()),
                VgCall::Clear { x, y, width, height } => rb_render_call_85a1b0(x, y, width, height),
                VgCall::LoadIdentity => rb_render_call_858748(),
                VgCall::LoadMatrix(matrix) => rb_render_call_8587c4(matrix.0),
                VgCall::MultiplyMatrix(Matrix::Local(values)) => rb_render_call_8588cc(values.as_ptr()),
                VgCall::MultiplyMatrix(Matrix::Guest(matrix)) => rb_render_call_8588cc(matrix.0 as *const f32),
                VgCall::Translate { x, y } => rb_render_call_858950(x, y),
                VgCall::Rotate(degrees) => rb_render_call_858af4(degrees),
                VgCall::Scale { x, y } => rb_render_call_8589dc(x, y),
                VgCall::SetPaint { paint, modes } => rb_render_call_85afbc(paint, modes),
                VgCall::DrawPath { path, modes } => rb_render_call_85ab88(path, modes),
                VgCall::DrawImage(image) => rb_render_call_85b930(image),
                VgCall::Finish => rb_render_call_8586d8(),
            }
        }
    }
    fn make_current(&mut self) -> bool {
        let attributes = SURFACE_ATTRIBUTES;
        unsafe { rb_render_call_88bf94(word(EGL_DISPLAY), self.surface, self.surface, word(EGL_CONTEXT), attributes.as_ptr()) != 0 }
    }
    fn get_integer(&mut self, parameter: VgWord) -> i32 { unsafe { rb_render_call_858e60(parameter.0) } }
    fn copy_buffers(&mut self) -> bool { unsafe { rb_render_call_88d620(word(EGL_DISPLAY), self.surface, self.object) != 0 } }
    fn record_error(&mut self) { set_word(EGL_ERROR, unsafe { rb_render_call_887280() }); }
    fn set_visits(&mut self, visited: u32, result: u32) {
        set_word(RENDER_RESULT, result);
        set_word(VISITED, visited);
    }
    fn visits(&mut self) -> (u32, u32) { (word(VISITED), word(RENDER_RESULT)) }
    fn yield_task(&mut self) { unsafe { rb_render_call_723ce8(0); } }
    fn stock_visit(&mut self, object: VglibObject) { unsafe { rb_render_call_6d7324(&VISIT_FILTER, object.0.0); } }
    fn stock_walk(&mut self, group: GroupIndex) { unsafe { rb_render_call_6d909c(group.sentinel(), STOCK_VISITOR, &VISIT_FILTER); } }
}

/// Owned work before the first stock call after the lock; `Owned` leaves the frame for `rb_render_frame_commit`.
#[no_mangle]
pub unsafe extern "C" fn rb_render_frame_apply() -> FrameOutcome {
    let state = unsafe { &mut *(&raw mut STATE) };
    match decide(state) {
        Ok(prepared) => {
            state.prepared = Some(prepared);
            FrameOutcome::Owned
        }
        Err(reason) => {
            state.stock_since = true;
            count_fallback(reason);
            publish_record(state);
            FrameOutcome::Fallback
        }
    }
}

fn count_fallback(reason: FallbackReason) {
    let record = unsafe { &mut *(&raw mut rb_render_frame_record) };
    record.fallbacks[reason as usize] = record.fallbacks[reason as usize].wrapping_add(1);
}

/// Called by `frame.S` after `Owned`, from the 48-byte stock frame, so the stock services run above that frame only.
/// Returns 0, or the frame step's return value when it ends before the wait.
#[no_mangle]
pub unsafe extern "C" fn rb_render_frame_commit() -> i32 {
    let state = unsafe { &mut *(&raw mut STATE) };
    match state.prepared.take() { Some(prepared) => commit(state, prepared), None => 0 }
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
    let registered = unsafe { rb_render_stock_6c7318(&raw mut ONE_SHOT as *mut u32, &raw mut ONE_SHOT_HANDLE) } >= 0;
    unsafe { (*(&raw mut LATCH)).commit_pass(register, registered); }
    unsafe { xm1_kernel_critical_exit(token); }
}

/// The latch update of `rb_render_frame_scanout_delivery`.
#[no_mangle]
pub unsafe extern "C" fn rb_render_frame_scanout_delivered() {
    let token = unsafe { xm1_kernel_critical_enter() };
    unsafe { (*(&raw mut LATCH)).delivered(); }
    unsafe { xm1_kernel_critical_exit(token); }
}

#[inline(never)]
fn decide(state: &mut FrameState) -> Result<Prepared, FallbackReason> {
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

    let memory = guest_memory().ok_or(FallbackReason::Scene)?;
    Scene::decode(&memory).and_then(|scene| scene.check()).map_err(|_| FallbackReason::Scene)?;

    apply_latch(state);
    let front = unsafe { read_volatile(FRONT as *const u8) };
    let back = render_next_buffer(front);
    if state.stock_since || state.owned_front.is_some_and(|owned| owned != front) { hand_over(state)?; }
    let publisher = publisher(state, addresses)?;
    let frame = publisher.begin_into(back).map_err(|error| match error {
        PublisherError::Busy => FallbackReason::PublisherBusy,
        _ => FallbackReason::Publisher,
    })?;
    let address = addresses.0[slot(back)];
    Ok(Prepared { frame, back, address, buffer: ScanoutAddress::new(address) })
}

fn slot(index: BufferIndex) -> usize { match index { BufferIndex::First => 0, BufferIndex::Second => 1 } }

/// Guest RAM and its alias, from the static regions.
fn guest_memory() -> Option<ResourceMemory<'static>> {
    let regions = unsafe { &mut *(&raw mut REGIONS) };
    *regions = [
        DataRegion { base: DataAddress(RAM_LOW.0), bytes: unsafe { core::slice::from_raw_parts(RAM_LOW.0 as *const u8, RAM_LOW.1) } },
        DataRegion { base: DataAddress(RAM_ALIAS.0), bytes: unsafe { core::slice::from_raw_parts(RAM_ALIAS.0 as *const u8, RAM_ALIAS.1) } },
    ];
    let regions: &'static [DataRegion<'static>; 2] = regions;
    ResourceMemory::new(regions).ok()
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

/// The stock stage-0 sequence from the lock to the wait (`I4:6d7938-6d7da0`): option, surface selection and render,
/// hand-off as the publisher's submit transport, front advance, option restore, and the scanout bridge queued in place
/// of `0x6d80f0`.
#[inline(never)]
fn commit(state: &mut FrameState, prepared: Prepared) -> i32 {
    let Prepared { frame, back, address, buffer } = prepared;
    unsafe { rb_render_call_6499d0(OPTION_GROUP, OPTION_INDEX, OPTION_RENDER); }
    let index = 4 * slot(back) as u32;
    let mut stage = StockStage { surface: word(SURFACES + index), object: word(IMAGES + index) };
    let snapshot: &'static mut [VglibObject; GROUP_SNAPSHOT] = unsafe { &mut *(&raw mut SNAPSHOT) };
    let floor = f32::from_bits(word(RESULT_FLOOR));
    let outcome = guest_memory().map(|memory| {
        render_stage(i32::from(half(GEOMETRY)), i32::from(half(GEOMETRY + 2)), floor, &memory, snapshot, &mut stage)
    });
    let render_result = match outcome {
        Some(RenderOutcome::Rendered(result)) => result,
        _ => {
            if let Some(publisher) = state.publisher.as_mut() { let _ = publisher.cancel(frame); }
            count_fallback(FallbackReason::SurfaceRefused);
            publish_record(state);
            return SURFACE_REFUSED;
        }
    };
    let hand_off = |address: u32| unsafe { rb_render_call_6d76ec(GEOMETRY, address, 0, render_result & 0xff); };
    let published = state.publisher.as_mut().and_then(|publisher| if publisher.publish(frame).is_ok() { Some(publisher) } else { None });
    match published {
        Some(publisher) => {
            let _ = publisher.submit(frame, |_, _, pixels| {
                hand_off(pixels.as_ptr() as u32);
                Ok::<(), SubmissionError<core::convert::Infallible>>(())
            });
        }
        None => hand_off(address),
    }
    let front = unsafe { rb_render_frame_advance_front() };
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
    0
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
