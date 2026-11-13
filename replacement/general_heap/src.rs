#![cfg_attr(not(feature = "host"), no_std)]
#![feature(allocator_api)]
#![cfg_attr(not(feature = "host"), feature(alloc_error_handler))]

use core::{alloc::{AllocError, Allocator, GlobalAlloc, Layout}, ptr::{self, NonNull}};

mod backend;
pub use backend::{Heap, HeapError, MAX_STOCK_BYTES};
use backend::{checked_layout, StockRequest};

#[cfg(all(target_arch = "arm", target_pointer_width = "32", not(feature = "host")))]
pub use target::allocator;

#[cfg(test)]
mod tests;

/// # Safety
/// `with_heap` must hold exclusive ownership of the initialized heap throughout the callback, releasing it on every normal return.
/// `capacity` must not exceed the arena's capacity and must be readable before initialization or locking.
pub unsafe trait HeapAccess {
    fn capacity(&self) -> usize;
    fn with_heap<T>(&self, operation: impl FnOnce(&mut Heap) -> T) -> T;
    fn set_errno(&self, errno: u32);
}

pub struct StockBridge<A> {
    access: A,
}

impl<A: HeapAccess> StockBridge<A> {
    pub const fn new(access: A) -> Self {
        Self { access }
    }

    fn finish(&self, result: Result<NonNull<u8>, HeapError>) -> Result<Option<NonNull<u8>>, HeapError> {
        match result {
            Ok(payload) => Ok(Some(payload)),
            Err(HeapError::OutOfMemory) => {
                self.access.set_errno(12);
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    pub fn malloc(&self, bytes: u32) -> Result<Option<NonNull<u8>>, HeapError> {
        let request = match StockRequest::parse(bytes, self.access.capacity()) {
            Ok(request) => request,
            Err(error) => return self.finish(Err(error)),
        };
        self.finish(self.access.with_heap(|heap| unsafe { heap.allocate_stock(request) }))
    }

    pub fn calloc(&self, count: u32, bytes: u32) -> Result<Option<NonNull<u8>>, HeapError> {
        let Some(product) = count.checked_mul(bytes) else {
            return self.finish(Err(HeapError::OutOfMemory));
        };
        let request = match StockRequest::parse(product, self.access.capacity()) {
            Ok(request) => request,
            Err(error) => return self.finish(Err(error)),
        };
        self.finish(self.access.with_heap(|heap| unsafe { heap.calloc_stock(request) }))
    }

    /// # Safety
    /// `old`, when present, must point to readable arena memory.
    pub unsafe fn realloc(&self, old: Option<NonNull<u8>>, bytes: u32) -> Result<Option<NonNull<u8>>, HeapError> {
        let request = match StockRequest::parse(bytes, self.access.capacity()) {
            Ok(request) => request,
            Err(error) => return self.finish(Err(error)),
        };
        self.finish(self.access.with_heap(|heap| heap.resize_stock(old, request)))
    }

    /// # Safety
    /// `payload`, when present, must point to readable arena memory.
    pub unsafe fn free(&self, payload: Option<NonNull<u8>>) -> Result<(), HeapError> {
        if payload.is_none() {
            return Ok(());
        }
        self.access.with_heap(|heap| heap.free(payload))
    }

    pub fn trim(&self, _pad: u32) -> u32 {
        self.access.with_heap(|_| 0)
    }
}

pub struct HeapAllocator<A> {
    access: A,
}

impl<A: HeapAccess> HeapAllocator<A> {
    pub const fn new(access: A) -> Self {
        Self { access }
    }

    fn allocate_nonzero(&self, layout: Layout) -> Result<NonNull<u8>, AllocError> {
        checked_layout(layout, self.access.capacity()).map_err(|_| AllocError)?;
        self.access.with_heap(|heap| unsafe { heap.allocate_native(layout) }).map_err(|_| AllocError)
    }

    unsafe fn resize(&self, payload: NonNull<u8>, old: Layout, new: Layout, zeroed: bool) -> Result<NonNull<[u8]>, AllocError> {
        if old.size() == 0 {
            return if zeroed { Allocator::allocate_zeroed(self, new) } else { Allocator::allocate(self, new) };
        }
        if new.size() == 0 {
            Allocator::deallocate(self, payload, old);
            return Ok(empty_allocation(new));
        }
        checked_layout(new, self.access.capacity()).map_err(|_| AllocError)?;
        let resized = self.access.with_heap(|heap| heap.resize_native(payload, old, new))
            .map_err(|_| AllocError)?;
        if zeroed && new.size() > old.size() {
            ptr::write_bytes(resized.as_ptr().add(old.size()), 0, new.size() - old.size());
        }
        Ok(NonNull::slice_from_raw_parts(resized, new.size()))
    }
}

unsafe impl<A: HeapAccess> GlobalAlloc for HeapAllocator<A> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.allocate_nonzero(layout).map_or(ptr::null_mut(), NonNull::as_ptr)
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let payload = GlobalAlloc::alloc(self, layout);
        if !payload.is_null() {
            ptr::write_bytes(payload, 0, layout.size());
        }
        payload
    }

    unsafe fn dealloc(&self, payload: *mut u8, layout: Layout) {
        let Some(payload) = NonNull::new(payload) else { refuse(); };
        if self.access.with_heap(|heap| heap.deallocate_native(payload, layout)).is_err() {
            refuse();
        }
    }

    unsafe fn realloc(&self, payload: *mut u8, old: Layout, bytes: usize) -> *mut u8 {
        let Ok(new) = Layout::from_size_align(bytes, old.align()) else { return ptr::null_mut(); };
        if checked_layout(new, self.access.capacity()).is_err() {
            return ptr::null_mut();
        }
        let Some(payload) = NonNull::new(payload) else { return ptr::null_mut(); };
        self.access.with_heap(|heap| heap.resize_native(payload, old, new))
            .map_or(ptr::null_mut(), NonNull::as_ptr)
    }
}

unsafe impl<A: HeapAccess> Allocator for HeapAllocator<A> {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        if layout.size() == 0 {
            return Ok(empty_allocation(layout));
        }
        self.allocate_nonzero(layout).map(|payload| NonNull::slice_from_raw_parts(payload, layout.size()))
    }

