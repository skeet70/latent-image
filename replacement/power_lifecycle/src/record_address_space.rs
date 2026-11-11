use core::num::NonZeroU32;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct GuestAddress(u32);

impl GuestAddress {
    pub const fn new(value: u32) -> Self { Self(value) }
    pub const fn get(self) -> u32 { self.0 }
    pub fn checked_add(self, offset: u32) -> Result<Self, MemoryError> {
        self.0.checked_add(offset).map(Self).ok_or(MemoryError::AddressOverflow)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct ByteCount(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GuestDomain { A, B }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegionDomain { Shared, Kernel(GuestDomain) }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegionPermission { ReadOnly, ReadWrite }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccessWidth { Byte, Halfword, Word }

impl AccessWidth {
    const fn bytes(self) -> u32 {
        match self { Self::Byte => 1, Self::Halfword => 2, Self::Word => 4 }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegisterWordCount { One, Two, Three, Four }

impl RegisterWordCount {
    pub const fn get(self) -> u8 { match self { Self::One => 1, Self::Two => 2, Self::Three => 3, Self::Four => 4 } }
    pub const fn new(value: u8) -> Option<Self> {
        match value { 1 => Some(Self::One), 2 => Some(Self::Two), 3 => Some(Self::Three), 4 => Some(Self::Four), _ => None }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryError {
    AddressOverflow,
    MissingStack,
    StackExhausted,
    FrameBounds,
    EmptyRegion,
    RegionLength,
    OverlappingRegions,
    Unmapped(GuestAddress),
    ReadOnly(GuestAddress),
    Misaligned { address: GuestAddress, width: AccessWidth },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StackAlignment { Word, Doubleword }

impl StackAlignment {
    const fn mask(self) -> u64 { match self { Self::Word => 3, Self::Doubleword => 7 } }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GuestFrame { base: GuestAddress, length: ByteCount }

impl GuestFrame {
    pub const fn base(self) -> GuestAddress { self.base }
    pub const fn length(self) -> ByteCount { self.length }
    pub fn at(self, offset: ByteCount, length: ByteCount) -> Result<GuestAddress, MemoryError> {
        if u64::from(offset.0) + u64::from(length.0) > u64::from(self.length.0) { return Err(MemoryError::FrameBounds); }
        self.base.checked_add(offset.0)
    }
    pub fn subframe(self, offset: ByteCount, length: ByteCount) -> Result<Self, MemoryError> {
        Ok(Self { base: self.at(offset, length)?, length })
    }
}

pub struct GuestStack { region: GuestFrame, cursor: ByteCount }

impl GuestStack {
    pub fn new(base: GuestAddress, length: ByteCount) -> Result<Self, MemoryError> {
        if length.0 == 0 { return Err(MemoryError::EmptyRegion); }
        if u64::from(base.0) + u64::from(length.0) > 1u64 << 32 { return Err(MemoryError::AddressOverflow); }
        Ok(Self { region: GuestFrame { base, length }, cursor: length })
    }
    fn claim(&mut self, length: ByteCount, alignment: StackAlignment) -> Result<GuestFrame, MemoryError> {
        if length.0 > self.cursor.0 { return Err(MemoryError::StackExhausted); }
        let address = (u64::from(self.region.base.0) + u64::from(self.cursor.0 - length.0)) & !alignment.mask();
        let offset = address.checked_sub(u64::from(self.region.base.0)).ok_or(MemoryError::StackExhausted)?;
        self.cursor = ByteCount(offset as u32);
        Ok(GuestFrame { base: GuestAddress::new(address as u32), length })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegionDescriptor {
    base: GuestAddress,
    length: ByteCount,
    domain: RegionDomain,
    permission: RegionPermission,
}

impl RegionDescriptor {
    pub fn new(base: GuestAddress, length: ByteCount, domain: RegionDomain, permission: RegionPermission) -> Result<Self, MemoryError> {
        if length.0 == 0 { return Err(MemoryError::EmptyRegion); }
        if u64::from(base.0) + u64::from(length.0) > (1u64 << 32) { return Err(MemoryError::AddressOverflow); }
        Ok(Self { base, length, domain, permission })
    }
    pub const fn base(self) -> GuestAddress { self.base }
    pub const fn length(self) -> ByteCount { self.length }
    pub const fn domain(self) -> RegionDomain { self.domain }
    pub const fn permission(self) -> RegionPermission { self.permission }
    fn visible(self, domain: GuestDomain) -> bool {
        self.domain == RegionDomain::Shared || self.domain == RegionDomain::Kernel(domain)
    }
    fn contains(self, address: GuestAddress, length: ByteCount) -> bool {
        u64::from(address.0) >= u64::from(self.base.0)
            && u64::from(address.0) + u64::from(length.0) <= u64::from(self.base.0) + u64::from(self.length.0)
    }
    fn overlaps(self, other: Self) -> bool {
        u64::from(self.base.0) < u64::from(other.base.0) + u64::from(other.length.0)
            && u64::from(other.base.0) < u64::from(self.base.0) + u64::from(self.length.0)
    }
}

pub trait GuestAddressSpace {
    fn domain(&self) -> GuestDomain;
    fn read_u8(&mut self, address: GuestAddress) -> Result<u8, MemoryError>;
    fn read_u16(&mut self, address: GuestAddress) -> Result<u16, MemoryError>;
    fn read_u32(&mut self, address: GuestAddress) -> Result<u32, MemoryError>;
    fn write_u8(&mut self, address: GuestAddress, value: u8) -> Result<(), MemoryError>;
    fn write_u16(&mut self, address: GuestAddress, value: u16) -> Result<(), MemoryError>;
    fn write_u32(&mut self, address: GuestAddress, value: u32) -> Result<(), MemoryError>;
    fn copy_bytes(&mut self, destination: GuestAddress, source: GuestAddress, length: ByteCount) -> Result<(), MemoryError>;
    fn copy_register_words(&mut self, destination: GuestAddress, source: GuestAddress, words: RegisterWordCount) -> Result<(), MemoryError> {
        let mut registers = [0; 4];
        for index in 0..words.get() {
            registers[index as usize] = self.read_u32(source.checked_add(u32::from(index) * 4)?)?;
        }
        for index in 0..words.get() {
            self.write_u32(destination.checked_add(u32::from(index) * 4)?, registers[index as usize])?;
        }
        Ok(())
    }
}

fn alignment(address: GuestAddress, width: AccessWidth) -> Result<(), MemoryError> {
    if address.0 & (width.bytes() - 1) != 0 { Err(MemoryError::Misaligned { address, width }) } else { Ok(()) }
}

pub struct MemoryRegion<'memory> {
    descriptor: RegionDescriptor,
    bytes: &'memory mut [u8],
}

impl<'memory> MemoryRegion<'memory> {
    pub fn new(descriptor: RegionDescriptor, bytes: &'memory mut [u8]) -> Result<Self, MemoryError> {
        if bytes.len() as u64 != u64::from(descriptor.length.0) { return Err(MemoryError::RegionLength); }
        Ok(Self { descriptor, bytes })
    }
    pub const fn descriptor(&self) -> RegionDescriptor { self.descriptor }
    pub fn bytes(&self) -> &[u8] { self.bytes }
}

pub struct HostAddressSpace<'regions, 'memory> {
    domain: GuestDomain,
    regions: &'regions mut [MemoryRegion<'memory>],
}

impl<'regions, 'memory> HostAddressSpace<'regions, 'memory> {
    pub fn new(domain: GuestDomain, regions: &'regions mut [MemoryRegion<'memory>]) -> Result<Self, MemoryError> {
        for (index, region) in regions.iter().enumerate() {
            if !region.descriptor.visible(domain) { continue; }
            if regions[..index].iter().any(|other| other.descriptor.visible(domain) && region.descriptor.overlaps(other.descriptor)) {
                return Err(MemoryError::OverlappingRegions);
            }
        }
        Ok(Self { domain, regions })
    }
    fn locate(&self, address: GuestAddress, length: ByteCount, write: bool) -> Result<(usize, usize), MemoryError> {
        let index = self.regions.iter().position(|region| region.descriptor.visible(self.domain) && region.descriptor.contains(address, length)).ok_or(MemoryError::Unmapped(address))?;
        let descriptor = self.regions[index].descriptor;
        if write && descriptor.permission == RegionPermission::ReadOnly { return Err(MemoryError::ReadOnly(address)); }
        Ok((index, (address.0 - descriptor.base.0) as usize))
    }
    fn load<const N: usize>(&self, address: GuestAddress) -> Result<[u8; N], MemoryError> {
        let (region, offset) = self.locate(address, ByteCount(N as u32), false)?;
        let mut value = [0; N];
        value.copy_from_slice(&self.regions[region].bytes[offset..offset + N]);
        Ok(value)
    }
    fn store<const N: usize>(&mut self, address: GuestAddress, value: [u8; N]) -> Result<(), MemoryError> {
        let (region, offset) = self.locate(address, ByteCount(N as u32), true)?;
        self.regions[region].bytes[offset..offset + N].copy_from_slice(&value);
        Ok(())
    }
}

impl GuestAddressSpace for HostAddressSpace<'_, '_> {
    fn domain(&self) -> GuestDomain { self.domain }
    fn read_u8(&mut self, address: GuestAddress) -> Result<u8, MemoryError> { Ok(self.load::<1>(address)?[0]) }
    fn read_u16(&mut self, address: GuestAddress) -> Result<u16, MemoryError> { alignment(address, AccessWidth::Halfword)?; Ok(u16::from_le_bytes(self.load(address)?)) }
    fn read_u32(&mut self, address: GuestAddress) -> Result<u32, MemoryError> { alignment(address, AccessWidth::Word)?; Ok(u32::from_le_bytes(self.load(address)?)) }
    fn write_u8(&mut self, address: GuestAddress, value: u8) -> Result<(), MemoryError> { self.store(address, [value]) }
    fn write_u16(&mut self, address: GuestAddress, value: u16) -> Result<(), MemoryError> { alignment(address, AccessWidth::Halfword)?; self.store(address, value.to_le_bytes()) }
    fn write_u32(&mut self, address: GuestAddress, value: u32) -> Result<(), MemoryError> { alignment(address, AccessWidth::Word)?; self.store(address, value.to_le_bytes()) }
    fn copy_bytes(&mut self, destination: GuestAddress, source: GuestAddress, length: ByteCount) -> Result<(), MemoryError> {
        if length.0 == 0 { return Ok(()); }
        let (source_region, source_offset) = self.locate(source, length, false)?;
        let (destination_region, destination_offset) = self.locate(destination, length, true)?;
        if source_region == destination_region {
            self.regions[source_region].bytes.copy_within(source_offset..source_offset + length.0 as usize, destination_offset);
        } else {
            for offset in 0..length.0 as usize {
                let value = self.regions[source_region].bytes[source_offset + offset];
                self.regions[destination_region].bytes[destination_offset + offset] = value;
            }
        }
        Ok(())
    }
}

#[cfg(target_arch = "arm")]
pub struct NativeAddressSpace<'regions> {
    domain: GuestDomain,
    regions: &'regions [RegionDescriptor],
}

#[cfg(target_arch = "arm")]
impl<'regions> NativeAddressSpace<'regions> {
    /// Every admitted region must remain mapped for this borrow; scalar reads require initialized fields.
    pub unsafe fn new(domain: GuestDomain, regions: &'regions [RegionDescriptor]) -> Result<Self, MemoryError> {
        for (index, region) in regions.iter().enumerate() {
            if region.visible(domain) && regions[..index].iter().any(|other| other.visible(domain) && region.overlaps(*other)) {
                return Err(MemoryError::OverlappingRegions);
            }
        }
        Ok(Self { domain, regions })
    }
    fn access(&self, address: GuestAddress, length: ByteCount, write: bool) -> Result<(), MemoryError> {
        let region = self.regions.iter().find(|region| region.visible(self.domain) && region.contains(address, length)).ok_or(MemoryError::Unmapped(address))?;
        if write && region.permission == RegionPermission::ReadOnly { return Err(MemoryError::ReadOnly(address)); }
        Ok(())
    }
}

#[cfg(target_arch = "arm")]
impl GuestAddressSpace for NativeAddressSpace<'_> {
    fn domain(&self) -> GuestDomain { self.domain }
    fn read_u8(&mut self, address: GuestAddress) -> Result<u8, MemoryError> { self.access(address, ByteCount(1), false)?; Ok(unsafe { core::ptr::read_volatile(address.0 as *const u8) }) }
    fn read_u16(&mut self, address: GuestAddress) -> Result<u16, MemoryError> { alignment(address, AccessWidth::Halfword)?; self.access(address, ByteCount(2), false)?; Ok(unsafe { core::ptr::read_volatile(address.0 as *const u16) }) }
    fn read_u32(&mut self, address: GuestAddress) -> Result<u32, MemoryError> { alignment(address, AccessWidth::Word)?; self.access(address, ByteCount(4), false)?; Ok(unsafe { core::ptr::read_volatile(address.0 as *const u32) }) }
    fn write_u8(&mut self, address: GuestAddress, value: u8) -> Result<(), MemoryError> { self.access(address, ByteCount(1), true)?; unsafe { core::ptr::write_volatile(address.0 as *mut u8, value) }; Ok(()) }
    fn write_u16(&mut self, address: GuestAddress, value: u16) -> Result<(), MemoryError> { alignment(address, AccessWidth::Halfword)?; self.access(address, ByteCount(2), true)?; unsafe { core::ptr::write_volatile(address.0 as *mut u16, value) }; Ok(()) }
    fn write_u32(&mut self, address: GuestAddress, value: u32) -> Result<(), MemoryError> { alignment(address, AccessWidth::Word)?; self.access(address, ByteCount(4), true)?; unsafe { core::ptr::write_volatile(address.0 as *mut u32, value) }; Ok(()) }
    fn copy_bytes(&mut self, destination: GuestAddress, source: GuestAddress, length: ByteCount) -> Result<(), MemoryError> {
        if length.0 == 0 { return Ok(()); }
        self.access(source, length, false)?;
        self.access(destination, length, true)?;
        let copy = |offset: u32| unsafe {
            let value = core::ptr::read_volatile(source.0.wrapping_add(offset) as *const core::mem::MaybeUninit<u8>);
            core::ptr::write_volatile(destination.0.wrapping_add(offset) as *mut core::mem::MaybeUninit<u8>, value);
        };
        if destination.0 > source.0 && u64::from(destination.0) < u64::from(source.0) + u64::from(length.0) {
            for offset in (0..length.0).rev() { copy(offset); }
        } else {
            for offset in 0..length.0 { copy(offset); }
        }
        Ok(())
    }
    fn copy_register_words(&mut self, destination: GuestAddress, source: GuestAddress, words: RegisterWordCount) -> Result<(), MemoryError> {
        let mut registers = [core::mem::MaybeUninit::<u32>::uninit(); 4];
        for index in 0..words.get() {
            let address = source.checked_add(u32::from(index) * 4)?;
            alignment(address, AccessWidth::Word)?;
            self.access(address, ByteCount(4), false)?;
            registers[index as usize] = unsafe { core::ptr::read_volatile(address.0 as *const core::mem::MaybeUninit<u32>) };
        }
        for index in 0..words.get() {
            let address = destination.checked_add(u32::from(index) * 4)?;
            alignment(address, AccessWidth::Word)?;
            self.access(address, ByteCount(4), true)?;
            unsafe { core::ptr::write_volatile(address.0 as *mut core::mem::MaybeUninit<u32>, registers[index as usize]) };
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct CallbackKey(NonZeroU32);

impl CallbackKey {
    pub const fn new(value: u32) -> Option<Self> {
        match NonZeroU32::new(value) { Some(value) => Some(Self(value)), None => None }
    }
    pub const fn get(self) -> u32 { self.0.get() }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallbackSignature {
    Completion, Status, StatusWord, StatusTwoWords, StatusThreeWords, StatusFourWords,
    StatusFiveWords, NineWords, ExposureAdjustment, ExtendedPayload, ApplicationRecord, TransitionAction,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CallbackId {
    key: CallbackKey,
    generation: NonZeroU32,
    signature: CallbackSignature,
}

impl CallbackId {
    pub const fn key(self) -> CallbackKey { self.key }
    pub const fn signature(self) -> CallbackSignature { self.signature }
    pub const fn generation(self) -> NonZeroU32 { self.generation }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallbackError { Capacity, DuplicateKey, GenerationExhausted, Unregistered, Signature }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordError {
    Memory(MemoryError),
    Kernel(crate::shutdown::KernelStatus),
    Callback(CallbackError),
}

impl From<MemoryError> for RecordError { fn from(error: MemoryError) -> Self { Self::Memory(error) } }
impl From<crate::shutdown::KernelStatus> for RecordError { fn from(error: crate::shutdown::KernelStatus) -> Self { Self::Kernel(error) } }
impl From<CallbackError> for RecordError { fn from(error: CallbackError) -> Self { Self::Callback(error) } }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExposureAdjustment { pub words: [u32; 13] }

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtendedPayload { pub registers: [u32; 4], pub stack: [u32; 257] }

pub enum CallbackHandler<Context> {
    Completion(fn(&mut Context) -> Result<(), RecordError>),
    Status(fn(&mut Context, u32) -> Result<(), RecordError>),
    StatusWord(fn(&mut Context, u32, u32) -> Result<(), RecordError>),
    StatusTwoWords(fn(&mut Context, u32, u32, u32) -> Result<(), RecordError>),
    StatusThreeWords(fn(&mut Context, u32, u32, u32, u32) -> Result<(), RecordError>),
    StatusFourWords(fn(&mut Context, u32, u32, u32, u32, u32) -> Result<(), RecordError>),
    StatusFiveWords(fn(&mut Context, u32, u32, u32, u32, u32, u32) -> Result<(), RecordError>),
    NineWords(fn(&mut Context, &[u32; 9]) -> Result<(), RecordError>),
    ExposureAdjustment(fn(&mut Context, &ExposureAdjustment) -> Result<(), RecordError>),
    ExtendedPayload(fn(&mut Context, &ExtendedPayload) -> Result<(), RecordError>),
    ApplicationRecord(fn(&mut Context, &[u8; 76]) -> Result<(), RecordError>),
    TransitionAction(fn(&mut Context, u32, u32) -> Result<u32, RecordError>),
}

impl<Context> Copy for CallbackHandler<Context> {}
impl<Context> Clone for CallbackHandler<Context> { fn clone(&self) -> Self { *self } }

impl<Context> CallbackHandler<Context> {
    pub const fn signature(&self) -> CallbackSignature {
        match self {
            Self::Completion(_) => CallbackSignature::Completion,
            Self::Status(_) => CallbackSignature::Status,
            Self::StatusWord(_) => CallbackSignature::StatusWord,
            Self::StatusTwoWords(_) => CallbackSignature::StatusTwoWords,
            Self::StatusThreeWords(_) => CallbackSignature::StatusThreeWords,
            Self::StatusFourWords(_) => CallbackSignature::StatusFourWords,
            Self::StatusFiveWords(_) => CallbackSignature::StatusFiveWords,
            Self::NineWords(_) => CallbackSignature::NineWords,
            Self::ExposureAdjustment(_) => CallbackSignature::ExposureAdjustment,
            Self::ExtendedPayload(_) => CallbackSignature::ExtendedPayload,
            Self::ApplicationRecord(_) => CallbackSignature::ApplicationRecord,
            Self::TransitionAction(_) => CallbackSignature::TransitionAction,
        }
    }
}

struct CallbackEntry<Context> { id: CallbackId, handler: CallbackHandler<Context> }

pub struct CallbackRegistry<Context, const N: usize> {
    entries: [Option<CallbackEntry<Context>>; N],
    generation: u32,
}

impl<Context, const N: usize> Default for CallbackRegistry<Context, N> {
    fn default() -> Self { Self::new() }
}

impl<Context, const N: usize> CallbackRegistry<Context, N> {
    pub const fn new() -> Self { Self { entries: [const { None }; N], generation: 0 } }
    pub fn register(&mut self, key: CallbackKey, handler: CallbackHandler<Context>) -> Result<CallbackId, CallbackError> {
        if self.entries.iter().flatten().any(|entry| entry.id.key == key) { return Err(CallbackError::DuplicateKey); }
        let slot = self.entries.iter().position(Option::is_none).ok_or(CallbackError::Capacity)?;
        let generation = self.generation.checked_add(1).and_then(NonZeroU32::new).ok_or(CallbackError::GenerationExhausted)?;
        let id = CallbackId { key, generation, signature: handler.signature() };
        self.entries[slot] = Some(CallbackEntry { id, handler });
        self.generation = generation.get();
        Ok(id)
    }
    pub fn resolve(&self, key: CallbackKey, signature: CallbackSignature) -> Result<CallbackId, CallbackError> {
        let entry = self.entries.iter().flatten().find(|entry| entry.id.key == key).ok_or(CallbackError::Unregistered)?;
        if entry.id.signature != signature { return Err(CallbackError::Signature); }
        Ok(entry.id)
    }
    pub fn handler(&self, id: CallbackId) -> Result<CallbackHandler<Context>, CallbackError> {
        self.entries.iter().flatten().find(|entry| entry.id == id).map(|entry| entry.handler).ok_or(CallbackError::Unregistered)
    }
    pub fn lookup(&self, key: CallbackKey, signature: CallbackSignature) -> Result<CallbackHandler<Context>, CallbackError> {
        self.handler(self.resolve(key, signature)?)
    }
    pub fn remove(&mut self, id: CallbackId) -> Result<(), CallbackError> {
        let slot = self.entries.iter().position(|entry| entry.as_ref().is_some_and(|entry| entry.id == id)).ok_or(CallbackError::Unregistered)?;
        self.entries[slot] = None;
        Ok(())
    }
    pub fn invoke_completion(&self, id: CallbackId, context: &mut Context) -> Result<(), RecordError> {
        match self.handler(id)? { CallbackHandler::Completion(callback) => callback(context), _ => Err(CallbackError::Signature.into()) }
    }
    pub fn invoke_status(&self, id: CallbackId, context: &mut Context, status: u32) -> Result<(), RecordError> {
        match self.handler(id)? { CallbackHandler::Status(callback) => callback(context, status), _ => Err(CallbackError::Signature.into()) }
    }
    pub fn invoke_status_word(&self, id: CallbackId, context: &mut Context, status: u32, word: u32) -> Result<(), RecordError> {
        match self.handler(id)? { CallbackHandler::StatusWord(callback) => callback(context, status, word), _ => Err(CallbackError::Signature.into()) }
    }
    pub fn invoke_status_two_words(&self, id: CallbackId, context: &mut Context, status: u32, words: [u32; 2]) -> Result<(), RecordError> {
        match self.handler(id)? { CallbackHandler::StatusTwoWords(callback) => callback(context, status, words[0], words[1]), _ => Err(CallbackError::Signature.into()) }
    }
    pub fn invoke_status_three_words(&self, id: CallbackId, context: &mut Context, status: u32, words: [u32; 3]) -> Result<(), RecordError> {
        match self.handler(id)? { CallbackHandler::StatusThreeWords(callback) => callback(context, status, words[0], words[1], words[2]), _ => Err(CallbackError::Signature.into()) }
    }
    pub fn invoke_status_four_words(&self, id: CallbackId, context: &mut Context, status: u32, words: [u32; 4]) -> Result<(), RecordError> {
        match self.handler(id)? { CallbackHandler::StatusFourWords(callback) => callback(context, status, words[0], words[1], words[2], words[3]), _ => Err(CallbackError::Signature.into()) }
    }
    pub fn invoke_status_five_words(&self, id: CallbackId, context: &mut Context, status: u32, words: [u32; 5]) -> Result<(), RecordError> {
        match self.handler(id)? { CallbackHandler::StatusFiveWords(callback) => callback(context, status, words[0], words[1], words[2], words[3], words[4]), _ => Err(CallbackError::Signature.into()) }
    }
    pub fn invoke_nine_words(&self, id: CallbackId, context: &mut Context, words: &[u32; 9]) -> Result<(), RecordError> {
        match self.handler(id)? { CallbackHandler::NineWords(callback) => callback(context, words), _ => Err(CallbackError::Signature.into()) }
    }
    pub fn invoke_exposure_adjustment(&self, id: CallbackId, context: &mut Context, words: &ExposureAdjustment) -> Result<(), RecordError> {
        match self.handler(id)? { CallbackHandler::ExposureAdjustment(callback) => callback(context, words), _ => Err(CallbackError::Signature.into()) }
    }
    pub fn invoke_extended_payload(&self, id: CallbackId, context: &mut Context, payload: &ExtendedPayload) -> Result<(), RecordError> {
        match self.handler(id)? { CallbackHandler::ExtendedPayload(callback) => callback(context, payload), _ => Err(CallbackError::Signature.into()) }
    }
    pub fn invoke_application_record(&self, id: CallbackId, context: &mut Context, record: &[u8; 76]) -> Result<(), RecordError> {
        match self.handler(id)? { CallbackHandler::ApplicationRecord(callback) => callback(context, record), _ => Err(CallbackError::Signature.into()) }
    }
    pub fn invoke_transition_action(&self, id: CallbackId, context: &mut Context, event: u32, argument: u32) -> Result<u32, RecordError> {
        match self.handler(id)? { CallbackHandler::TransitionAction(callback) => callback(context, event, argument), _ => Err(CallbackError::Signature.into()) }
    }
}

pub struct RecordContext<Memory, Effects> {
    pub memory: Memory,
    pub effects: Effects,
    stack: Option<GuestStack>,
}

impl<Memory, Effects> RecordContext<Memory, Effects> {
    pub const fn new(memory: Memory, effects: Effects) -> Self { Self { memory, effects, stack: None } }
    pub const fn with_stack(memory: Memory, effects: Effects, stack: GuestStack) -> Self { Self { memory, effects, stack: Some(stack) } }
    pub fn with_guest_frame<R, Error: From<MemoryError>>(&mut self, length: ByteCount, alignment: StackAlignment, body: impl FnOnce(&mut Self, GuestFrame) -> Result<R, Error>) -> Result<R, Error> {
        let stack = self.stack.as_mut().ok_or(MemoryError::MissingStack).map_err(Error::from)?;
        let checkpoint = stack.cursor;
        let frame = stack.claim(length, alignment).map_err(Error::from)?;
        let result = body(self, frame);
        if let Some(stack) = self.stack.as_mut() { stack.cursor = checkpoint; }
        result
    }
}

impl<Memory: GuestAddressSpace, Effects> GuestAddressSpace for RecordContext<Memory, Effects> {
    fn domain(&self) -> GuestDomain { self.memory.domain() }
    fn read_u8(&mut self, address: GuestAddress) -> Result<u8, MemoryError> { self.memory.read_u8(address) }
    fn read_u16(&mut self, address: GuestAddress) -> Result<u16, MemoryError> { self.memory.read_u16(address) }
    fn read_u32(&mut self, address: GuestAddress) -> Result<u32, MemoryError> { self.memory.read_u32(address) }
    fn write_u8(&mut self, address: GuestAddress, value: u8) -> Result<(), MemoryError> { self.memory.write_u8(address, value) }
    fn write_u16(&mut self, address: GuestAddress, value: u16) -> Result<(), MemoryError> { self.memory.write_u16(address, value) }
    fn write_u32(&mut self, address: GuestAddress, value: u32) -> Result<(), MemoryError> { self.memory.write_u32(address, value) }
    fn copy_bytes(&mut self, destination: GuestAddress, source: GuestAddress, length: ByteCount) -> Result<(), MemoryError> { self.memory.copy_bytes(destination, source, length) }
    fn copy_register_words(&mut self, destination: GuestAddress, source: GuestAddress, words: RegisterWordCount) -> Result<(), MemoryError> { self.memory.copy_register_words(destination, source, words) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor(base: u32, length: u32, domain: RegionDomain, permission: RegionPermission) -> RegionDescriptor {
        RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), domain, permission).unwrap()
    }

    #[test]
    fn domains_permissions_widths_and_overlapping_copies() {
        let mut shared = [0x91, 0x82, 0x73, 0x64, 0x55, 0x46, 0x37, 0x28];
        let mut a = [0x11; 4];
        let mut b = [0x22; 4];
        let mut rom = [0x33; 4];
        let mut regions = [
            MemoryRegion::new(descriptor(0x1000, 8, RegionDomain::Shared, RegionPermission::ReadWrite), &mut shared).unwrap(),
            MemoryRegion::new(descriptor(0x2000, 4, RegionDomain::Kernel(GuestDomain::A), RegionPermission::ReadWrite), &mut a).unwrap(),
            MemoryRegion::new(descriptor(0x2000, 4, RegionDomain::Kernel(GuestDomain::B), RegionPermission::ReadWrite), &mut b).unwrap(),
            MemoryRegion::new(descriptor(0x3000, 4, RegionDomain::Shared, RegionPermission::ReadOnly), &mut rom).unwrap(),
        ];
        let mut memory = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        assert_eq!(memory.read_u32(GuestAddress::new(0x1000)), Ok(0x6473_8291));
        assert_eq!(memory.read_u32(GuestAddress::new(0x2000)), Ok(0x1111_1111));
        assert_eq!(memory.write_u8(GuestAddress::new(0x3000), 9), Err(MemoryError::ReadOnly(GuestAddress::new(0x3000))));
        assert!(matches!(memory.read_u16(GuestAddress::new(0x1001)), Err(MemoryError::Misaligned { .. })));
        memory.copy_bytes(GuestAddress::new(0x1002), GuestAddress::new(0x1000), ByteCount(6)).unwrap();
        assert_eq!(memory.read_u32(GuestAddress::new(0x1004)), Ok(0x4655_6473));
        let mut memory = HostAddressSpace::new(GuestDomain::B, &mut regions).unwrap();
        assert_eq!(memory.read_u32(GuestAddress::new(0x2000)), Ok(0x2222_2222));
    }

    #[test]
    fn callbacks_require_owned_matching_live_registration() {
        fn status(context: &mut u32, value: u32) -> Result<(), RecordError> { *context = value; Ok(()) }
        let key = CallbackKey::new(0x1234).unwrap();
        let mut registry = CallbackRegistry::<u32, 1>::new();
        let first = registry.register(key, CallbackHandler::Status(status)).unwrap();
        let mut result = 0;
        registry.invoke_status(first, &mut result, 17).unwrap();
        assert_eq!(result, 17);
        assert_eq!(registry.resolve(key, CallbackSignature::StatusWord), Err(CallbackError::Signature));
        registry.remove(first).unwrap();
        let second = registry.register(key, CallbackHandler::Status(status)).unwrap();
        assert_eq!(registry.invoke_status(first, &mut result, 99), Err(RecordError::Callback(CallbackError::Unregistered)));
        registry.invoke_status(second, &mut result, 23).unwrap();
        assert_eq!(result, 23);
    }
}
