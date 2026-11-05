#![no_std]

use core::mem::MaybeUninit;
use core::panic::PanicInfo;
use core::ptr::{read_volatile, write_volatile};

#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop { core::hint::spin_loop(); } }

#[repr(transparent)]
#[derive(Clone, Copy)]
struct TaskId(u32);
#[repr(transparent)]
#[derive(Clone, Copy)]
struct GuestAddress(u32);
#[repr(transparent)]
#[derive(Clone, Copy)]
struct Priority(u32);
#[repr(transparent)]
#[derive(Clone, Copy)]
struct StackBytes(u32);
#[repr(transparent)]
#[derive(Clone, Copy)]
struct Auxiliary(u32);

#[repr(C)]
struct Queue { attributes: u32, message_bytes: u32 }
// The linked frame must preserve the stock auxiliary slot and task80's argument slot.
#[repr(C)]
struct Descriptor {
    task: TaskId,
    name: GuestAddress,
    priority: Priority,
    stack: StackBytes,
    entry: GuestAddress,
    argument: MaybeUninit<u32>,
    auxiliary: MaybeUninit<Auxiliary>,
}

extern "C" {
    fn xm1_create_queue(task: TaskId, configuration: &Queue) -> i32;
    fn xm1_submit_task(descriptor: &Descriptor) -> i32;
    fn xm1_usb_clear() -> i32;
    fn xm1_record_initialize(task: TaskId) -> i32;
    fn xm1_record_service() -> i32;
    fn xm1_record_finish() -> i32;
    fn xm1_diag_initialize() -> i32;
    fn xm1_sac_prepare() -> i32;
    fn xm1_sac_empty() -> i32;
    fn xm1_sac_optional() -> i32;
    fn xm1_sac_capture() -> i32;
    fn xm1_sac_f4c368() -> i32;
    fn xm1_sac_10546c0() -> i32;
    fn xm1_sac_1039500() -> i32;
    fn xm1_sac_eecbf4() -> i32;
    fn xm1_sac_1009974() -> i32;
    fn xm1_sac_1050efc() -> i32;
    fn xm1_sac_1051f30() -> i32;
    fn xm1_sac_1053854() -> i32;
    fn xm1_sac_reset() -> i32;
}

#[inline(always)]
unsafe fn queue(task: TaskId, message_bytes: u32) -> i32 {
    xm1_create_queue(task, &Queue { attributes: 0x30, message_bytes })
}

#[inline(always)]
unsafe fn submit(task: TaskId, name: GuestAddress, priority: Priority, stack: StackBytes, entry: GuestAddress, argument: u32) -> i32 {
    xm1_submit_task(&Descriptor { task, name, priority, stack, entry, argument: MaybeUninit::new(argument), auxiliary: MaybeUninit::uninit() })
}

macro_rules! constructor {
    ($name:ident, $section:literal, $id:expr, $bytes:expr, $label:expr, $priority:expr, $stack:expr, $entry:expr) => {
        #[no_mangle]
        #[link_section = $section]
        pub unsafe extern "C" fn $name() -> i32 {
            let task = TaskId($id);
            let descriptor = Descriptor { task, name: GuestAddress($label), priority: Priority($priority), stack: StackBytes($stack), entry: GuestAddress($entry), argument: MaybeUninit::new(0), auxiliary: MaybeUninit::uninit() };
            let result = queue(task, $bytes);
            if result != 0 { return result; }
            xm1_submit_task(&descriptor)
        }
    };
}

constructor!(xm1_ctor_d0a238_body, ".ctor.d0a238.body", 1, 0x54, 0xd0a2b0, 6, 0xc00, 0xc22f60);
constructor!(xm1_ctor_ec5350_body, ".ctor.ec5350.body", 2, 0x100, 0xec53e8, 6, 0x1000, 0xec2668);
constructor!(xm1_ctor_db63e0_body, ".ctor.db63e0.body", 4, 0xcc, 0xdb6458, 6, 0x1000, 0x1215480);
constructor!(xm1_ctor_ed10b4_body, ".ctor.ed10b4.body", 7, 0x54, 0xed1354, 6, 0xc00, 0xecc058);
constructor!(xm1_ctor_ed2b00_body, ".ctor.ed2b00.body", 35, 0xc, 0xed3adc, 9, 0xc00, 0xed2ce0);
constructor!(xm1_ctor_1055d10_body, ".ctor.1055d10.body", 64, 0x40, 0x1055e04, 6, 0xc00, 0x1054cf4);