    fn allocate_zeroed(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        let allocation = Allocator::allocate(self, layout)?;
        unsafe { ptr::write_bytes(allocation.as_ptr() as *mut u8, 0, layout.size()); }
        Ok(allocation)
    }

    unsafe fn deallocate(&self, payload: NonNull<u8>, layout: Layout) {
        if layout.size() != 0 && self.access.with_heap(|heap| heap.deallocate_native(payload, layout)).is_err() {
            refuse();
        }
    }

    unsafe fn grow(&self, payload: NonNull<u8>, old: Layout, new: Layout) -> Result<NonNull<[u8]>, AllocError> {
        self.resize(payload, old, new, false)
    }

    unsafe fn grow_zeroed(&self, payload: NonNull<u8>, old: Layout, new: Layout) -> Result<NonNull<[u8]>, AllocError> {
        self.resize(payload, old, new, true)
    }

    unsafe fn shrink(&self, payload: NonNull<u8>, old: Layout, new: Layout) -> Result<NonNull<[u8]>, AllocError> {
        self.resize(payload, old, new, false)
    }
}

fn empty_allocation(layout: Layout) -> NonNull<[u8]> {
    let dangling = unsafe { NonNull::new_unchecked(layout.align() as *mut u8) };
    NonNull::slice_from_raw_parts(dangling, 0)
}

fn refuse() -> ! {
    #[cfg(all(target_arch = "arm", not(feature = "host")))]
    unsafe { target::xm1_heap_panic() }
    #[cfg(any(not(target_arch = "arm"), feature = "host"))]
    panic!("invalid general heap allocation")
}

#[cfg(all(target_arch = "arm", target_pointer_width = "32", not(feature = "host")))]
mod target {
    use super::*;
    use core::mem::{align_of, size_of};

    const ARENA_BASE: usize = 0x00a7_1a00;
    #[cfg(all(feature = "recipe-arena", not(feature = "fixed-arena")))]
    const ARENA_END: usize = 0x00be_1a00;
    #[cfg(not(any(feature = "recipe-arena", feature = "fixed-arena")))]
    const ARENA_END: usize = 0x00bf_1a00;
    #[cfg(feature = "fixed-arena")]
    const ARENA_END: usize = include!(env!("XM1_FIXED_HEAP_END_PATH"));
    const GUARD_BYTES: usize = 32;
    const ARENA_BYTES: usize = ARENA_END - ARENA_BASE - GUARD_BYTES;
    const STATE_BYTES: usize = size_of::<Heap>();
    const POOL_START: usize = (ARENA_BASE + STATE_BYTES + rlsf::GRANULARITY - 1)
        & !(rlsf::GRANULARITY - 1);
    const INITIALIZED: u32 = 0x4845_4150;

