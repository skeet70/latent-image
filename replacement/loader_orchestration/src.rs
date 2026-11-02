#![no_std]

use core::arch::asm;
use core::panic::PanicInfo;
use core::ptr::{addr_of_mut, read_volatile, write_volatile};

#[repr(transparent)]
pub struct DispatchEntry(u32);
#[repr(transparent)]
struct ReturnedCode(u32);
#[repr(transparent)]
struct Identity(u32);
#[repr(transparent)]
struct GuestAddress(u32);
#[repr(transparent)]
struct ByteCount(u32);
#[repr(transparent)]
struct ScheduleEntryRegister(i32);
#[repr(transparent)]
struct ReadyWord(u32);

#[repr(u32)]
enum Phase { Setup = 0, Root = 1, Diagnostic = 2, Idle0 = 3, PeerTasks = 4 }
#[repr(u32)]
enum LogMode { Initialize = 0, Timestamp = 1 }
#[derive(Clone, Copy, PartialEq, Eq)]
enum Role { Primary, Peer }

extern "C" {
    fn xm1_loader_identity() -> Identity;
    fn xm1_loader_clear(destination: GuestAddress, bytes: ByteCount);
    fn xm1_loader_private_initialize();
    fn xm1_loader_log(mode: LogMode);
    fn xm1_loader_prepare1();
    fn xm1_loader_prepare2();
    fn xm1_loader_prepare3();
    fn xm1_loader_prepare4();
    fn xm1_loader_primary_continue();
    fn xm1_loader_peer_continue();
    fn xm1_loader_continue1();
    fn xm1_loader_continue2();
    fn xm1_loader_continue3();
    fn xm1_loader_continue4();
    fn xm1_loader_continue5();
    fn xm1_loader_continue6();
    fn xm1_loader_continue7();
    fn xm1_loader_publish_callback(code: ReturnedCode);
    fn xm1_loader_auto_ready();
    fn xm1_loader_activate_timers();
    fn xm1_loader_schedule(entry_register: ScheduleEntryRegister) -> !;
    static mut xm1_loader_ready: ReadyWord;
}

impl DispatchEntry {
    #[inline(always)]
    unsafe fn invoke(&self, phase: Phase) -> ReturnedCode {
        let dispatch: unsafe extern "C" fn(Phase) -> ReturnedCode = unsafe { core::mem::transmute(self.0) };
        unsafe { dispatch(phase) }
    }
}

#[inline(always)]
unsafe fn wait_for_primary() {
    while unsafe { read_volatile(0x400a00c4 as *const u32) } == 0 {
        for _ in 0..100 {
            unsafe { asm!("nop", options(nomem, nostack, preserves_flags)); }
        }
    }
}

/// # Safety
/// Caller supplies a source-bound A32 dispatcher, the original loader stack and admitted retained-service resources.
#[no_mangle]
pub unsafe extern "C" fn xm1_loader_orchestration(dispatch: DispatchEntry) -> ! {
    unsafe {
        let role = if xm1_loader_identity().0 == 0 { Role::Primary } else { Role::Peer };
        match role {
            Role::Primary => xm1_loader_clear(GuestAddress(0x400a0800), ByteCount(0x20100)),
            Role::Peer => wait_for_primary(),
        }
        xm1_loader_private_initialize();
        xm1_loader_log(LogMode::Initialize);
        xm1_loader_prepare1();
        xm1_loader_prepare2();
        xm1_loader_prepare3();
        xm1_loader_prepare4();
        match role {
            Role::Primary => {
                let _ = dispatch.invoke(Phase::Setup);
                write_volatile(0x400a00c4 as *mut u32, 1);
                asm!("dsb sy", options(nostack, preserves_flags));
                xm1_loader_primary_continue();
            },
            Role::Peer => xm1_loader_peer_continue(),
        }
        xm1_loader_continue1();
        xm1_loader_continue2();
        xm1_loader_continue3();
        xm1_loader_continue4();
        xm1_loader_continue5();
        xm1_loader_continue6();
        xm1_loader_continue7();
        match role {
            Role::Primary => {
                let _ = dispatch.invoke(Phase::Root);
                let _ = dispatch.invoke(Phase::Idle0);
            },
            Role::Peer => { let _ = dispatch.invoke(Phase::PeerTasks); },
        }
        xm1_loader_publish_callback(dispatch.invoke(Phase::Diagnostic));
        xm1_loader_auto_ready();
        xm1_loader_activate_timers();
        xm1_loader_log(LogMode::Timestamp);
        write_volatile(addr_of_mut!(xm1_loader_ready.0), 1);
        xm1_loader_schedule(ScheduleEntryRegister(-10))
    }
}

#[panic_handler]
fn panic(_: &PanicInfo<'_>) -> ! {
    loop { core::hint::spin_loop(); }
}
