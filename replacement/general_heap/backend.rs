use core::{alloc::Layout, mem::{size_of, MaybeUninit}, ptr::{self, NonNull}};

pub(crate) type FirstLevelBitmap = u32;
pub(crate) type SecondLevelBitmap = u16;
pub(crate) const FIRST_LEVELS: usize = 17;
pub(crate) const SECOND_LEVELS: usize = 16;
type Tlsf = rlsf::Tlsf<'static, FirstLevelBitmap, SecondLevelBitmap, FIRST_LEVELS, SECOND_LEVELS>;
/// Evaluates rlsf's compile-time geometry assertion, which `initialize` bypasses.
const _: Tlsf = Tlsf::new();

pub(crate) const STOCK_ALIGNMENT: usize = 8;
pub(crate) const PREFIX_BYTES: usize = 16;
const STOCK_MARKER: u32 = 0x584d_3148;
pub const MAX_STOCK_BYTES: u32 = 0x7fff_fff4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HeapError {
    OutOfMemory,
    InvalidPointer,
    CorruptHeader,
}

#[derive(Clone, Copy)]
pub(crate) struct StockRequest {
    bytes: u32,
    tag: u32,
    layout: Layout,
}

impl StockRequest {
    pub(crate) fn parse(bytes: u32, capacity: usize) -> Result<Self, HeapError> {
        if bytes > MAX_STOCK_BYTES {
            return Err(HeapError::OutOfMemory);
        }
        let chunk_bytes = bytes.checked_add(4).and_then(|n| n.checked_add(7))
            .map(|n| (n & !7).max(16)).ok_or(HeapError::OutOfMemory)?;
        let total = (chunk_bytes as usize).checked_sub(4)
            .and_then(|n| n.checked_add(PREFIX_BYTES)).ok_or(HeapError::OutOfMemory)?;
        let layout = checked_layout(Layout::from_size_align(total, STOCK_ALIGNMENT)
            .map_err(|_| HeapError::OutOfMemory)?, capacity)?;
        Ok(Self { bytes, tag: chunk_bytes | 1, layout })
    }
}

#[derive(Clone, Copy)]
#[repr(C)]
struct StockHeader {
    marker: u32,
    requested: u32,
    integrity: u32,
    stock_tag: u32,
}

const _: () = assert!(size_of::<StockHeader>() == PREFIX_BYTES);
const _: () = assert!(core::mem::offset_of!(StockHeader, stock_tag) == PREFIX_BYTES - 4);


pub struct Heap {
    core: Tlsf,
    pool: NonNull<u8>,
    pool_bytes: usize,
}

pub(crate) fn checked_layout(layout: Layout, capacity: usize) -> Result<Layout, HeapError> {
    let maximum_block = (rlsf::GRANULARITY << FIRST_LEVELS) - rlsf::GRANULARITY;
    let overhead = (rlsf::GRANULARITY / 2).max(layout.align());
    let charged = layout.size().checked_add(overhead)
        .and_then(|n| n.checked_add(rlsf::GRANULARITY - 1))
        .map(|n| n & !(rlsf::GRANULARITY - 1)).ok_or(HeapError::OutOfMemory)?;
    if layout.size() == 0 || charged > capacity || charged > maximum_block {
        return Err(HeapError::OutOfMemory);
    }
    Ok(layout)
}

impl Heap {
    /// # Safety
    /// As for [`Heap::initialize`].
    pub unsafe fn new(base: NonNull<u8>, bytes: usize) -> Result<Self, HeapError> {
        let mut state = MaybeUninit::<Self>::uninit();
        Self::initialize(state.as_mut_ptr(), base, bytes)?;
        Ok(state.assume_init())
    }

