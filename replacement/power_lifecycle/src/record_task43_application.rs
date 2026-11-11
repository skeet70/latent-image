//! Installed application callbacks and the default 0x72b210 queue sender.

use crate::callback_registry::{registry as callbacks, CallbackError, CallbackWord};
use crate::shutdown::{KernelStatus, QueueId, RequestId, ServiceMessage, ShutdownRtos, TaskId, UiRouting};

pub type ApplicationCallback = crate::callback_slots::Application;

#[derive(Clone, Copy)]
pub enum InstalledCallback { Default, Owned(ApplicationCallback) }

#[derive(Clone, Copy)]
pub enum NativeCallback { Buffered, Ready(InstalledCallback) }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BufferedCount(pub u32);

pub struct EarlyApplicationBuffer<'a> {
    pub slots: &'a mut [[u8; 76]; 64],
    pub count: &'a mut BufferedCount,
}

impl EarlyApplicationBuffer<'_> {
    pub fn dispatch(&mut self, record: &[u8; 76], marker: impl FnOnce(ApplicationMarker)) -> Result<(), KernelStatus> {
        if self.count.0 > 63 {
            marker(ApplicationMarker::EarlyBufferFull);
            return Err(KernelStatus(-63));
        }
        self.slots[self.count.0 as usize][..8].copy_from_slice(&record[..8]);
        self.count.0 += 1;
        Ok(())
    }
}

pub enum ApplicationIngress<'a> {
    Buffered(EarlyApplicationBuffer<'a>),
    Active(ApplicationDispatcher),
}

