use crate::{Heap, HeapAccess, HeapAllocator, HeapError, StockBridge, MAX_STOCK_BYTES};
use core::{alloc::{Allocator, GlobalAlloc, Layout}, ptr::{self, NonNull}};
use std::{alloc::{alloc_zeroed, dealloc}, cell::{Cell, RefCell}, vec, vec::Vec};

struct Arena {
    base: NonNull<u8>,
    layout: Layout,
}

impl Arena {
    fn new(bytes: usize) -> Self {
        let layout = Layout::from_size_align(bytes, 64).unwrap();
        let base = NonNull::new(unsafe { alloc_zeroed(layout) }).unwrap();
        Self { base, layout }
    }
}

impl Drop for Arena {
    fn drop(&mut self) {
        unsafe { dealloc(self.base.as_ptr(), self.layout); }
    }
}

struct Host {
    heap: RefCell<Option<Heap>>,
    arena: Arena,
    errno: Cell<u32>,
    depth: Cell<usize>,
    acquisitions: Cell<usize>,
    releases: Cell<usize>,
    initializations: Cell<usize>,
}

impl Host {
    fn new(bytes: usize) -> Self {
        Self {
            heap: RefCell::new(None), arena: Arena::new(bytes), errno: Cell::new(37),
            depth: Cell::new(0), acquisitions: Cell::new(0), releases: Cell::new(0),
            initializations: Cell::new(0),
        }
    }

    fn balanced(&self) {
        assert_eq!(self.depth.get(), 0);
        assert_eq!(self.acquisitions.get(), self.releases.get());
    }
}

struct Held<'a>(&'a Host);

impl Drop for Held<'_> {
    fn drop(&mut self) {
        self.0.depth.set(self.0.depth.get() - 1);
        self.0.releases.set(self.0.releases.get() + 1);
    }
}

unsafe impl HeapAccess for &Host {
    fn capacity(&self) -> usize {
        self.arena.layout.size() - core::mem::size_of::<Heap>() - rlsf::GRANULARITY
    }

    fn with_heap<T>(&self, operation: impl FnOnce(&mut Heap) -> T) -> T {
        self.acquisitions.set(self.acquisitions.get() + 1);
        self.depth.set(self.depth.get() + 1);
        let _held = Held(self);
        let mut slot = self.heap.borrow_mut();
        let heap = slot.get_or_insert_with(|| {
            self.initializations.set(self.initializations.get() + 1);
            unsafe { Heap::new(self.arena.base, self.arena.layout.size()).unwrap() }
        });
        operation(heap)
    }

    fn set_errno(&self, errno: u32) {
        self.errno.set(errno);
    }
}

unsafe fn bytes_at(payload: NonNull<u8>, bytes: usize) -> Vec<u8> {
    core::slice::from_raw_parts(payload.as_ptr(), bytes).to_vec()
}

unsafe fn fill(payload: NonNull<u8>, bytes: &[u8]) {
    ptr::copy_nonoverlapping(bytes.as_ptr(), payload.as_ptr(), bytes.len());
}

#[test]
fn stock_boundaries_have_compatible_usable_tags_and_independent_lifetimes() {
    let host = Host::new(128 * 1024);
    let bridge = StockBridge::new(&host);
    for bytes in (0..160).chain([255, 256, 257, 4095, 4096, 4097]) {
        let payload = bridge.malloc(bytes).unwrap().unwrap();
        assert_eq!(payload.as_ptr() as usize % 8, 0);
        let tag = unsafe { ptr::read(payload.as_ptr().sub(4).cast::<u32>()) };
        assert_eq!(tag & 7, 1);
        let usable = ((tag & !3) - 4) as usize;
        assert!(usable >= (bytes as usize).max(1));
        assert!(payload.as_ptr() as usize + usable <= host.arena.base.as_ptr() as usize + host.arena.layout.size());
        unsafe { ptr::write_bytes(payload.as_ptr(), 0x5c, usable); }
        let neighbor = bridge.calloc(17, 3).unwrap().unwrap();
        assert_ne!(payload, neighbor);
        assert_eq!(unsafe { bytes_at(payload, usable) }, vec![0x5c; usable]);
        let neighbor_usable = unsafe { stock_usable(neighbor) };
        assert_eq!(unsafe { bytes_at(neighbor, neighbor_usable) }, vec![0; neighbor_usable]);
        unsafe {
            bridge.free(Some(neighbor)).unwrap();
            bridge.free(Some(payload)).unwrap();
        }
        let zeroed = bridge.calloc(bytes, 1).unwrap().unwrap();
        let zeroed_usable = unsafe { stock_usable(zeroed) };
        assert_eq!(unsafe { bytes_at(zeroed, zeroed_usable) }, vec![0; zeroed_usable]);
        unsafe { bridge.free(Some(zeroed)).unwrap(); }
        host.balanced();
    }
    assert_eq!(host.initializations.get(), 1);
    assert_eq!(host.errno.get(), 37);
}

