#![no_std]

use core::arch::asm;
use core::panic::PanicInfo;

#[repr(transparent)]
pub struct StartupPhase(u32);
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct DescriptorAddress(u32);
#[repr(transparent)]
pub struct DispatchReturn(u32);
#[repr(transparent)]
struct SetupStatus(u32);
#[repr(transparent)]
struct TaskStatus(u32);
#[repr(transparent)]
struct RetainedMode(u32);
#[repr(transparent)]
struct DataAddress(u32);
#[repr(transparent)]
struct WordCount(u32);

enum Task { Root, Idle0, Idle1, Standby1 }

struct TaskId(u32);
struct TaskNameAddress(u32);
struct Priority(u32);
struct EntryAddress(u32);
struct TaskFields {
    id: TaskId,
    name: TaskNameAddress,
    priority: Priority,
    entry: EntryAddress,
}

extern "C" {
    fn xm1_startup_mode() -> RetainedMode;
    fn xm1_startup_clear(base: DataAddress, value: u32, words: WordCount);
    fn xm1_startup_install() -> SetupStatus;
    fn xm1_startup_submit(descriptor: DescriptorAddress) -> TaskStatus;
}

impl DescriptorAddress {
    #[inline(always)]
    unsafe fn store(self, offset: u32, value: u32) {
        unsafe {
            asm!("str {value}, [{address}]", address = in(reg) self.0.wrapping_add(offset),
                value = in(reg) value, options(nostack, preserves_flags));
        }
    }

    #[inline(never)]
    unsafe fn submit(self, task: Task) {
        let fields = match task {
            Task::Root => TaskFields { id: TaskId(104), name: TaskNameAddress(0x781960), priority: Priority(1), entry: EntryAddress(0x69d160) },
            Task::Idle0 => TaskFields { id: TaskId(102), name: TaskNameAddress(0x781968), priority: Priority(15), entry: EntryAddress(0x69cbf0) },
            Task::Idle1 => TaskFields { id: TaskId(103), name: TaskNameAddress(0x781970), priority: Priority(15), entry: EntryAddress(0x69cbf0) },
            Task::Standby1 => TaskFields { id: TaskId(93), name: TaskNameAddress(0x781978), priority: Priority(13), entry: EntryAddress(0x69cd58) },
        };
        unsafe {
            self.store(0, fields.id.0);
            self.store(4, fields.name.0);
            self.store(8, fields.priority.0);
            self.store(12, 0x400);
            self.store(16, fields.entry.0);
            self.store(20, 0);
            let _ = xm1_startup_submit(self);
        }
    }
}

/// # Safety
/// The A32 adapter supplies D = incoming SP - 44. The descriptor has guest word-access semantics and no Rust references. Its unknown word at D+24 must remain outside Rust scratch. Retained calls preserve r4-r11 and SP; their writes and aliases must not overlap executing code, saved registers or Rust scratch. Mapping, stack backing, interruption and descriptor lifetime require separate admission.
#[no_mangle]
pub unsafe extern "C" fn xm1_startup_dispatch(phase: StartupPhase, descriptor: DescriptorAddress) -> DispatchReturn {
    match phase.0 {
        0 => unsafe {
            if xm1_startup_mode().0 == 1 {
                xm1_startup_clear(DataAddress(0x89ec00), 0, WordCount(0x352e00 / 4));
            }
            if xm1_startup_install().0 != 0 {
                loop { asm!("b .", options(nomem, nostack)); }
            }
        },
        1 => unsafe { descriptor.submit(Task::Root) },
        2 => return DispatchReturn(0x69cc6c),
        3 => unsafe { descriptor.submit(Task::Idle0) },
        4 => unsafe {
            descriptor.submit(Task::Idle1);
            descriptor.submit(Task::Standby1);
        },
        _ => (),
    }
    DispatchReturn(0)
}

#[panic_handler]
fn panic(_: &PanicInfo<'_>) -> ! {
    loop { core::hint::spin_loop(); }
}
