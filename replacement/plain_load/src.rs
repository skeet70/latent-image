#![no_std]

use core::arch::asm;
use core::panic::PanicInfo;

#[repr(u32)]
enum SegmentId {
    Plain = 6,
}

#[repr(transparent)]
#[derive(Clone, Copy)]
struct WordAddress(u32);

#[repr(transparent)]
#[derive(Clone, Copy)]
struct SegmentPointer(WordAddress);

#[repr(transparent)]
struct ExtentPointer(WordAddress);

#[repr(transparent)]
struct DestinationPointer(u32);

#[repr(transparent)]
struct ByteOffset(u32);

#[repr(transparent)]
struct BlockOffset(u32);

#[repr(transparent)]
struct ByteCount(u32);

#[repr(transparent)]
struct BlockCount(u32);

#[repr(transparent)]
pub struct Status(u32);

#[repr(transparent)]
struct ModeValue(u32);

enum LoadMode {
    Read,
    Skip,
}

#[repr(u32)]
#[derive(Clone, Copy)]
enum ResourceId {
    First = 0x24,
    Second = 0x34,
}

#[repr(transparent)]
struct Timeout(u32);

#[repr(u32)]
enum SkipPolicy {
    SkipBadBlocks = 1,
}

#[repr(transparent)]
struct Ticks(u32);

type Progress = unsafe extern "C" fn(ByteCount) -> Status;

extern "C" {
    fn xm1_plain_segment(id: SegmentId) -> SegmentPointer;
    fn xm1_plain_extents() -> ExtentPointer;
    fn xm1_plain_mode() -> ModeValue;
    fn xm1_plain_read(
        offset: BlockOffset,
        blocks: BlockCount,
        destination: DestinationPointer,
        progress: Progress,
        skip: SkipPolicy,
    ) -> Status;
    fn xm1_plain_progress(bytes: ByteCount) -> Status;
    fn xm1_plain_take(resource: ResourceId, timeout: Timeout) -> Status;
    fn xm1_plain_give(resource: ResourceId) -> Status;
    fn xm1_plain_delay(ticks: Ticks) -> Status;
}

impl WordAddress {
    #[inline(always)]
    unsafe fn load<const OFFSET: u32>(self) -> u32 {
        let value;
        unsafe {
            asm!("ldr {value}, [{address}, #{offset}]", value = out(reg) value,
                address = in(reg) self.0, offset = const OFFSET,
                options(nostack, preserves_flags));
        }
        value
    }
}

impl SegmentPointer {
    #[inline(always)]
    fn is_null(self) -> bool {
        self.0.0 == 0
    }

    #[inline(always)]
    unsafe fn flash_offset(self) -> ByteOffset {
        ByteOffset(unsafe { self.0.load::<20>() })
    }

    #[inline(always)]
    unsafe fn flash_length(self) -> ByteCount {
        ByteCount(unsafe { self.0.load::<24>() })
    }
}

impl ExtentPointer {
    #[inline(always)]
    unsafe fn destination(self) -> DestinationPointer {
        DestinationPointer(unsafe { self.0.load::<4>() })
    }
}

impl ModeValue {
    #[inline(always)]
    fn classify(self) -> LoadMode {
        match self.0 {
            0 => LoadMode::Read,
            _ => LoadMode::Skip,
        }
    }
}

impl ByteOffset {
    #[inline(always)]
    fn block(self) -> BlockOffset {
        BlockOffset(self.0 >> 17)
    }
}

impl ByteCount {
    #[inline(always)]
    fn rounded_blocks(self) -> BlockCount {
        BlockCount(self.0.wrapping_add(0x1ffff) >> 17)
    }
}

impl BlockCount {
    #[inline(always)]
    fn next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }

    #[inline(always)]
    fn bytes(&self) -> ByteCount {
        ByteCount(self.0 << 17)
    }
}

impl Timeout {
    const FOREVER: Self = Self(u32::MAX);
}

impl Status {
    const SUCCESS: Self = Self(0);
}

/// # Safety
/// Reached guest words must be mapped and aligned. Retained services return in A32 preserving r4-r11 and SP. Outside writers preserve executing code and live stack. The caller supplies an AAPCS stack.
#[no_mangle]
pub unsafe extern "C" fn xm1_plain_load() -> Status {
    unsafe {
        let segment = xm1_plain_segment(SegmentId::Plain);
        let extents = xm1_plain_extents();
        if segment.is_null() {
            loop {
                asm!("b .", options(nomem, nostack));
            }
        }
        let status = match xm1_plain_mode().classify() {
            LoadMode::Read => {
                let offset = segment.flash_offset();
                let blocks = segment.flash_length().rounded_blocks();
                let destination = extents.destination();
                xm1_plain_read(
                    offset.block(),
                    blocks,
                    destination,
                    xm1_plain_progress,
                    SkipPolicy::SkipBadBlocks,
                )
            }
            LoadMode::Skip => {
                let blocks = segment.flash_length().rounded_blocks();
                let _ = xm1_plain_take(ResourceId::First, Timeout::FOREVER);
                let _ = xm1_plain_take(ResourceId::Second, Timeout::FOREVER);
                let mut completed = BlockCount(0);
                while completed.0 != blocks.0 {
                    let _ = xm1_plain_delay(Ticks(14));
                    completed = completed.next();
                    let _ = xm1_plain_progress(completed.bytes());
                }
                let _ = xm1_plain_give(ResourceId::Second);
                let _ = xm1_plain_give(ResourceId::First);
                Status::SUCCESS
            }
        };
        let _ = xm1_plain_progress(segment.flash_length());
        status
    }
}

#[panic_handler]
fn panic(_: &PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
