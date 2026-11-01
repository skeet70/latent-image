#![no_std]
#![feature(alloc_error_handler)]

//! Isolated native-Layout target replay.
//! Calls are serialized on core 0 with interrupt delivery disabled by the host.
//! The host owns pool mapping, disjoint writable output memory and the validated lifetime tape.
//! Free and footprint require the original size/alignment of a live allocation.
//! Talc and BuddyAlloc are conditional comparators, not RTOS-deadline-qualified replacements.

#[cfg(not(all(target_arch = "arm", target_pointer_width = "32")))]
compile_error!("heap replay requires the pinned 32-bit ARM target");

#[cfg(any(
    not(any(feature = "rlsf", feature = "talc", feature = "buddy")),
    all(feature = "rlsf", feature = "talc"),
    all(feature = "rlsf", feature = "buddy"),
    all(feature = "talc", feature = "buddy"),
))]
compile_error!("select exactly one replay backend");

#[cfg(feature = "buddy")]
use buddy_alloc::buddy_alloc::{BuddyAlloc, BuddyAllocParam};
use core::{alloc::{GlobalAlloc, Layout}, mem::size_of, ptr::{self, NonNull}};
#[cfg(feature = "talc")]
use talc::{base::{binning::Binning, Talc}, source::Manual, DefaultBinning};

mod observations;

// Maximum TLSF block is (16 << 17) - 16 bytes on this target.
#[cfg(feature = "rlsf")]
type Tlsf = rlsf::Tlsf<'static, u32, u16, 17, 16>;
#[cfg(feature = "talc")]
type TalcCore = Talc<Manual, DefaultBinning>;
#[cfg(feature = "buddy")]
const LEAF_BYTES: usize = 16;

struct ReplayAllocator;

#[global_allocator]
static GLOBAL_ALLOCATOR: ReplayAllocator = ReplayAllocator;

unsafe impl GlobalAlloc for ReplayAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        heap_replay_alloc(layout.size() as u32, layout.align() as u32) as *mut u8
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        if heap_replay_free(pointer as u32, layout.size() as u32, layout.align() as u32) != Status::Success as u32 {
            heap_replay_panic();
        }
    }
}

#[repr(u32)]
#[derive(Clone, Copy)]
enum Backend {
    #[cfg(feature = "rlsf")]
    Rlsf = 1,
    #[cfg(feature = "talc")]
    Talc = 2,
    #[cfg(feature = "buddy")]
    Buddy = 3,
}

impl TryFrom<u32> for Backend {
    type Error = Status;

    fn try_from(value: u32) -> Result<Self, Status> {
        match value {
            #[cfg(feature = "rlsf")]
            1 => Ok(Self::Rlsf),
            #[cfg(feature = "talc")]
            2 => Ok(Self::Talc),
            #[cfg(feature = "buddy")]
            3 => Ok(Self::Buddy),
            _ => Err(Status::Invalid),
        }
    }
}

#[repr(u32)]
#[derive(Clone, Copy)]
enum Status {
    Success = 0,
    Invalid = 1,
    OutOfMemory = 2,
    UnsupportedMetric = 3,
}

#[repr(C)]
pub struct Stats {
    pub allocated_block_bytes: u32,
    pub free_block_bytes: u32,
    pub largest_free_block_bytes: u32,
    pub external_metadata_bytes: u32,
    pub pool_managed_bytes: u32,
}

#[no_mangle]
pub static mut heap_replay_status: u32 = Status::Success as u32;
static mut STATE: Option<State> = None;

#[derive(Clone, Copy)]
struct Pool {
    base: NonNull<u8>,
    bytes: usize,
}

impl Pool {
    fn parse(base: u32, bytes: u32) -> Result<Self, Status> {
        if base != 0x00a7_1a00 || !matches!(bytes, 1_572_832 | 1_507_296) {
            return Err(Status::Invalid);
        }
        Ok(Self { base: NonNull::new(base as *mut u8).ok_or(Status::Invalid)?, bytes: bytes as usize })
    }

    fn start(self) -> usize {
        self.base.as_ptr() as usize
    }

    fn end(self) -> usize {
        self.start() + self.bytes
    }

    fn contains(self, address: usize, bytes: usize) -> bool {
        address >= self.start() && address.checked_add(bytes).is_some_and(|end| end <= self.end())
    }

