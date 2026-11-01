#![no_std]

use core::arch::asm;
use core::panic::PanicInfo;
use core::ptr::addr_of;

macro_rules! word_type {
    ($($name:ident),+ $(,)?) => {$(
        #[repr(transparent)]
        #[derive(Clone, Copy)]
        struct $name(u32);
    )+};
}

word_type!(DescriptorId, TaskClass, RootIndex, CoreId, EncodedTaskId, ByteCount,
    IrqMask, CriticalToken, TaskPriority, TaskArgument, FillByte);

#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct GuestAddress(u32);

#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct StorageIndex(u32);

#[repr(transparent)]
#[derive(Clone, Copy)]
struct TaskId(u16);

#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct SubmissionStatus(i32);

#[repr(u32)]
#[derive(Clone, Copy)]
enum ErrorEvent {
    Submission = 3,
    Reservation = 37,
}

#[repr(u32)]
enum LockOwner {
    FirstCore = 1,
    OtherCore = 2,
}

#[repr(u32)]
enum AuxiliaryFlags {
    Sentinel = 0,
    Ordinary = 4,
}

#[repr(u32)]
enum CreationFlags {
    StartupReady = 2,
}

#[repr(C)]
struct AuxiliaryParameters {
    flags: AuxiliaryFlags,
    reserved: u32,
}

#[repr(C)]
struct CreationParameters {
    flags: CreationFlags,
    argument: TaskArgument,
    entry: GuestAddress,
    priority: TaskPriority,
    bytes: ByteCount,
    base: GuestAddress,
}

type AuxiliaryService = unsafe extern "C" fn(EncodedTaskId, *const AuxiliaryParameters) -> SubmissionStatus;
type CreationService = unsafe extern "C" fn(EncodedTaskId, *const CreationParameters) -> SubmissionStatus;
type ActivationService = unsafe extern "C" fn(EncodedTaskId) -> SubmissionStatus;
type IdentityService = unsafe extern "C" fn() -> CoreId;
type IrqSaveService = unsafe extern "C" fn() -> IrqMask;
type IrqRestoreService = unsafe extern "C" fn(IrqMask);

extern "C" {
    static xm1_task_class_table: u8;
    static xm1_task_interface_roots: u8;
    static xm1_task_wrapper_base: u8;
    static xm1_task_state_base: u8;
    static xm1_task_error_word: u8;
    static xm1_task_release_lock_literal: GuestAddress;

    fn xm1_task_critical_enter() -> CriticalToken;
    fn xm1_task_critical_exit(token: CriticalToken);
    fn xm1_task_reservation(index: StorageIndex, bytes: ByteCount, output: GuestAddress) -> SubmissionStatus;
    fn xm1_task_fill(base: GuestAddress, byte: FillByte, bytes: ByteCount);
    fn xm1_task_cache(base: GuestAddress, bytes: ByteCount);
    fn xm1_task_log(event: ErrorEvent, id: DescriptorId, status: SubmissionStatus);
    fn xm1_task_error_callback_dispatch(status: SubmissionStatus);
    fn xm1_task_try_lock(lock: GuestAddress, owner: LockOwner) -> SubmissionStatus;
    fn xm1_task_unlock(lock: GuestAddress) -> SubmissionStatus;
}

impl GuestAddress {
    #[inline(always)]
    fn offset(self, bytes: u32) -> Self {
        Self(self.0.wrapping_add(bytes))
    }

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

    #[inline(always)]
    unsafe fn low_id(self) -> TaskId {
        let value: u32;
        unsafe {
            asm!("ldrh {value}, [{address}]", value = out(reg) value,
                address = in(reg) self.0, options(nostack, preserves_flags));
        }
        TaskId(value as u16)
    }

    #[inline(always)]
    unsafe fn store<const OFFSET: u32>(self, value: u32) {
        unsafe {
            asm!("str {value}, [{address}, #{offset}]", value = in(reg) value,
                address = in(reg) self.0, offset = const OFFSET,
                options(nostack, preserves_flags));
        }
    }
}

impl DescriptorId {
    #[inline(always)]
    fn index(self) -> StorageIndex {
        StorageIndex(self.0.wrapping_sub(1))
    }
}

impl StorageIndex {
    #[inline(always)]
    fn wrapper(self) -> GuestAddress {
        GuestAddress(addr_of!(xm1_task_wrapper_base) as u32).offset(self.0.wrapping_mul(8))
    }
}

#[inline(always)]
unsafe fn class(id: TaskId) -> TaskClass {
    let slot = GuestAddress(addr_of!(xm1_task_class_table) as u32)
        .offset((id.0 as u32).wrapping_sub(1).wrapping_mul(4));
    TaskClass(unsafe { slot.load::<0>() })
}

#[inline(always)]
unsafe fn root(index: RootIndex) -> GuestAddress {
    let slot = GuestAddress(addr_of!(xm1_task_interface_roots) as u32)
        .offset(index.0.wrapping_mul(4));
    GuestAddress(unsafe { slot.load::<0>() })
}

#[inline(always)]
fn encoded(id: TaskId, class: TaskClass) -> EncodedTaskId {
    EncodedTaskId((id.0 as u32) | class.0.wrapping_shl(16))
}

