#![no_std]

use core::arch::asm;
use core::panic::PanicInfo;
use core::ptr::{addr_of, addr_of_mut};

#[repr(u32)]
enum SegmentId {
    Image5 = 5,
}

#[repr(transparent)]
#[derive(Clone, Copy)]
struct WordAddress(u32);

#[repr(transparent)]
#[derive(Clone, Copy)]
struct SegmentPointer(WordAddress);

#[repr(transparent)]
struct ByteOffset(u32);

#[repr(transparent)]
struct BlockOffset(u32);

#[repr(transparent)]
struct ByteCount(u32);

#[repr(transparent)]
struct BlockCount(u32);

#[repr(transparent)]
struct WordCount(u32);

#[repr(transparent)]
struct BssPointer(u32);

#[repr(transparent)]
struct DestinationPointer(u32);

#[repr(transparent)]
pub struct Status(u32);

#[repr(transparent)]
struct ModeValue(u32);

#[repr(transparent)]
struct FillResult(u32);

/// Log-string addresses passed to the loader-A marker writer.
#[repr(u32)]
enum MarkerId {
    MainStart = 0x7819a0,
    LoadStart = 0x7819a8,
    LoadEnd = 0x7819b0,
    DecodeStart = 0x7819b8,
    DecodeEnd = 0x7819c0,
    MainEnd = 0x7819c8,
}

#[repr(u32)]
enum StagingPointer {
    Image5 = 0x43e40000,
}

#[repr(u32)]
enum FillPattern {
    Zero = 0,
}

#[repr(u32)]
enum SkipPolicy {
    SkipBadBlocks = 1,
}

/// Both cache arguments zero select the whole cache.
#[repr(u32)]
enum CacheBound {
    Whole = 0,
}

enum LoadMode {
    Read,
    Skip,
}

enum BssPolicy {
    Requested,
    CpuFill,
}

/// The fill-done callback writes 1 here.
struct FillFlag;

type FillDone = unsafe extern "C" fn();
type Feed = unsafe extern "C" fn(ByteCount) -> Status;

extern "C" {
    static mut xm1_image5_fill_flag: u32;
    fn xm1_image5_segment(id: SegmentId) -> SegmentPointer;
    fn xm1_image5_marker(marker: MarkerId);
    fn xm1_image5_fill_request(
        base: BssPointer,
        words: WordCount,
        pattern: FillPattern,
        done: FillDone,
    ) -> FillResult;
    fn xm1_image5_fill_done();
    fn xm1_image5_mode() -> ModeValue;
    fn xm1_image5_decoder_init(destination: DestinationPointer, staging: StagingPointer);
    fn xm1_image5_read(
        offset: BlockOffset,
        blocks: BlockCount,
        staging: StagingPointer,
        feed: Option<Feed>,
        skip: SkipPolicy,
    ) -> Status;
    fn xm1_image5_feed(bytes: ByteCount) -> Status;
    fn xm1_image5_decoder_finish();
    fn xm1_image5_cpu_fill(base: BssPointer, pattern: FillPattern, length: ByteCount);
    fn xm1_image5_cache(start: CacheBound, length: CacheBound);
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
    unsafe fn destination(self) -> DestinationPointer {
        DestinationPointer(unsafe { self.0.load::<4>() })
    }

    #[inline(always)]
    unsafe fn bss_base(self) -> BssPointer {
        BssPointer(unsafe { self.0.load::<12>() })
    }

    #[inline(always)]
    unsafe fn bss_length(self) -> ByteCount {
        ByteCount(unsafe { self.0.load::<16>() })
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

impl BssPointer {
    #[inline(always)]
    fn is_word_aligned(&self) -> bool {
        self.0 & 3 == 0
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
    fn is_word_aligned(&self) -> bool {
        self.0 & 3 == 0
    }

    #[inline(always)]
    fn words(self) -> WordCount {
        WordCount(self.0 >> 2)
    }

    #[inline(always)]
    fn rounded_blocks(self) -> BlockCount {
        BlockCount(self.0.wrapping_add(0x1ffff) >> 17)
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

impl FillResult {
    #[inline(always)]
    fn policy(self) -> BssPolicy {
        match self.0 {
            0 => BssPolicy::Requested,
            _ => BssPolicy::CpuFill,
        }
    }
}

impl FillFlag {
    #[inline(always)]
    unsafe fn clear() {
        unsafe { addr_of_mut!(xm1_image5_fill_flag).write_volatile(0) }
    }

    #[inline(always)]
    unsafe fn wait() {
        while unsafe { addr_of!(xm1_image5_fill_flag).read_volatile() } == 0 {}
    }
}

/// # Safety
/// Reached guest words must be mapped and aligned. Retained services return in A32 preserving r4-r11 and SP. Outside writers preserve executing code and live stack. The caller supplies an AAPCS stack.
#[no_mangle]
pub unsafe extern "C" fn xm1_image5_load() -> Status {
    unsafe {
        let segment = xm1_image5_segment(SegmentId::Image5);
        if segment.is_null() {
            loop {}
        }
        let offset = segment.flash_offset();
        let length = segment.flash_length();
        xm1_image5_marker(MarkerId::MainStart);
        let base = segment.bss_base();
        let policy = if !base.is_word_aligned() {
            BssPolicy::CpuFill
        } else {
            let bss = segment.bss_length();
            if !bss.is_word_aligned() {
                BssPolicy::CpuFill
            } else {
                FillFlag::clear();
                xm1_image5_fill_request(base, bss.words(), FillPattern::Zero, xm1_image5_fill_done)
                    .policy()
            }
        };
        let block = offset.block();
        let blocks = length.rounded_blocks();
        let status = match xm1_image5_mode().classify() {
            LoadMode::Read => {
                xm1_image5_marker(MarkerId::LoadStart);
                xm1_image5_decoder_init(segment.destination(), StagingPointer::Image5);
                let status = xm1_image5_read(
                    block,
                    blocks,
                    StagingPointer::Image5,
                    Some(xm1_image5_feed),
                    SkipPolicy::SkipBadBlocks,
                );
                xm1_image5_marker(MarkerId::LoadEnd);
                xm1_image5_marker(MarkerId::DecodeStart);
                xm1_image5_decoder_finish();
                xm1_image5_marker(MarkerId::DecodeEnd);
                status
            }
            LoadMode::Skip => xm1_image5_read(
                block,
                blocks,
                StagingPointer::Image5,
                None,
                SkipPolicy::SkipBadBlocks,
            ),
        };
        match policy {
            BssPolicy::Requested => FillFlag::wait(),
            BssPolicy::CpuFill => {
                xm1_image5_cpu_fill(segment.bss_base(), FillPattern::Zero, segment.bss_length());
                xm1_image5_cache(CacheBound::Whole, CacheBound::Whole);
            }
        }
        xm1_image5_marker(MarkerId::MainEnd);
        status
    }
}

#[panic_handler]
fn panic(_: &PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