impl ApplicationIngress<'_> {
    pub fn dispatch(&mut self, rtos: &mut impl ShutdownRtos, record: &[u8; 76], marker: impl FnOnce(ApplicationMarker)) -> Result<(), KernelStatus> {
        match self {
            Self::Buffered(buffer) => buffer.dispatch(record, marker),
            Self::Active(dispatcher) => dispatcher.dispatch(rtos, record, marker),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SequenceNumber(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApplicationGate {
    pub issued: SequenceNumber,
    pub consumed: SequenceNumber,
}

pub struct ApplicationDispatcher {
    pub callback: InstalledCallback,
    pub routing: UiRouting,
    pub first_request_task: TaskId,
    pub gate: ApplicationGate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplicationMarker { GateOccupied, EarlyBufferFull }

impl ApplicationDispatcher {
    pub fn dispatch(&mut self, rtos: &mut impl ShutdownRtos, record: &[u8; 76], marker: impl FnOnce(ApplicationMarker)) -> Result<(), KernelStatus> {
        if let InstalledCallback::Owned(callback) = self.callback {
            let status = unsafe { callback(record.as_ptr()) };
            return if status == 0 { Ok(()) } else { Err(KernelStatus(status)) };
        }
        let sender = rtos.current_task();
        let event = u32::from_le_bytes(record[..4].try_into().unwrap());
        let destination = match self.routing {
            UiRouting::Primary if event & 0x900 != 0 => QueueId(7),
            UiRouting::Primary | UiRouting::Other => QueueId(1),
        };
        if event == 0x82e && destination == QueueId(1) {
            if self.gate.issued != self.gate.consumed {
                marker(ApplicationMarker::GateOccupied);
                return Ok(());
            }
            self.gate.issued.0 = self.gate.issued.0.wrapping_add(1);
        }
        let result = rtos.post(ServiceMessage { operation: RequestId(0), sender, destination, payload: record });
        if event == 0x91b && self.first_request_task.0 == 0 {
            self.first_request_task = rtos.current_task();
        }
        result
    }
}

pub fn register_native_callback(callback: Option<ApplicationCallback>) -> Result<(), CallbackError> {
    match callback {
        Some(callback) => crate::callback_slots::register_application(callbacks(), callback),
        None => Ok(()),
    }
}

pub fn resolve_native_callback(installed: u32) -> Result<NativeCallback, CallbackError> {
    if installed == 0x72b318 { return Ok(NativeCallback::Buffered); }
    if installed == 0x72b210 { return Ok(NativeCallback::Ready(InstalledCallback::Default)); }
    let callback = crate::callback_slots::resolve_application(callbacks(), CallbackWord(installed as usize))?;
    Ok(NativeCallback::Ready(InstalledCallback::Owned(callback)))
}

#[cfg(target_arch = "arm")]
#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_application_install_callback(callback: Option<ApplicationCallback>) {
    let address = match register_native_callback(callback) {
        Ok(()) => callback.map_or(0x72b210, |callback| callback as usize as u32),
        Err(error) => { let _ = crate::callback_registry::refused(error); 0x72b210 },
    };
    unsafe { core::ptr::write_volatile(0x829a0c as *mut u32, address) };
}

#[cfg(target_arch = "arm")]
unsafe extern "C" {
    fn xm1_power_record_application_gate_marker();
    fn xm1_power_interrupt_lock() -> u32;
    fn xm1_power_interrupt_unlock(saved: u32);
    fn xm1_power_append_stream_text(task: u32, text: u32);
    fn xm1_pipeline_image_current_task() -> u32;
    fn xm1_pipeline_image_send_message(channel: u32, message: *const crate::record_task43::StorageMessage) -> i32;
}

#[cfg(target_arch = "arm")]
fn word(address: usize) -> u32 { unsafe { core::ptr::read_volatile(address as *const u32) } }
#[cfg(target_arch = "arm")]
fn byte(address: usize) -> u8 { unsafe { core::ptr::read_volatile(address as *const u8) } }
#[cfg(target_arch = "arm")]
fn put_word(address: usize, value: u32) { unsafe { core::ptr::write_volatile(address as *mut u32, value) }; }

#[cfg(target_arch = "arm")]
#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_72b680(record: u32) -> i32 {
    let installed = match resolve_native_callback(word(0x829a0c)) { Ok(installed) => installed, Err(error) => return crate::callback_registry::refused(error) };
    if let NativeCallback::Buffered = installed {
        let saved = unsafe { xm1_power_interrupt_lock() };
        let count = word(0x9134c0);
        let result = if count > 63 {
            let task = unsafe { xm1_pipeline_image_current_task() };
            unsafe { xm1_power_append_stream_text(task & 255, 0x782054) };
            -63
        } else {
            let destination = 0x9121c0 + count as usize * 76;
            put_word(destination, word(record as usize));
            put_word(destination + 4, word(record as usize + 4));
            put_word(0x9134c0, count + 1);
            0
        };
        unsafe { xm1_power_interrupt_unlock(saved) };
        return result;
    }
    let NativeCallback::Ready(callback) = installed else { unreachable!() };
    if let InstalledCallback::Owned(callback) = callback {
        return unsafe { callback(record as *const u8) };
    }
    let task = unsafe { xm1_pipeline_image_current_task() } as u8;
    let primary = word(0x9134f8) == 1;
    let event = word(record as usize);
    let destination = if primary && event & 0x900 != 0 { 7 } else { 1 };
    if event == 0x82e && destination == 1 {
        let issued = word(0x9121bc);
        if issued != word(0x9134c4) {
            unsafe { xm1_power_record_application_gate_marker() };
            return 0;
        }
        put_word(0x9121bc, issued.wrapping_add(1));
    }
    let packet = crate::record_task43::StorageMessage { code: 0, task, destination, length: 76, payload: record };
    let status = unsafe { xm1_pipeline_image_send_message(u32::from(destination), &packet) };
    if event == 0x91b && byte(0x954884) == 0 {
        let first_task = unsafe { xm1_pipeline_image_current_task() } as u8;
        unsafe { core::ptr::write_volatile(0x954884 as *mut u8, first_task) };
    }
    status
}

#[cfg(test)]
#[path = "record_task43_application_tests.rs"]
mod tests;