    #[cfg(feature = "rlsf")]
    fn slice(self) -> NonNull<[u8]> {
        NonNull::slice_from_raw_parts(self.base, self.bytes)
    }
}

#[derive(Clone, Copy)]
struct NativeLayout(Layout);

impl NativeLayout {
    fn parse(size: u32, align: u32) -> Result<Self, Status> {
        Layout::from_size_align((size as usize).max(1), align as usize)
            .map(Self)
            .map_err(|_| Status::Invalid)
    }

    #[cfg(feature = "buddy")]
    fn buddy_block_bytes(self) -> Result<usize, Status> {
        self.0.size().checked_add(self.0.align().saturating_sub(LEAF_BYTES))
            .and_then(|bytes| bytes.max(LEAF_BYTES).checked_next_power_of_two())
            .ok_or(Status::OutOfMemory)
    }

    #[cfg(feature = "talc")]
    // talc 5.1.1 src/base/chunk.rs:16-34 and src/base/mod.rs:203-231.
    fn talc_block_bytes(self) -> usize {
        (self.0.size() + size_of::<usize>() + talc::base::CHUNK_UNIT - 1)
            & !(talc::base::CHUNK_UNIT - 1)
    }
}

#[cfg(feature = "buddy")]
struct BuddyGeometry {
    managed_base: usize,
    capacity: usize,
    head_base: usize,
    orders: usize,
}

#[cfg(feature = "buddy")]
impl BuddyGeometry {
    fn new(pool: Pool, core: &BuddyAlloc) -> Self {
        // buddy-alloc 0.6.0 buddy_alloc.rs:201-239. Entry is three target pointers.
        let orders = (usize::BITS - 1 - (pool.bytes / LEAF_BYTES).leading_zeros()) as usize + 2;
        let capacity = core.available_bytes();
        // Both supplied endpoints and the post-metadata base are leaf-aligned.
        Self { managed_base: pool.end() - capacity, capacity, head_base: pool.start() + orders * 3 * size_of::<usize>(), orders }
    }

    fn native_pointer(&self, pointer: NonNull<u8>, layout: NativeLayout) -> Result<*mut u8, Status> {
        let block = layout.buddy_block_bytes()?;
        let address = pointer.as_ptr() as usize;
        let offset = address.checked_sub(self.managed_base).ok_or(Status::Invalid)?;
        let native = self.managed_base + (offset & !(block - 1));
        let end = native.checked_add(block).ok_or(Status::Invalid)?;
        if end > self.managed_base + self.capacity
            || address.checked_add(layout.0.size()).is_none_or(|payload_end| payload_end > end)
            || address != align_up(native, layout.0.align())
        {
            return Err(Status::Invalid);
        }
        Ok(native as *mut u8)
    }
}

enum Engine {
    #[cfg(feature = "rlsf")]
    Rlsf { core: Tlsf, admitted: Pool },
    #[cfg(feature = "talc")]
    Talc { core: TalcCore, metadata_bytes: usize },
    #[cfg(feature = "buddy")]
    Buddy { core: BuddyAlloc, geometry: BuddyGeometry },
}

struct State {
    pool: Pool,
    engine: Engine,
}

impl State {
    unsafe fn new(pool: Pool, backend: Backend) -> Result<Self, Status> {
        let engine = match backend {
            #[cfg(feature = "rlsf")]
            Backend::Rlsf => {
                let mut core = Tlsf::new();
                let bytes = core.insert_free_block_ptr(pool.slice()).ok_or(Status::OutOfMemory)?.get();
                Engine::Rlsf { core, admitted: Pool { bytes, ..pool } }
            }
            #[cfg(feature = "talc")]
            Backend::Talc => {
                let mut core = TalcCore::new(Manual);
                core.claim(pool.base.as_ptr(), pool.bytes).ok_or(Status::OutOfMemory)?;
                // talc 5.1.1 src/base/mod.rs:633-653, first-heap bin table and tail tag.
                let metadata_bytes = align_up(pool.start() + DefaultBinning::BIN_COUNT as usize * size_of::<usize>() + size_of::<usize>(), talc::base::CHUNK_UNIT) - pool.start();
                Engine::Talc { core, metadata_bytes }
            }
            #[cfg(feature = "buddy")]
            Backend::Buddy => {
                let core = BuddyAlloc::new(BuddyAllocParam::new(pool.base.as_ptr(), pool.bytes, LEAF_BYTES));
                let geometry = BuddyGeometry::new(pool, &core);
                Engine::Buddy { core, geometry }
            }
        };
        Ok(Self { pool, engine })
    }