unsafe fn stock_usable(payload: NonNull<u8>) -> usize {
    let tag = ptr::read(payload.as_ptr().sub(4).cast::<u32>());
    ((tag & !3) - 4) as usize
}

#[test]
fn rejected_sizes_and_products_do_not_lock_or_initialize() {
    let host = Host::new(32 * 1024);
    let bridge = StockBridge::new(&host);
    for bytes in [MAX_STOCK_BYTES, MAX_STOCK_BYTES + 1, u32::MAX, 32 * 1024] {
        assert!(bridge.malloc(bytes).unwrap().is_none());
        assert!(unsafe { bridge.realloc(None, bytes) }.unwrap().is_none());
    }
    for (count, bytes) in [(u32::MAX, 2), (65536, 65536), (0x8000_0000, 2)] {
        assert!(bridge.calloc(count, bytes).unwrap().is_none());
    }
    unsafe { bridge.free(None).unwrap(); }
    assert_eq!(host.errno.get(), 12);
    assert_eq!(host.acquisitions.get(), 0);
    assert_eq!(host.initializations.get(), 0);
    assert_eq!(bridge.trim(0), 0);
    assert_eq!(bridge.trim(u32::MAX), 0);
    assert_eq!(host.initializations.get(), 1);
    host.balanced();
}

#[test]
fn zero_products_and_zero_resizes_keep_live_nonnull_allocations() {
    let host = Host::new(32 * 1024);
    let bridge = StockBridge::new(&host);
    let first = bridge.calloc(0, u32::MAX).unwrap().unwrap();
    let second = bridge.calloc(u32::MAX, 0).unwrap().unwrap();
    assert_ne!(first, second);
    for payload in [first, second] {
        let usable = unsafe { stock_usable(payload) };
        assert_eq!(unsafe { bytes_at(payload, usable) }, vec![0; usable]);
    }
    let first = unsafe { bridge.realloc(Some(first), 0) }.unwrap().unwrap();
    assert_ne!(first, second);
    unsafe {
        bridge.free(Some(first)).unwrap();
        bridge.free(Some(second)).unwrap();
    }
    host.balanced();
}

#[test]
fn exhaustion_preserves_realloc_bytes_and_freed_space_coalesces_for_reuse() {
    let host = Host::new(64 * 1024);
    let bridge = StockBridge::new(&host);
    let mut allocations = Vec::new();
    while let Some(payload) = bridge.malloc(512).unwrap() {
        let pattern = vec![allocations.len() as u8; 512];
        unsafe { fill(payload, &pattern); }
        allocations.push((payload, pattern));
    }
    assert!(allocations.len() > 16);
    let (first, pattern) = &allocations[0];
    assert!(unsafe { bridge.realloc(Some(*first), 8192) }.unwrap().is_none());
    assert_eq!(unsafe { bytes_at(*first, pattern.len()) }, *pattern);
    for parity in [0, 1] {
        for (index, (payload, pattern)) in allocations.iter().enumerate() {
            if index % 2 == parity {
                assert_eq!(unsafe { bytes_at(*payload, pattern.len()) }, *pattern);
                unsafe { bridge.free(Some(*payload)).unwrap(); }
            }
        }
    }
    for _ in 0..4 {
        let payload = bridge.malloc(40 * 1024).unwrap().unwrap();
        unsafe { bridge.free(Some(payload)).unwrap(); }
    }
    host.balanced();
}

