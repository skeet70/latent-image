use core::ptr::{read_volatile, write_volatile};

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct ByteOffset(pub(super) u32);

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub(crate) struct Address32(pub(crate) u32);

impl Address32 {
    pub(super) fn offset(self, offset: ByteOffset) -> Self {
        Self(self.0.wrapping_add(offset.0))
    }

    pub(super) unsafe fn word(self) -> u32 {
        unsafe { read_volatile(self.0 as *const u32) }
    }

    pub(super) unsafe fn half(self) -> u16 {
        unsafe { read_volatile(self.0 as *const u16) }
    }

    pub(super) unsafe fn byte(self) -> u8 {
        unsafe { read_volatile(self.0 as *const u8) }
    }

    pub(super) unsafe fn write_word(self, value: u32) {
        unsafe { write_volatile(self.0 as *mut u32, value) };
    }

    pub(super) unsafe fn write_half(self, value: u16) {
        unsafe { write_volatile(self.0 as *mut u16, value) };
    }

    pub(super) unsafe fn write_byte(self, value: u8) {
        unsafe { write_volatile(self.0 as *mut u8, value) };
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct StageContext(pub(super) *mut u8);

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct FrameAddress(pub(super) Address32);

impl StageContext {
    pub(super) unsafe fn frame(self) -> FrameAddress {
        FrameAddress(Address32(unsafe { read_volatile(self.0 as *const u32) }))
    }

    pub(super) fn address(self) -> Address32 {
        Address32(self.0 as u32)
    }
}

impl FrameAddress {
    pub(super) fn field(self, offset: ByteOffset) -> Address32 {
        self.0.offset(offset)
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct OperationCode(pub(super) u32);

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct ObjectGroup(pub(super) u32);

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct ObjectKind(pub(super) i32);

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub(super) struct StageResult(pub(super) u32);

impl StageResult {
    pub(super) const SUCCESS: Self = Self(0);
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct ServiceCompletion(pub(super) u32);