#[inline(always)]
fn mapped(status: i32) -> i32 { if status == 0 { 0 } else { -255 } }

#[no_mangle]
#[link_section = ".ctor.d80d44.body"]
pub unsafe extern "C" fn xm1_ctor_d80d44_body() -> i32 {
    let task = TaskId(36);
    if queue(task, 0x20) != 0 { return -255; }
    if submit(task, GuestAddress(0xd80dd4), Priority(11), StackBytes(0xc00), GuestAddress(0xd81218), 0) != 0 { return -255; }
    xm1_usb_clear();
    0
}

#[no_mangle]
#[link_section = ".ctor.dca604.body"]
pub unsafe extern "C" fn xm1_ctor_dca604_body() -> i32 {
    write_volatile(0x1526a7c as *mut u32, 44);
    write_volatile(0x1526a80 as *mut u32, 0x1526a84);
    let task = TaskId(3);
    let status = queue(task, 0x34);
    if status != 0 { return status; }
    let status = submit(task, GuestAddress(0xdca790), Priority(6), StackBytes(0x1000), GuestAddress(0xe00a80), 0);
    if status != 0 { return status; }
    xm1_record_initialize(task);
    xm1_record_service();
    xm1_record_finish();
    0
}

macro_rules! hook_constructor {
    ($name:ident, $section:literal, $id:expr, $bytes:expr, $label:expr, $priority:expr, $stack:expr, $entry:expr, $always:expr) => {
        #[no_mangle]
        #[link_section = $section]
        pub unsafe extern "C" fn $name() -> i32 {
            let task = TaskId($id);
            let channel = queue(task, $bytes);
            if !$always && channel != 0 { return -255; }
            let status = submit(task, GuestAddress($label), Priority($priority), StackBytes($stack), GuestAddress($entry), 0);
            mapped(channel | status)
        }
    };
}
hook_constructor!(xm1_ctor_10726f4_body, ".ctor.10726f4.body", 56, 0x10, 0x1072960, 12, 0x1400, 0x1071ca4, false);
hook_constructor!(xm1_ctor_1066280_body, ".ctor.1066280.body", 55, 0x18, 0x1066474, 7, 0xc00, 0x106ceec, true);
hook_constructor!(xm1_ctor_1062090_body, ".ctor.1062090.body", 62, 0xb0, 0x1062414, 12, 0x1000, 0x1064224, true);

#[no_mangle]
#[link_section = ".ctor.69f000.body"]
pub unsafe extern "C" fn xm1_ctor_69f000_body() -> i32 {
    let task = TaskId(80);
    if queue(task, 0x10) != 0 { return -255; }
    let descriptor = Descriptor { task, name: GuestAddress(0x781994), priority: Priority(6), stack: StackBytes(0xc00), entry: GuestAddress(0x69f0e0), argument: MaybeUninit::uninit(), auxiliary: MaybeUninit::uninit() };
    mapped(xm1_submit_task(&descriptor))
}

#[no_mangle]
#[link_section = ".ctor.ef8c90.body"]
pub unsafe extern "C" fn xm1_ctor_ef8c90_body(callback: Option<unsafe extern "C" fn(i32) -> i32>) -> i32 {
    xm1_diag_initialize();
    xm1_sac_prepare();
    xm1_sac_empty();
    write_volatile(0x169d374 as *mut u16, 0x3ba2);
    write_volatile(0x169d370 as *mut u32, 43);
    let task = TaskId(43);
    let channel = queue(task, 0xb0);
    submit(task, GuestAddress(0xef9a8c), Priority(5), StackBytes(0x1000), GuestAddress(0xf01bc8), 0);
    let mut errors = u32::from(channel != 0);
    if read_volatile(0x954dcc as *const u32) == 0 {
        errors += u32::from(xm1_sac_optional() != 0);
    }
    xm1_sac_capture();
    xm1_sac_f4c368();
    xm1_sac_10546c0();
    errors += u32::from(xm1_sac_1039500() != 0);
    errors += u32::from(xm1_sac_eecbf4() != 0);
    xm1_sac_1009974();
    errors += u32::from(xm1_sac_1050efc() != 0);
    xm1_sac_1051f30();
    xm1_sac_1053854();
    let status = xm1_sac_reset();
    match callback {
        None => status,
        Some(complete) => complete(if errors == 0 { 0 } else { -98 }),
    }
}