#[inline(always)]
unsafe fn current_core() -> CoreId {
    let first = unsafe { root(RootIndex(0)) };
    let service: IdentityService = unsafe { core::mem::transmute(first.load::<0x1618>()) };
    unsafe { service() }
}

#[inline(never)]
unsafe fn report_error(descriptor: GuestAddress, event: ErrorEvent, status: SubmissionStatus) -> SubmissionStatus {
    unsafe {
        let id = DescriptorId(descriptor.load::<0>());
        xm1_task_log(event, id, status);
        GuestAddress(addr_of!(xm1_task_error_word) as u32).store::<0>(status.0 as u32);
        xm1_task_error_callback_dispatch(status);
    }
    status
}

/// # Safety
/// Reached guest accesses must be mapped with A32 word and halfword semantics. Guest accesses must not alias executing code or the private stack. Retained services must preserve AAPCS callee-saved registers and SP. The caller owns descriptor, root, class, storage and callback lifetimes and synchronization.
#[no_mangle]
pub unsafe extern "C" fn xm1_task_submit(descriptor: GuestAddress) -> SubmissionStatus {
    unsafe {
        let initial_id = descriptor.low_id();
        let selected_class = class(initial_id);
        let token = xm1_task_critical_enter();
        let wrapper = DescriptorId(descriptor.load::<0>()).index().wrapper();
        if wrapper.load::<4>() == 0 && wrapper.load::<0>() == 0 {
            let requested = ByteCount(descriptor.load::<12>());
            wrapper.store::<4>(requested.0);
            let index = DescriptorId(descriptor.load::<0>()).index();
            let output = index.wrapper();
            let bytes = ByteCount(output.load::<4>());
            let status = xm1_task_reservation(index, bytes, output);
            if status.0 != 0 {
                xm1_task_critical_exit(token);
                return report_error(descriptor, ErrorEvent::Reservation, status);
            }
        }

        let auxiliary = AuxiliaryParameters {
            flags: if descriptor.load::<24>() == 0xa5f0_a5f0 {
                AuxiliaryFlags::Sentinel
            } else {
                AuxiliaryFlags::Ordinary
            },
            reserved: 0,
        };
        let selected_root = RootIndex(selected_class.0.wrapping_sub(1));
        let id = descriptor.low_id();
        let interface = root(selected_root);
        let task_class = class(id);
        let service: AuxiliaryService = core::mem::transmute(interface.load::<0x80>());
        let status = service(encoded(id, task_class), &auxiliary);
        if status.0 != 0 && status.0 != -41 {
            xm1_task_critical_exit(token);
            return report_error(descriptor, ErrorEvent::Submission, status);
        }

        let argument = TaskArgument(descriptor.load::<20>());
        let entry = GuestAddress(descriptor.load::<16>());
        let priority = TaskPriority(descriptor.load::<8>());
        let full_id = DescriptorId(descriptor.load::<0>());
        let wrapper = full_id.index().wrapper();
        let bytes = ByteCount(wrapper.load::<4>());
        let base = GuestAddress(wrapper.load::<0>());
        let creation = CreationParameters {
            flags: CreationFlags::StartupReady,
            argument,
            entry,
            priority,
            bytes,
            base,
        };
        xm1_task_fill(base, FillByte(full_id.0 & 0xff), bytes);
        xm1_task_cache(GuestAddress(base.0 & !0x4000_0000), bytes);
        let id = descriptor.low_id();
        let interface = root(selected_root);
        let task_class = class(id);
        let service: CreationService = core::mem::transmute(interface.load::<0>());
        let status = service(encoded(id, task_class), &creation);
        xm1_task_critical_exit(token);
        if status.0 != 0 {
            return report_error(descriptor, ErrorEvent::Submission, status);
        }

        let core = current_core();
        let id = descriptor.low_id();
        let interface = root(RootIndex(core.0));
        let task_class = class(id);
        let service: ActivationService = core::mem::transmute(interface.load::<12>());
        let status = service(encoded(id, task_class));
        if status.0 != 0 {
            return report_error(descriptor, ErrorEvent::Submission, status);
        }
        status
    }
}

/// # Safety
/// The index and reached guest accesses must satisfy the retained release caller's mapping and lifetime premises. Services must preserve AAPCS callee-saved registers and SP. Guest writes must not alias executing code or the private stack.
#[no_mangle]
pub unsafe extern "C" fn xm1_task_release(index: StorageIndex) -> SubmissionStatus {
    unsafe {
        let core = current_core();
        let interface = root(RootIndex(core.0));
        let service: IrqSaveService = core::mem::transmute(interface.load::<0x164>());
        let mask = service();
        let lock;
        asm!("ldr {value}, {literal}", value = out(reg) lock,
            literal = sym xm1_task_release_lock_literal, options(nostack, preserves_flags));
        let lock = GuestAddress(lock);
        let owner = if core.0 == 0 { LockOwner::FirstCore } else { LockOwner::OtherCore };
        let _ = xm1_task_try_lock(lock, owner);
        GuestAddress(addr_of!(xm1_task_state_base) as u32)
            .offset(index.0.wrapping_mul(12)).store::<8>(0);
        let _ = xm1_task_unlock(lock);
        let interface = root(RootIndex(core.0));
        let service: IrqRestoreService = core::mem::transmute(interface.load::<0x168>());
        service(mask);
        SubmissionStatus(0)
    }
}

#[panic_handler]
fn panic(_: &PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