#[test]
fn every_corrupt_prefix_field_refuses_free_and_resize_without_losing_payload() {
    let host = Host::new(32 * 1024);
    let bridge = StockBridge::new(&host);
    let payload = bridge.malloc(64).unwrap().unwrap();
    let contents = vec![0xa7; 64];
    unsafe { fill(payload, &contents); }
    let mut saved = [0u8; 16];
    unsafe { ptr::copy_nonoverlapping(payload.as_ptr().sub(16), saved.as_mut_ptr(), saved.len()); }
    for offset in 0..16 {
        unsafe {
            let byte = payload.as_ptr().sub(16).add(offset);
            ptr::write(byte, ptr::read(byte) ^ 0x80);
            assert_eq!(bridge.free(Some(payload)), Err(HeapError::CorruptHeader));
            assert_eq!(bridge.realloc(Some(payload), 128), Err(HeapError::CorruptHeader));
            assert_eq!(bytes_at(payload, 64), contents);
            ptr::copy_nonoverlapping(saved.as_ptr(), payload.as_ptr().sub(16), saved.len());
        }
        host.balanced();
    }
    let other = bridge.malloc(64).unwrap().unwrap();
    let mut other_saved = [0u8; 16];
    unsafe {
        ptr::copy_nonoverlapping(other.as_ptr().sub(16), other_saved.as_mut_ptr(), 16);
        ptr::copy_nonoverlapping(saved.as_ptr(), other.as_ptr().sub(16), 16);
        assert_eq!(bridge.free(Some(other)), Err(HeapError::CorruptHeader));
        ptr::copy_nonoverlapping(other_saved.as_ptr(), other.as_ptr().sub(16), 16);
        bridge.free(Some(other)).unwrap();
        bridge.free(Some(payload)).unwrap();
    }
    host.balanced();
}

#[test]
fn native_and_stock_payloads_keep_independent_lifetimes_in_one_arena() {
    let host = Host::new(64 * 1024);
    let bridge = StockBridge::new(&host);
    let allocator = HeapAllocator::new(&host);
    let stock = bridge.malloc(64).unwrap().unwrap();
    let layout = Layout::from_size_align(64, 256).unwrap();
    let native = Allocator::allocate(&allocator, layout).unwrap().cast::<u8>();
    unsafe {
        fill(stock, &[0x35; 64]);
        fill(native, &[0x91; 64]);
    }
    assert_eq!(unsafe { bytes_at(stock, 64) }, vec![0x35; 64]);
    assert_eq!(unsafe { bytes_at(native, 64) }, vec![0x91; 64]);
    let stock = unsafe { bridge.realloc(Some(stock), 2048) }.unwrap().unwrap();
    assert_eq!(unsafe { bytes_at(stock, 64) }, vec![0x35; 64]);
    assert_eq!(unsafe { bytes_at(native, 64) }, vec![0x91; 64]);
    unsafe {
        Allocator::deallocate(&allocator, native, layout);
        bridge.free(Some(stock)).unwrap();
    }
    host.balanced();
}