    /// Builds the heap in place at `state`, without a `Heap`-sized stack temporary.
    ///
    /// # Safety
    /// `state` must be writable for a `Heap`. `base..base+bytes` must be writable, exclusively owned, and live until
    /// this heap and all its allocations are discarded. The prefix charged for `Heap` may contain `state`, but must
    /// have no other writer. On error `state` is partially written and must not be used.
    pub unsafe fn initialize(state: *mut Self, base: NonNull<u8>, bytes: usize) -> Result<(), HeapError> {
        let start = base.as_ptr() as usize;
        let end = start.checked_add(bytes).ok_or(HeapError::OutOfMemory)?;
        let pool_start = start.checked_add(size_of::<Self>())
            .and_then(|n| n.checked_add(rlsf::GRANULARITY - 1))
            .map(|n| n & !(rlsf::GRANULARITY - 1)).ok_or(HeapError::OutOfMemory)?;
        let pool_bytes = end.checked_sub(pool_start).ok_or(HeapError::OutOfMemory)?;
        if pool_bytes < rlsf::GRANULARITY * 2 {
            return Err(HeapError::OutOfMemory);
        }
        let pool = NonNull::new_unchecked(base.as_ptr().add(pool_start - start));
        let core = ptr::addr_of_mut!((*state).core);
        // rlsf 0.2.3 `Tlsf::new()` is all zero bits: zero bitmaps and `None` free lists.
        core.write_bytes(0, 1);
        let admitted = (*core).insert_free_block_ptr(NonNull::slice_from_raw_parts(pool, pool_bytes))
            .ok_or(HeapError::OutOfMemory)?.get();
        ptr::addr_of_mut!((*state).pool).write(pool);
        ptr::addr_of_mut!((*state).pool_bytes).write(admitted);
        Ok(())
    }

    pub unsafe fn malloc(&mut self, bytes: u32) -> Result<NonNull<u8>, HeapError> {
        self.allocate_stock(StockRequest::parse(bytes, self.pool_bytes)?)
    }

    pub unsafe fn calloc(&mut self, count: u32, bytes: u32) -> Result<NonNull<u8>, HeapError> {
        let product = count.checked_mul(bytes).ok_or(HeapError::OutOfMemory)?;
        self.calloc_stock(StockRequest::parse(product, self.pool_bytes)?)
    }

    /// # Safety
    /// `old`, when present, must point to readable arena memory.
    pub unsafe fn realloc(&mut self, old: Option<NonNull<u8>>, bytes: u32) -> Result<NonNull<u8>, HeapError> {
        self.resize_stock(old, StockRequest::parse(bytes, self.pool_bytes)?)
    }

    /// # Safety
    /// `payload`, when present, must point to readable arena memory.
    pub unsafe fn free(&mut self, payload: Option<NonNull<u8>>) -> Result<(), HeapError> {
        let Some(payload) = payload else { return Ok(()); };
        let (raw, _) = self.stock_origin(payload)?;
        ptr::write(raw.cast::<StockHeader>().as_ptr(), StockHeader {
            marker: 0, requested: 0, integrity: 0, stock_tag: 0,
        });
        self.core.deallocate(raw, STOCK_ALIGNMENT);
        Ok(())
    }

    pub(crate) unsafe fn allocate_stock(&mut self, request: StockRequest) -> Result<NonNull<u8>, HeapError> {
        let raw = self.core.allocate(request.layout).ok_or(HeapError::OutOfMemory)?;
        Self::write_header(raw, request);
        Ok(NonNull::new_unchecked(raw.as_ptr().add(PREFIX_BYTES)))
    }

    pub(crate) unsafe fn calloc_stock(&mut self, request: StockRequest) -> Result<NonNull<u8>, HeapError> {
        let payload = self.allocate_stock(request)?;
        ptr::write_bytes(payload.as_ptr(), 0, ((request.tag & !3) - 4) as usize);
        Ok(payload)
    }

    pub(crate) unsafe fn resize_stock(&mut self, old: Option<NonNull<u8>>, request: StockRequest) -> Result<NonNull<u8>, HeapError> {
        let Some(old) = old else { return self.allocate_stock(request); };
        let (raw, _) = self.stock_origin(old)?;
        let resized = self.core.reallocate(raw, request.layout).ok_or(HeapError::OutOfMemory)?;
        Self::write_header(resized, request);
        Ok(NonNull::new_unchecked(resized.as_ptr().add(PREFIX_BYTES)))
    }

