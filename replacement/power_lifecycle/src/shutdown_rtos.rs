use crate::shutdown::{KernelStatus, QueueId, ServiceMessage};

/// A device-owned queue claims its messages before the generic kernel block allocation.
pub trait LensQueueIngress: Sync {
    fn try_post(&self, queue: QueueId, message: &ServiceMessage<'_>) -> Option<Result<(), KernelStatus>>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IngressInstalled;

#[cfg(target_arch = "arm")]
pub use native::install_lens_queue_ingress;

#[cfg(target_arch = "arm")]
mod native {
    use super::{IngressInstalled, LensQueueIngress};
    use crate::callback_registry::{registry as callbacks, CallbackWord};
    use crate::shutdown::{KernelStatus, QueueId, ServiceMessage, ShutdownRtos, TaskId};
    use core::cell::UnsafeCell;
    use core::mem::MaybeUninit;
    use core::ptr::{copy_nonoverlapping, read_volatile, write_volatile};
    use core::sync::atomic::{AtomicBool, Ordering};

    struct IngressSlot { claimed: AtomicBool, published: AtomicBool, ingress: UnsafeCell<MaybeUninit<&'static dyn LensQueueIngress>> }
    // `ingress` is written by the single claimant and read only after `published`.
    unsafe impl Sync for IngressSlot {}
    static INGRESS: IngressSlot = IngressSlot { claimed: AtomicBool::new(false), published: AtomicBool::new(false), ingress: UnsafeCell::new(MaybeUninit::uninit()) };

    pub fn install_lens_queue_ingress(ingress: &'static dyn LensQueueIngress) -> Result<(), IngressInstalled> {
        if INGRESS.claimed.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() { return Err(IngressInstalled); }
        unsafe { (*INGRESS.ingress.get()).write(ingress) };
        INGRESS.published.store(true, Ordering::Release);
        Ok(())
    }
    fn lens_ingress() -> Option<&'static dyn LensQueueIngress> {
        if !INGRESS.published.load(Ordering::Acquire) { return None; }
        Some(unsafe { (*INGRESS.ingress.get()).assume_init_read() })
    }

    extern "C" {
        fn xm1_power_current_task() -> u32;
        fn xm1_power_service_error(operation: u32, id: i32, status: i32);
        fn xm1_power_service_event(operation: u32, id: i32, status: i32);
        fn xm1_power_record_service_error(status: i32);
        fn xm1_power_service_post_trace(header: u32);
    }

    unsafe fn word(address: u32) -> u32 { unsafe { read_volatile(address as *const u32) } }
    unsafe fn interface() -> Result<u32, crate::callback_registry::CallbackError> {
        let primary = unsafe { word(0x96bd20) };
        let core: unsafe extern "C" fn() -> u32 = callbacks().resolve(CallbackWord(unsafe { word(primary + 0x1618) }))?;
        Ok(unsafe { word(0x96bd20 + core() * 4) })
    }
    unsafe fn encoded(index: u32) -> i32 {
        (index | unsafe { word(0x7cad9c + (index - 1) * 4) } << 16) as i32
    }
    fn status(value: i32) -> Result<(), KernelStatus> {
        if value == 0 { Ok(()) } else { Err(KernelStatus(value)) }
    }

    #[repr(C)]
    pub struct ServiceDescriptor {
        pub operation: u16,
        pub sender: u8,
        pub destination: u8,
        pub length: u32,
        pub payload: *const u8,
    }

    fn post_to(queue: u32, message: ServiceMessage<'_>) -> Result<(), KernelStatus> {
        let fail = |error: i32| {
            unsafe { xm1_power_service_error(0x1c, queue as i32, error) };
            KernelStatus(error)
        };
        let interface = match unsafe { interface() } { Ok(interface) => interface, Err(error) => return Err(fail(error.status())) };
        if queue == 0 { return Err(fail(-18)); }
        if let Some(ingress) = lens_ingress() {
            if let Ok(id) = u8::try_from(queue) {
                if let Some(result) = ingress.try_post(QueueId(id), &message) { return result; }
            }
        }
        let capacity = unsafe { word(0x40a0f3d0 + queue * 4 + 0x67c) }.wrapping_sub(8);
        if message.payload.len() > capacity as usize { return Err(fail(-60)); }
        let header = ServiceDescriptor {
            operation: message.operation.0,
            sender: message.sender.0,
            destination: message.destination.0,
            length: message.payload.len() as u32,
            payload: message.payload.as_ptr(),
        };
        unsafe { xm1_power_service_post_trace(&header as *const ServiceDescriptor as u32) };
        let id = unsafe { encoded(queue) };
        let mut block = MaybeUninit::<u32>::uninit();
        let allocate: unsafe extern "C" fn(i32, u32) -> i32 = match callbacks().resolve(CallbackWord(unsafe { word(interface + 0x170) })) { Ok(entry) => entry, Err(error) => return Err(fail(error.status())) };
        let result = unsafe { allocate(id, block.as_mut_ptr() as u32) };
        if result != 0 { return Err(fail(if result == -50 { -62 } else { result })); }
        let block = unsafe { block.assume_init() };
        unsafe {
            write_volatile((block + 4) as *mut u16, message.operation.0);
            write_volatile((block + 6) as *mut u8, message.sender.0);
            write_volatile((block + 7) as *mut u8, message.destination.0);
            write_volatile((block + 8) as *mut u32, message.payload.len() as u32);
            copy_nonoverlapping(message.payload.as_ptr(), (block + 12) as *mut u8, message.payload.len());
        }
        let context: unsafe extern "C" fn() -> u32 = match callbacks().resolve(CallbackWord(unsafe { word(interface + 0x148) })) { Ok(entry) => entry, Err(error) => return Err(fail(error.status())) };
        let offset = if unsafe { context() } == 1 { 0xb0 } else { 0xac };
        let send: unsafe extern "C" fn(i32, u32) -> i32 = match callbacks().resolve(CallbackWord(unsafe { word(interface + offset) })) { Ok(entry) => entry, Err(error) => return Err(fail(error.status())) };
        let result = unsafe { send(id, block) };
        if result != 0 {
            unsafe { xm1_power_service_event(0x1c, queue as i32, result) };
            let release: unsafe extern "C" fn(i32, u32) = match callbacks().resolve(CallbackWord(unsafe { word(interface + 0xf8) })) { Ok(entry) => entry, Err(error) => return Err(fail(error.status())) };
            unsafe { release(id, block) };
            unsafe { xm1_power_record_service_error(result) };
            return Err(KernelStatus(result));
        }
        Ok(())
    }

    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_service_post(queue: u32, header: *const ServiceDescriptor) -> i32 {
        let header = unsafe { &*header };
        let payload = if header.length == 0 { &[] } else {
            unsafe { core::slice::from_raw_parts(header.payload, header.length as usize) }
        };
        match post_to(queue, ServiceMessage {
            operation: crate::shutdown::RequestId(header.operation),
            sender: TaskId(header.sender),
            destination: crate::shutdown::QueueId(header.destination),
            payload,
        }) {
            Ok(()) => 0,
            Err(error) => error.0,
        }
    }

    pub struct NativeShutdownRtos;
    impl ShutdownRtos for NativeShutdownRtos {
        fn current_task(&mut self) -> TaskId { TaskId(unsafe { xm1_power_current_task() } as u8) }
        fn post(&mut self, message: ServiceMessage<'_>) -> Result<(), KernelStatus> {
            post_to(u32::from(message.destination.0), message)
        }
        fn set_event_flag(&mut self, flag: u32, bits: u32) -> Result<(), KernelStatus> {
            let interface = match unsafe { interface() } { Ok(interface) => interface, Err(error) => return status(error.status()) };
            let context: unsafe extern "C" fn() -> u32 = match callbacks().resolve(CallbackWord(unsafe { word(interface + 0x148) })) { Ok(entry) => entry, Err(error) => return status(error.status()) };
            let offset = if unsafe { context() } == 1 { 0x8c } else { 0x88 };
            let set: unsafe extern "C" fn(i32, u32) -> i32 = match callbacks().resolve(CallbackWord(unsafe { word(interface + offset) })) { Ok(entry) => entry, Err(error) => return status(error.status()) };
            let id = unsafe { encoded(flag & 0xffff) };
            let result = unsafe { set(id, bits) };
            if result != 0 { unsafe { xm1_power_service_error(0x13, flag as i32, result) }; }
            status(result)
        }
    }

    pub unsafe fn wait_current_task(bits: u32, observed: &mut u32) -> Result<(), KernelStatus> {
        status(unsafe { xm1_power_wait_current_all(bits, observed, -1) })
    }

    enum CurrentWait { Any, All }
    impl CurrentWait {
        fn mode(&self) -> u32 { match self { Self::Any => 0, Self::All => 1 } }
        fn diagnostic(&self) -> u32 { match self { Self::Any => 0x12, Self::All => 0x11 } }
    }
    unsafe fn wait_current(bits: u32, observed: *mut u32, timeout: i32, mode: CurrentWait) -> i32 {
        let interface = match unsafe { interface() } { Ok(interface) => interface, Err(error) => return error.status() };
        let current: unsafe extern "C" fn() -> u32 = match callbacks().resolve(CallbackWord(unsafe { word(interface + 0x16c) })) { Ok(entry) => entry, Err(error) => return error.status() };
        let task = unsafe { current() };
        let id = unsafe { encoded(task & 0xffff) };
        let wait: unsafe extern "C" fn(i32, u32, u32, *mut u32, i32) -> i32 = match callbacks().resolve(CallbackWord(unsafe { word(interface + 0x9c) })) { Ok(entry) => entry, Err(error) => return error.status() };
        let result = unsafe { wait(id, bits, mode.mode(), observed, timeout) };
        if result != 0 && result != -50 {
            unsafe { xm1_power_service_error(mode.diagnostic(), current() as i32, result) };
        }
        result
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_wait_current_all(bits: u32, observed: *mut u32, timeout: i32) -> i32 {
        unsafe { wait_current(bits, observed, timeout, CurrentWait::All) }
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_wait_current_any(bits: u32, observed: *mut u32, timeout: i32) -> i32 {
        unsafe { wait_current(bits, observed, timeout, CurrentWait::Any) }
    }

    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_read_interface_word_174(output: *mut u32) {
        if let Ok(interface) = unsafe { interface() } { unsafe { write_volatile(output, word(word(interface + 0x174))) }; }
    }

    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_clear_current_flags() -> i32 {
        let task = unsafe { xm1_power_current_task() };
        let id = unsafe { encoded(task & 0xffff) };
        let interface = match unsafe { interface() } { Ok(interface) => interface, Err(error) => return error.status() };
        let clear: unsafe extern "C" fn(i32, u32) -> i32 = match callbacks().resolve(CallbackWord(unsafe { word(interface + 0x90) })) { Ok(entry) => entry, Err(error) => return error.status() };
        let result = unsafe { clear(id, 0) };
        if result != 0 {
            unsafe { xm1_power_service_error(0x14, xm1_power_current_task() as i32, result) };
        }
        result
    }

    #[no_mangle]
    pub extern "C" fn xm1_power_set_task_flags(task: u32, bits: u32) -> i32 {
        match NativeShutdownRtos.set_event_flag(task, bits) {
            Ok(()) => 0,
            Err(error) => error.0,
        }
    }
}
#[cfg(target_arch = "arm")]
pub use native::{wait_current_task, NativeShutdownRtos, ServiceDescriptor, xm1_power_service_post, xm1_power_wait_current_all, xm1_power_wait_current_any, xm1_power_set_task_flags, xm1_power_clear_current_flags, xm1_power_read_interface_word_174};