    fn validate_request(&self, layout: NativeLayout) -> Result<(), Status> {
        let bytes = match self.engine {
            #[cfg(feature = "buddy")]
            Engine::Buddy { .. } => layout.buddy_block_bytes()?,
            #[cfg(feature = "talc")]
            Engine::Talc { .. } => layout.talc_block_bytes(),
            #[cfg(feature = "rlsf")]
            Engine::Rlsf { .. } => layout.0.size(),
        };
        if bytes > self.pool.bytes { Err(Status::OutOfMemory) } else { Ok(()) }
    }

    fn validate_pointer(&self, pointer: u32, layout: NativeLayout) -> Result<NonNull<u8>, Status> {
        let pointer = NonNull::new(pointer as *mut u8).ok_or(Status::Invalid)?;
        if pointer.as_ptr() as usize & (layout.0.align() - 1) != 0
            || !self.pool.contains(pointer.as_ptr() as usize, layout.0.size())
        {
            return Err(Status::Invalid);
        }
        #[cfg(feature = "buddy")]
        {
            let Engine::Buddy { geometry, .. } = &self.engine;
            geometry.native_pointer(pointer, layout)?;
        }
        Ok(pointer)
    }

    unsafe fn allocate(&mut self, layout: NativeLayout) -> Result<NonNull<u8>, Status> {
        let pointer = match &mut self.engine {
            #[cfg(feature = "rlsf")]
            Engine::Rlsf { core, .. } => core.allocate(layout.0),
            #[cfg(feature = "talc")]
            Engine::Talc { core, .. } => core.try_allocate(layout.0),
            #[cfg(feature = "buddy")]
            Engine::Buddy { core, .. } => {
                let native = core.malloc(layout.buddy_block_bytes()?);
                if native.is_null() { None } else { NonNull::new(align_up(native as usize, layout.0.align()) as *mut u8) }
            }
        };
        pointer.ok_or(Status::OutOfMemory)
    }

    unsafe fn deallocate(&mut self, pointer: NonNull<u8>, layout: NativeLayout) -> Result<(), Status> {
        match &mut self.engine {
            #[cfg(feature = "rlsf")]
            Engine::Rlsf { core, .. } => core.deallocate(pointer, layout.0.align()),
            #[cfg(feature = "talc")]
            Engine::Talc { core, .. } => core.deallocate(pointer.as_ptr(), layout.0),
            #[cfg(feature = "buddy")]
            Engine::Buddy { core, geometry } => core.free(geometry.native_pointer(pointer, layout)?),
        }
        Ok(())
    }
}

#[cfg(any(feature = "talc", feature = "buddy"))]
fn align_up(address: usize, alignment: usize) -> usize {
    (address + alignment - 1) & !(alignment - 1)
}

extern "C" {
    fn heap_replay_stock_lock(context: u32);
    fn heap_replay_stock_unlock(context: u32);
    fn heap_replay_panic() -> !;
}

struct StockLock;

impl StockLock {
    unsafe fn acquire() -> Self {
        heap_replay_stock_lock(0x0089_c978);
        Self
    }
}

impl Drop for StockLock {
    fn drop(&mut self) {
        unsafe { heap_replay_stock_unlock(0x0089_c978) };
    }
}

fn status(status: Status) -> u32 {
    unsafe { ptr::write_volatile(ptr::addr_of_mut!(heap_replay_status), status as u32) };
    status as u32
}

fn finish(result: Result<(), Status>) -> u32 {
    status(match result { Ok(()) => Status::Success, Err(error) => error })
}

// The host clears [__bss_start,__bss_end) before the first call after image load.
#[no_mangle]
pub unsafe extern "C" fn heap_replay_init(pool_base: u32, pool_bytes: u32, backend: u32) -> u32 {
    let result = (|| {
        let backend = Backend::try_from(backend)?;
        let pool = Pool::parse(pool_base, pool_bytes)?;
        if (*ptr::addr_of!(STATE)).is_some() { return Err(Status::Invalid); }
        let _guard = StockLock::acquire();
        *ptr::addr_of_mut!(STATE) = Some(State::new(pool, backend)?);
        Ok(())
    })();
    finish(result)
}