    unsafe fn write_header(raw: NonNull<u8>, request: StockRequest) {
        ptr::write(raw.cast::<StockHeader>().as_ptr(), StockHeader {
            marker: STOCK_MARKER,
            requested: request.bytes,
            integrity: integrity(raw, request.bytes, request.tag),
            stock_tag: request.tag,
        });
    }

    fn contains(&self, address: usize, bytes: usize) -> bool {
        let start = self.pool.as_ptr() as usize;
        address >= start && address.checked_add(bytes)
            .is_some_and(|end| end <= start + self.pool_bytes)
    }

    unsafe fn stock_origin(&self, payload: NonNull<u8>) -> Result<(NonNull<u8>, StockRequest), HeapError> {
        let address = payload.as_ptr() as usize;
        let raw_address = address.checked_sub(PREFIX_BYTES).ok_or(HeapError::InvalidPointer)?;
        if address & (STOCK_ALIGNMENT - 1) != 0 || !self.contains(raw_address, PREFIX_BYTES)
        {
            return Err(HeapError::InvalidPointer);
        }
        let raw = NonNull::new_unchecked(payload.as_ptr().sub(PREFIX_BYTES));
        let header = ptr::read(raw.cast::<StockHeader>().as_ptr());
        if header.marker != STOCK_MARKER {
            return Err(HeapError::CorruptHeader);
        }
        let request = StockRequest::parse(header.requested, self.pool_bytes)
            .map_err(|_| HeapError::CorruptHeader)?;
        if header.stock_tag != request.tag
            || header.integrity != integrity(raw, header.requested, header.stock_tag)
            || !self.contains(raw_address, request.layout.size())
        {
            return Err(HeapError::CorruptHeader);
        }
        Ok((raw, request))
    }

    pub unsafe fn allocate_native(&mut self, layout: Layout) -> Result<NonNull<u8>, HeapError> {
        let layout = checked_layout(layout, self.pool_bytes)?;
        self.core.allocate(layout).ok_or(HeapError::OutOfMemory)
    }

    fn validate_native_pointer(&self, payload: NonNull<u8>, layout: Layout) -> Result<(), HeapError> {
        let address = payload.as_ptr() as usize;
        if layout.size() == 0 || address & (layout.align() - 1) != 0
            || !self.contains(address, layout.size())
        {
            return Err(HeapError::InvalidPointer);
        }
        Ok(())
    }

    /// # Safety
    /// `payload` must be a live native allocation from this heap with its original `layout`.
    pub unsafe fn deallocate_native(&mut self, payload: NonNull<u8>, layout: Layout) -> Result<(), HeapError> {
        self.validate_native_pointer(payload, layout)?;
        self.core.deallocate(payload, layout.align());
        Ok(())
    }

    /// # Safety
    /// `payload` must be a live native allocation from this heap with its original `old` layout.
    pub unsafe fn resize_native(&mut self, payload: NonNull<u8>, old: Layout, new: Layout) -> Result<NonNull<u8>, HeapError> {
        let new = checked_layout(new, self.pool_bytes)?;
        self.validate_native_pointer(payload, old)?;
        let resized = if old.align() == new.align() {
            self.core.reallocate(payload, new).ok_or(HeapError::OutOfMemory)?
        } else {
            let resized = self.core.allocate(new).ok_or(HeapError::OutOfMemory)?;
            ptr::copy_nonoverlapping(payload.as_ptr(), resized.as_ptr(), old.size().min(new.size()));
            self.core.deallocate(payload, old.align());
            resized
        };
        Ok(resized)
    }
}

fn integrity(raw: NonNull<u8>, requested: u32, tag: u32) -> u32 {
    let address = raw.as_ptr() as usize as u64;
    STOCK_MARKER ^ (address as u32).rotate_left(7) ^ ((address >> 32) as u32)
        ^ requested.rotate_left(13) ^ tag.rotate_left(23)
}