    #[used]
    #[no_mangle]
    #[link_section = ".heap.metadata"]
    pub static XM1_HEAP_METADATA: [u32; 14] = [
        0x4845_4150, 1, ARENA_BASE as u32, ARENA_END as u32,
        STATE_BYTES as u32, POOL_START as u32, GUARD_BYTES as u32,
        backend::FIRST_LEVELS as u32, backend::SECOND_LEVELS as u32,
        rlsf::GRANULARITY as u32, backend::PREFIX_BYTES as u32,
        backend::STOCK_ALIGNMENT as u32, backend::FirstLevelBitmap::BITS,
        backend::SecondLevelBitmap::BITS,
    ];

    extern "C" {
        fn xm1_heap_lock(context: u32);
        fn xm1_heap_unlock(context: u32);
        pub(super) fn xm1_heap_panic() -> !;
        static mut __heap_sentinel: u32;
    }

    pub struct StockContext(u32);

    struct HeldLock(u32);

    impl Drop for HeldLock {
        fn drop(&mut self) {
            unsafe { xm1_heap_unlock(self.0); }
        }
    }

    #[cold]
    #[inline(never)]
    unsafe fn initialize(state: *mut Heap) {
        if Heap::initialize(state, NonNull::new_unchecked(ARENA_BASE as *mut u8), ARENA_BYTES).is_err() {
            refuse();
        }
    }

    #[inline(never)]
    unsafe fn acquire(context: u32) -> *mut Heap {
        xm1_heap_lock(context);
        let sentinel = ptr::addr_of_mut!(__heap_sentinel);
        let state = ARENA_BASE as *mut Heap;
        match ptr::read_volatile(sentinel) {
            0 => {
                initialize(state);
                ptr::write_volatile(sentinel, INITIALIZED);
            }
            INITIALIZED => {}
            _ => refuse(),
        }
        state
    }

    unsafe impl HeapAccess for StockContext {
        fn capacity(&self) -> usize {
            ARENA_END - GUARD_BYTES - POOL_START
        }

        fn with_heap<T>(&self, operation: impl FnOnce(&mut Heap) -> T) -> T {
            unsafe {
                let state = acquire(self.0);
                let _held = HeldLock(self.0);
                operation(&mut *state)
            }
        }

        fn set_errno(&self, errno: u32) {
            unsafe { ptr::write_volatile(self.0 as *mut u32, errno); }
        }
    }

    const _: () = assert!(ARENA_BASE % align_of::<Heap>() == 0);

    #[global_allocator]
    static GLOBAL_ALLOCATOR: HeapAllocator<StockContext> = HeapAllocator::new(StockContext(0));

    pub fn allocator() -> &'static (impl Allocator + GlobalAlloc + Sync) {
        &GLOBAL_ALLOCATOR
    }

    fn result_pointer(result: Result<Option<NonNull<u8>>, HeapError>) -> u32 {
        match result {
            Ok(Some(payload)) => payload.as_ptr() as u32,
            Ok(None) => 0,
            Err(_) => refuse(),
        }
    }

    #[no_mangle]
    #[link_section = ".heap.malloc"]
    pub extern "C" fn xm1_heap_malloc_r(context: u32, bytes: u32) -> u32 {
        result_pointer(StockBridge::new(StockContext(context)).malloc(bytes))
    }

    #[no_mangle]
    #[link_section = ".heap.free"]
    pub unsafe extern "C" fn xm1_heap_free_r(context: u32, payload: u32) {
        if StockBridge::new(StockContext(context)).free(NonNull::new(payload as *mut u8)).is_err() {
            refuse();
        }
    }

    #[no_mangle]
    #[link_section = ".heap.realloc"]
    pub unsafe extern "C" fn xm1_heap_realloc_r(context: u32, old: u32, bytes: u32) -> u32 {
        result_pointer(StockBridge::new(StockContext(context)).realloc(NonNull::new(old as *mut u8), bytes))
    }

    #[no_mangle]
    #[link_section = ".heap.calloc"]
    pub extern "C" fn xm1_heap_calloc_r(context: u32, count: u32, bytes: u32) -> u32 {
        result_pointer(StockBridge::new(StockContext(context)).calloc(count, bytes))
    }

    #[no_mangle]
    #[link_section = ".heap.trim"]
    pub extern "C" fn xm1_heap_trim_r(context: u32, pad: u32) -> u32 {
        StockBridge::new(StockContext(context)).trim(pad)
    }

    #[panic_handler]
    fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
        unsafe { xm1_heap_panic() }
    }

    #[alloc_error_handler]
    fn allocation_error(_: Layout) -> ! {
        unsafe { xm1_heap_panic() }
    }
}
