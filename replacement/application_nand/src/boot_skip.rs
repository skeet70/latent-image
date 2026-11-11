use core::marker::PhantomData;
use core::ptr::read_volatile;

const CAPACITY: u32 = 314;
const PUBLICATION: *const u32 = 0x4061_c00c as *const u32;
const HEADER: u32 = 0x4061_c184;
const TABLE: u32 = 0x4310;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootSkipError { Publication, Capacity, BlockRange, Order }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootBlock(u16);

impl BootBlock {
    pub const fn index(self) -> u32 { self.0 as u32 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootSkipAddress(u32);

impl BootSkipAddress {
    pub const fn as_mut_ptr(self) -> *mut u32 { self.0 as *mut u32 }
}

pub struct BootSkipTable<'a> {
    count: u16,
    entries: *const u32,
    lifetime: PhantomData<&'a [u32]>,
}

impl<'a> BootSkipTable<'a> {
    /// # Safety
    /// The boot handoff header and workspace remain mapped and immutable for 'a. No Rust mutable references overlap them.
    pub unsafe fn from_boot() -> Result<Self, BootSkipError> {
        let header = unsafe { read_volatile(PUBLICATION) };
        if header != HEADER { return Err(BootSkipError::Publication); }
        let table = unsafe { read_volatile((header + 4) as *const u32) };
        if table != TABLE { return Err(BootSkipError::Publication); }
        let count = unsafe { read_volatile(table as *const u32) };
        if count > CAPACITY { return Err(BootSkipError::Capacity); }
        let entries = (table + 4) as *const u32;
        let mut previous = None;
        for index in 0..count {
            let block = unsafe { read_volatile(entries.add(index as usize)) };
            if block >= CAPACITY { return Err(BootSkipError::BlockRange); }
            if previous.is_some_and(|previous| block <= previous) { return Err(BootSkipError::Order); }
            previous = Some(block);
        }
        Ok(Self { count: count as u16, entries, lifetime: PhantomData })
    }

    pub fn address(&self) -> BootSkipAddress { BootSkipAddress(TABLE) }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = BootBlock> + '_ {
        (0..usize::from(self.count)).map(|index| BootBlock(unsafe { read_volatile(self.entries.add(index)) } as u16))
    }
}