#[test]
fn allocator_traits_preserve_layout_alignment_zeroing_and_failure_ownership() {
    let host = Host::new(256 * 1024);
    let allocator = HeapAllocator::new(&host);
    for alignment in [1, 8, 16, 32, 64, 256, 4096, 16384] {
        let zero = Layout::from_size_align(0, alignment).unwrap();
        let empty = Allocator::allocate(&allocator, zero).unwrap();
        assert_eq!(empty.len(), 0);
        assert_eq!(empty.as_ptr() as *mut u8 as usize % alignment, 0);
        unsafe { Allocator::deallocate(&allocator, empty.cast(), zero); }
        let old = Layout::from_size_align(113, alignment).unwrap();
        let payload = Allocator::allocate_zeroed(&allocator, old).unwrap().cast::<u8>();
        assert_eq!(payload.as_ptr() as usize % alignment, 0);
        assert_eq!(unsafe { bytes_at(payload, 113) }, vec![0; 113]);
        unsafe { fill(payload, &[0x3a; 113]); }
        let impossible = Layout::from_size_align(1024 * 1024, alignment).unwrap();
        assert!(unsafe { Allocator::grow(&allocator, payload, old, impossible) }.is_err());
        assert_eq!(unsafe { bytes_at(payload, 113) }, vec![0x3a; 113]);
        let grown = Layout::from_size_align(701, alignment * 2).unwrap();
        let payload = unsafe { Allocator::grow_zeroed(&allocator, payload, old, grown) }.unwrap().cast::<u8>();
        assert_eq!(payload.as_ptr() as usize % (alignment * 2), 0);
        assert_eq!(unsafe { bytes_at(payload, 113) }, vec![0x3a; 113]);
        assert_eq!(unsafe { bytes_at(NonNull::new_unchecked(payload.as_ptr().add(113)), 588) }, vec![0; 588]);
        let shrunk = Layout::from_size_align(17, alignment).unwrap();
        let payload = unsafe { Allocator::shrink(&allocator, payload, grown, shrunk) }.unwrap().cast::<u8>();
        assert_eq!(unsafe { bytes_at(payload, 17) }, vec![0x3a; 17]);
        let empty = unsafe { Allocator::shrink(&allocator, payload, shrunk, zero) }.unwrap();
        let payload = unsafe { Allocator::grow_zeroed(&allocator, empty.cast(), zero, old) }.unwrap().cast::<u8>();
        assert_eq!(unsafe { bytes_at(payload, 113) }, vec![0; 113]);
        unsafe { Allocator::deallocate(&allocator, payload, old); }
        let global = unsafe { GlobalAlloc::alloc_zeroed(&allocator, old) };
        assert!(!global.is_null());
        assert_eq!(unsafe { bytes_at(NonNull::new(global).unwrap(), 113) }, vec![0; 113]);
        unsafe { ptr::write_bytes(global, 0x46, 113); }
        assert!(unsafe { GlobalAlloc::realloc(&allocator, global, old, usize::MAX) }.is_null());
        assert_eq!(unsafe { bytes_at(NonNull::new(global).unwrap(), 113) }, vec![0x46; 113]);
        let global = unsafe { GlobalAlloc::realloc(&allocator, global, old, 997) };
        assert!(!global.is_null());
        assert_eq!(unsafe { bytes_at(NonNull::new(global).unwrap(), 113) }, vec![0x46; 113]);
        unsafe { GlobalAlloc::dealloc(&allocator, global, Layout::from_size_align(997, alignment).unwrap()); }
        host.balanced();
    }
}

struct Generator(u64);

impl Generator {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 as u32
    }
}

struct Lifetime {
    payload: NonNull<u8>,
    expected: Vec<u8>,
}

fn check_lifetimes(live: &[Lifetime]) {
    for (index, allocation) in live.iter().enumerate() {
        assert_eq!(unsafe { bytes_at(allocation.payload, allocation.expected.len()) }, allocation.expected);
        let start = allocation.payload.as_ptr() as usize;
        let end = start + allocation.expected.len().max(1);
        for other in &live[index + 1..] {
            let other_start = other.payload.as_ptr() as usize;
            let other_end = other_start + other.expected.len().max(1);
            assert!(end <= other_start || other_end <= start);
        }
    }
}