#[no_mangle]
pub unsafe extern "C" fn heap_replay_alloc(size: u32, align: u32) -> u32 {
    let result = (|| {
        let layout = NativeLayout::parse(size, align)?;
        let state = (*ptr::addr_of_mut!(STATE)).as_mut().ok_or(Status::Invalid)?;
        state.validate_request(layout)?;
        let _guard = StockLock::acquire();
        state.allocate(layout)
    })();
    match result {
        Ok(pointer) => { status(Status::Success); pointer.as_ptr() as u32 }
        Err(error) => { status(error); 0 }
    }
}

#[no_mangle]
pub unsafe extern "C" fn heap_replay_free(pointer: u32, size: u32, align: u32) -> u32 {
    let result = (|| {
        let layout = NativeLayout::parse(size, align)?;
        let state = (*ptr::addr_of_mut!(STATE)).as_mut().ok_or(Status::Invalid)?;
        state.validate_request(layout)?;
        if pointer == 0 { return Ok(()); }
        let pointer = state.validate_pointer(pointer, layout)?;
        let _guard = StockLock::acquire();
        state.deallocate(pointer, layout)
    })();
    finish(result)
}

#[no_mangle]
pub unsafe extern "C" fn heap_replay_footprint(pointer: u32, size: u32, align: u32) -> u32 {
    let result = (|| {
        let layout = NativeLayout::parse(size, align)?;
        let state = (*ptr::addr_of!(STATE)).as_ref().ok_or(Status::Invalid)?;
        state.validate_request(layout)?;
        let pointer = state.validate_pointer(pointer, layout)?;
        let _guard = StockLock::acquire();
        state.footprint(pointer, layout)
    })();
    match result {
        Ok(bytes) => { status(Status::Success); bytes as u32 }
        Err(error) => { status(error); 0 }
    }
}

#[no_mangle]
pub unsafe extern "C" fn heap_replay_stats(output: *mut Stats) -> u32 {
    let result = (|| {
        let address = output as usize;
        if address == 0 || address % core::mem::align_of::<Stats>() != 0
            || address.checked_add(size_of::<Stats>()).is_none()
        {
            return Err(Status::Invalid);
        }
        let state = (*ptr::addr_of!(STATE)).as_ref().ok_or(Status::Invalid)?;
        let _guard = StockLock::acquire();
        let stats = state.stats()?;
        output.write(stats);
        Ok(())
    })();
    finish(result)
}

struct PayloadSpan {
    pointer: NonNull<u8>,
    bytes: usize,
}

impl PayloadSpan {
    unsafe fn parse(pointer: u32, bytes: u32) -> Result<Self, Status> {
        let state = (*ptr::addr_of!(STATE)).as_ref().ok_or(Status::Invalid)?;
        let pointer = NonNull::new(pointer as *mut u8).ok_or(Status::Invalid)?;
        let bytes = bytes as usize;
        if !state.pool.contains(pointer.as_ptr() as usize, bytes) {
            return Err(Status::Invalid);
        }
        Ok(Self { pointer, bytes })
    }
}

#[no_mangle]
pub unsafe extern "C" fn heap_replay_zero(pointer: u32, bytes: u32) -> u32 {
    let result = (|| {
        let span = PayloadSpan::parse(pointer, bytes)?;
        ptr::write_bytes(span.pointer.as_ptr(), 0, span.bytes);
        Ok(())
    })();
    finish(result)
}

#[no_mangle]
pub unsafe extern "C" fn heap_replay_copy(destination: u32, source: u32, bytes: u32) -> u32 {
    let result = (|| {
        let destination = PayloadSpan::parse(destination, bytes)?;
        let source = PayloadSpan::parse(source, bytes)?;
        ptr::copy(source.pointer.as_ptr(), destination.pointer.as_ptr(), source.bytes);
        Ok(())
    })();
    finish(result)
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    unsafe { heap_replay_panic() }
}

#[alloc_error_handler]
fn allocation_error(_: Layout) -> ! {
    unsafe { heap_replay_panic() }
}
