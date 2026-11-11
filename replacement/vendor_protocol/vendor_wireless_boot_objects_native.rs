//! Selected-domain RTOS primitives behind the application object wrappers.

use super::{CoreClass, Cursor, EventRecord, KernelId, ObjectError, ObjectKernel, ObjectStatus, SemaphoreRecord, TaskHandle, TaskKind, TaskRecord};
use xm1_power_services::callback_registry::{CallbackFn, KernelSlot};

extern "C" {
    fn xm1_power_current_task() -> u32;
    fn xm1_rtos_own_first_slot_1618() -> u32;
    fn xm1_rtos_own_first_slot_164() -> u32;
    fn xm1_rtos_own_first_slot_168(saved: u32) -> u32;
}

pub struct NativeObjectKernel { _exclusive: () }
impl NativeObjectKernel {
    /// # Safety
    /// The RTOS dispatch roots, class table and dynamic object cursors are initialized.
    pub unsafe fn acquire() -> Self { Self { _exclusive: () } }
    fn load(address: u32) -> u32 { unsafe { core::ptr::read_volatile(address as *const u32) } }
    fn store(address: u32, value: u32) { unsafe { core::ptr::write_volatile(address as *mut u32, value); } }
    fn current_index() -> u32 { unsafe { xm1_rtos_own_first_slot_1618() } }
    fn root() -> u32 { Self::load(0x0096_bd20 + Self::current_index() * 4) }
    fn cursor_address(kind: Cursor) -> u32 {
        match kind { Cursor::Semaphore => 0x40a0_f86c, Cursor::Event => 0x40a0_fc38, Cursor::Task => 0x40a0_fc34 }
    }
    /// Kernel interface slots hold an entry with the signature `F`; they are not registry callbacks.
    ///
    /// # Safety
    /// `base + offset` must hold a kernel interface word with the signature `F`.
    unsafe fn entry<F: CallbackFn>(base: u32, offset: u32) -> F {
        unsafe { KernelSlot::read(base, offset).entry() }
    }
    fn unary(offset: u32, id: KernelId) -> ObjectStatus {
        let call = unsafe { Self::entry::<unsafe extern "C" fn(u32) -> i32>(Self::root(), offset) };
        ObjectStatus(unsafe { call(id.wire()) })
    }
}
impl ObjectKernel for NativeObjectKernel {
    fn core(&self) -> Result<CoreClass, ObjectError> {
        match Self::current_index() { 0 => Ok(CoreClass::Primary), 1 => Ok(CoreClass::Secondary), _ => Err(ObjectError::InvalidCore) }
    }
    fn current_task(&self) -> Result<TaskHandle, ObjectError> {
        let id = unsafe { xm1_power_current_task() } as i32;
        if id > 0 { Ok(TaskHandle(id as u16)) } else { Err(ObjectError::Kernel(ObjectStatus(id))) }
    }
    fn cursor(&self, kind: Cursor) -> i32 { Self::load(Self::cursor_address(kind)) as i32 }
    fn set_cursor(&mut self, kind: Cursor, value: i32) { Self::store(Self::cursor_address(kind), value as u32); }
    fn semaphore_class(&self, slot: u16) -> u32 { Self::load(0x0093_e6f4 + (u32::from(slot) - 1) * 4) }
    fn set_semaphore_class(&mut self, slot: u16, class: u32) { Self::store(0x0093_e6f4 + (u32::from(slot) - 1) * 4, class); }
    fn mask_interrupts(&mut self) -> u32 { unsafe { xm1_rtos_own_first_slot_164() } }
    fn restore_interrupts(&mut self, saved: u32) { unsafe { let _ = xm1_rtos_own_first_slot_168(saved); } }
    fn create_semaphore(&mut self, id: KernelId, record: SemaphoreRecord) -> ObjectStatus {
        // The record pointer travels as one word.
        let call = unsafe { Self::entry::<unsafe extern "C" fn(u32, *const u32) -> i32>(Self::root(), 0x64) };
        ObjectStatus(unsafe { call(id.wire(), &record as *const SemaphoreRecord as *const u32) })
    }
    fn delete_semaphore(&mut self, id: KernelId) -> ObjectStatus { Self::unary(0x68, id) }
    fn create_event(&mut self, id: KernelId, record: EventRecord) -> ObjectStatus {
        let call = unsafe { Self::entry::<unsafe extern "C" fn(u32, *const u32) -> i32>(Self::root(), 0x80) };
        ObjectStatus(unsafe { call(id.wire(), &record as *const EventRecord as *const u32) })
    }
    fn delete_event(&mut self, id: KernelId) -> ObjectStatus { Self::unary(0x84, id) }
    fn set_event(&mut self, id: KernelId, bits: u32) -> ObjectStatus {
        let context = unsafe { Self::entry::<unsafe extern "C" fn() -> u32>(Self::root(), 0x148) };
        let offset = if unsafe { context() } == 1 { 0x8c } else { 0x88 };
        let call = unsafe { Self::entry::<unsafe extern "C" fn(u32, u32) -> i32>(Self::root(), offset) };
        ObjectStatus(unsafe { call(id.wire(), bits) })
    }
    fn create_task(&mut self, id: KernelId, record: TaskRecord) -> ObjectStatus {
        extern "C" { fn xm1_wireless_driver_task(context: u32); fn xm1_wireless_supplicant_task(context: u32); }
        let entry = match record.kind { TaskKind::Driver => xm1_wireless_driver_task as usize as u32, TaskKind::Supplicant => xm1_wireless_supplicant_task as usize as u32 };
        let record = [2, record.context.address(), entry, record.kind.priority(), record.kind.stack_length() as u32, record.stack.address()];
        let call = unsafe { Self::entry::<unsafe extern "C" fn(u32, *const u32) -> i32>(Self::load(0x0096_bd20), 0) };
        ObjectStatus(unsafe { call(id.wire(), record.as_ptr()) })
    }
    fn activate_task(&mut self, id: KernelId) -> ObjectStatus { Self::unary(0x0c, id) }
    fn suspend_task(&mut self, id: KernelId) -> ObjectStatus { Self::unary(0x10, id) }
    fn terminate_task(&mut self, id: KernelId) -> ObjectStatus { Self::unary(0x18, id) }
    fn delete_task(&mut self, id: KernelId) -> ObjectStatus {
        let call = unsafe { Self::entry::<unsafe extern "C" fn(u32, u32) -> i32>(Self::root(), 4) };
        ObjectStatus(unsafe { call(id.wire(), 1) })
    }
}