#[test]
fn native_admission_rejects_before_lock_and_exhaustion_is_reusable() {
    let host = Host::new(64 * 1024);
    let allocator = HeapAllocator::new(&host);
    let impossible = Layout::from_size_align(1, 1024 * 1024).unwrap();
    assert!(Allocator::allocate(&allocator, impossible).is_err());
    let empty = Layout::from_size_align(0, 1024 * 1024).unwrap();
    let payload = Allocator::allocate(&allocator, empty).unwrap();
    unsafe { Allocator::deallocate(&allocator, payload.cast(), empty); }
    assert_eq!(host.initializations.get(), 0);
    assert_eq!(host.acquisitions.get(), 0);
    let small = Layout::from_size_align(73, 64).unwrap();
    let mut live = Vec::new();
    while let Ok(payload) = Allocator::allocate(&allocator, small) {
        let payload = payload.cast::<u8>();
        unsafe { fill(payload, &[0x79; 73]); }
        live.push(payload);
    }
    assert!(live.len() > 16);
    for payload in live {
        assert_eq!(unsafe { bytes_at(payload, 73) }, vec![0x79; 73]);
        unsafe { Allocator::deallocate(&allocator, payload, small); }
    }
    let large = Layout::from_size_align(40 * 1024, 4096).unwrap();
    let payload = Allocator::allocate(&allocator, large).unwrap().cast::<u8>();
    unsafe { Allocator::deallocate(&allocator, payload, large); }
    assert_eq!(host.errno.get(), 37);
    host.balanced();
}

#[test]
fn deterministic_generated_stock_operations_match_independent_byte_and_lifetime_oracle() {
    for seed in [1, 2, 0x5eed, 0x84e1_950b, 0xdead_beef] {
        let host = Host::new(64 * 1024);
        let bridge = StockBridge::new(&host);
        let mut generator = Generator(seed);
        let mut live: Vec<Lifetime> = Vec::new();
        for step in 0..4000 {
            let action = generator.next() % 5;
            let size = match generator.next() % 16 {
                0 => 0,
                1 => u32::MAX,
                2 => MAX_STOCK_BYTES,
                _ => generator.next() % 2049,
            };
            match action {
                0 | 1 if live.len() < 80 => {
                    let zeroed = action == 1;
                    let result = if zeroed { bridge.calloc(size, 1) } else { bridge.malloc(size) }.unwrap();
                    if let Some(payload) = result {
                        if zeroed {
                            let usable = unsafe { stock_usable(payload) };
                            assert_eq!(unsafe { bytes_at(payload, usable) }, vec![0; usable]);
                        }
                        let expected = (0..size).map(|offset| (offset as u8).wrapping_add(step as u8)).collect::<Vec<_>>();
                        unsafe { fill(payload, &expected); }
                        live.push(Lifetime { payload, expected });
                    } else {
                        assert_eq!(host.errno.get(), 12);
                    }
                }
                2 if !live.is_empty() => {
                    let index = generator.next() as usize % live.len();
                    let allocation = &mut live[index];
                    if let Some(payload) = unsafe { bridge.realloc(Some(allocation.payload), size) }.unwrap() {
                        let preserved = allocation.expected.len().min(size as usize);
                        assert_eq!(unsafe { bytes_at(payload, preserved) }, allocation.expected[..preserved]);
                        let expected = (0..size).map(|offset| (offset as u8) ^ (step as u8)).collect::<Vec<_>>();
                        unsafe { fill(payload, &expected); }
                        *allocation = Lifetime { payload, expected };
                    } else {
                        assert_eq!(unsafe { bytes_at(allocation.payload, allocation.expected.len()) }, allocation.expected);
                        assert_eq!(host.errno.get(), 12);
                    }
                }
                3 | 4 if !live.is_empty() => {
                    let index = generator.next() as usize % live.len();
                    let allocation = live.swap_remove(index);
                    assert_eq!(unsafe { bytes_at(allocation.payload, allocation.expected.len()) }, allocation.expected);
                    unsafe { bridge.free(Some(allocation.payload)).unwrap(); }
                }
                _ => {}
            }
            check_lifetimes(&live);
            host.balanced();
        }
        for allocation in live {
            unsafe { bridge.free(Some(allocation.payload)).unwrap(); }
        }
        let large = bridge.malloc(40 * 1024).unwrap().unwrap();
        unsafe { bridge.free(Some(large)).unwrap(); }
        host.balanced();
    }
}
