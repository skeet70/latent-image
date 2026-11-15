//! Task43 keeps one explicitly charged general-heap stack across stop/recreate.

use core::{ptr::{addr_of_mut, read_volatile, write_volatile}, sync::atomic::{AtomicU32, Ordering}};

const TASK: u16 = 43;
const STOCK_STACK_BYTES: u32 = 0x1000;
pub const STACK_BYTES: u32 = 0x10000;
const HEAP_START: u32 = 0xa71a00;
const ALLOCATING: u32 = 1;
static STACK: AtomicU32 = AtomicU32::new(0);

extern "C" {
    fn __saclac_runtime_malloc(bytes: u32) -> u32;
    fn __saclac_runtime_create_task(configuration: *const u32) -> i32;
    // 725e0c..725e38: runtime + ((43 - 1 + 0x9600) * 8) + 0xe34.
    static mut __saclac_runtime_stack_words: [u32; 2];
}

#[repr(C)]
pub struct StackContract {
    magic: u32,
    task: u32,
    bytes: u32,
    runtime: *mut u32,
    heap_start: u32,
    heap_end: u32,
}
unsafe impl Sync for StackContract {}

#[used]
#[no_mangle]
#[link_section = ".rodata.XM1_SACLAC_TASK_STACK"]
pub static XM1_SACLAC_TASK_STACK: StackContract = StackContract {
    magic: 0x31545354,
    task: TASK as u32,
    bytes: STACK_BYTES,
    runtime: addr_of_mut!(__saclac_runtime_stack_words).cast::<u32>(),
    heap_start: HEAP_START,
    heap_end: xm1_fixed_image_layout::IMAGE_BASE,
};

/// Called instead of the single 725dd8 call at ef8d1c, with its original descriptor.
///
/// # Safety
/// `configuration` is that writable, word-aligned seven-word task43 descriptor.
#[no_mangle]
pub unsafe extern "C" fn xm1_saclac_create_task(configuration: *mut u32) -> i32 {
    assert!(!configuration.is_null() && configuration as usize & 3 == 0, "task43 descriptor");
    assert!(unsafe { read_volatile(configuration.cast::<u16>()) } == TASK, "task43 id");
    assert!(unsafe { read_volatile(configuration.add(2)) } == 0xf01bc8, "task43 entry");
    assert!(matches!(unsafe { read_volatile(configuration.add(3)) }, STOCK_STACK_BYTES | STACK_BYTES), "task43 descriptor stack");

    let runtime = addr_of_mut!(__saclac_runtime_stack_words).cast::<u32>();
    let previous = unsafe { [read_volatile(runtime), read_volatile(runtime.add(1))] };
    let mut stack = STACK.load(Ordering::Acquire);
    if stack == 0 {
        assert!(previous == [0, 0], "task43 stack already owned");
        assert!(STACK.compare_exchange(0, ALLOCATING, Ordering::AcqRel, Ordering::Acquire).is_ok(), "task43 stack reentry");
        stack = unsafe { __saclac_runtime_malloc(STACK_BYTES) } & !0x40000000;
        assert!(stack >= HEAP_START && stack & 7 == 0
            && stack.checked_add(STACK_BYTES).is_some_and(|end| end <= xm1_fixed_image_layout::IMAGE_BASE),
            "task43 stack allocation");
        STACK.store(stack, Ordering::Release);
    } else {
        assert!(stack != ALLOCATING, "task43 stack reentry");
        assert!(previous == [0, 0] || previous == [stack, STACK_BYTES], "task43 stack ownership changed");
    }
    unsafe {
        write_volatile(runtime, stack);
        write_volatile(runtime.add(1), STACK_BYTES);
        write_volatile(configuration.add(3), STACK_BYTES);
        __saclac_runtime_create_task(configuration)
    }
}
