use core::arch::asm;
use core::ptr::{read_volatile, write_volatile};
use crate::layout::{DomainId, DomainWords, GuestAddress, KernelContext, Task, TaskFlags};

#[derive(Clone, Copy)]
struct TaskHandle { domain: DomainId, number: i16 }

impl TaskHandle {
    #[inline(always)]
    fn parse(raw: u32) -> Option<Self> {
        Some(Self { domain: DomainId::from_word(raw >> 16)?, number: raw as i16 })
    }
}

#[derive(Clone, Copy)]
enum Request { Activate, CancelActivation, Wake, CancelWake }

#[derive(Clone, Copy)]
enum Invocation { Task, Handler }

const WORDS: DomainWords = crate::DOMAIN.words();

extern "C" {
    fn s3b_prepare(context: KernelContext, task: Task) -> u32;
    fn s3b_release_wait(context: KernelContext, task: Task) -> u32;
    fn s3b_notify(id: u32) -> u32;
    fn s3b_switch(reason: i32);
}

#[inline(always)]
unsafe fn word(at: u32) -> u32 { read_volatile(at as *const u32) }
#[inline(always)]
unsafe fn byte(at: u32) -> u8 { read_volatile(at as *const u8) }
#[inline(always)]
unsafe fn irq<const MASK: bool>() {
    let status: u32;
    asm!("mrs {status}, cpsr", status = out(reg) status, options(nostack, preserves_flags));
    let status = if MASK { status | 0x80 } else { status & !0x80 };
    asm!("msr CPSR_fc, {status}", status = in(reg) status, options(nostack));
}

#[cfg_attr(feature = "component", inline(never))]
#[cfg_attr(not(feature = "component"), inline(always))]
unsafe fn service(raw: u32, request: Request, invocation: Invocation) -> i32 {
    let handler = matches!(invocation, Invocation::Handler);
    let status: u32;
    asm!("mrs {status}, cpsr", status = out(reg) status, options(nostack, preserves_flags));
    if (word(WORDS.handler_context.0) != 0) != handler || status & 0x80 != 0 { return -25; }
    let Some(handle) = TaskHandle::parse(raw) else { return -18; };
    let context = match handle.domain {
        DomainId::Own => word(WORDS.current_context.0),
        DomainId::A => WORDS.contexts.0,
        DomainId::B => WORDS.contexts.0 + KernelContext::SIZE,
    };
    let number = handle.number as i32;
    if number < 0 || (handler && number == 0) || number > word(context + 0x48) as i32 { return -18; }
    let task = if number == 0 { word(WORDS.running.0) } else { word(context + 0x44) + (number as u32 - 1) * Task::SIZE };
    if word(word(task + 8) + 0x24) == 0 { return -42; }
    let lock = context + 8;
    loop {
        irq::<true>();
        let acquire: unsafe extern "C" fn(u32) -> i32 = core::mem::transmute(word(context + 0x0c));
        if acquire(lock) == 0 { break; }
        if !handler { irq::<false>(); }
    }
    let mut dispatch = false;
    let state = byte(task + 0x0c);
    let flags = byte(task + 0x0e);
    let bit = match request { Request::Activate | Request::CancelActivation => TaskFlags::QUEUED_ACTIVATION.0, _ => TaskFlags::QUEUED_WAKEUP.0 };
    let mut result = 0;
    match request {
        Request::CancelActivation | Request::CancelWake => {
            if (matches!(request, Request::CancelWake) && state == 0) || (task == word(WORDS.running.0) && word(task + 0x30) != 0) {
                result = -41;
            } else {
                result = i32::from(flags & bit != 0);
                write_volatile((task + 0x0e) as *mut u8, flags & !bit);
            }
        }
        Request::Activate | Request::Wake => {
            let wake = matches!(request, Request::Wake);
            if wake && (state == 0 || (!handler && task == word(WORDS.running.0) && word(task + 0x30) != 0)) {
                result = -41;
            } else if (!wake && state == 0) || (wake && state & 8 != 0) {
                let context = KernelContext(GuestAddress(context));
                let task = Task(GuestAddress(task));
                let changed = if wake { s3b_release_wait(context, task) } else { s3b_prepare(context, task) };
                if changed != 0 && s3b_notify(word(context.0.0)) != 0 {
                    if handler { write_volatile(WORDS.reschedule.0 as *mut u32, 1); } else { dispatch = true; }
                }
            } else if flags & bit != 0 { result = -43; }
            else { write_volatile((task + 0x0e) as *mut u8, flags | bit); }
        }
    }
    let release: unsafe extern "C" fn(u32) = core::mem::transmute(word(context + 0x10));
    release(lock);
    if dispatch { s3b_switch(if matches!(request, Request::Activate) { -7 } else { -19 }); }
    irq::<false>();
    result
}

macro_rules! entry {
    ($name:ident, $section:literal, $handler:ident, $request:ident) => {
        #[no_mangle]
        #[link_section = $section]
        pub unsafe extern "C" fn $name(handle: u32) -> i32 { service(handle, Request::$request, Invocation::$handler) }
    };
}
entry!(xm1_s3b_interrupt_activate, ".text.s3b_interrupt_activate", Handler, Activate);
entry!(xm1_s3b_activate, ".text.s3b_activate", Task, Activate);
entry!(xm1_s3b_cancel_activation, ".text.s3b_cancel_activation", Task, CancelActivation);
entry!(xm1_s3b_wake, ".text.s3b_wake", Task, Wake);
entry!(xm1_s3b_interrupt_wake, ".text.s3b_interrupt_wake", Handler, Wake);
entry!(xm1_s3b_cancel_wake, ".text.s3b_cancel_wake", Task, CancelWake);
