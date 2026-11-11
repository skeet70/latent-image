//! Queue 1/7 notification publication over Power's service post.
#![cfg(target_arch = "arm")]

use crate::control_types::KeyEvent;
use crate::key_ui::*;

pub trait UiPlatform {
    fn loadtest_state(&mut self) -> LoadtestState;
    fn trace(&mut self, trace: PublicationTrace);
    fn count_key(&mut self, key: KeyEvent);
    fn initialize_notification_source(&mut self);
}

#[repr(C)]
struct Descriptor { operation: u16, sender: u8, destination: u8, length: u32, payload: *const u8 }

extern "C" {
    fn xm1_power_current_task() -> u32;
    fn xm1_power_service_post(queue: u32, header: *const Descriptor) -> i32;
}

pub struct NativeUiServices<P: UiPlatform> { pub platform: P }

impl<P: UiPlatform> PublicationServices for NativeUiServices<P> {
    fn current_task(&mut self) -> PublicationTask { PublicationTask(unsafe { xm1_power_current_task() } as u8) }
    fn loadtest_state(&mut self) -> LoadtestState { self.platform.loadtest_state() }
    fn send_notification(&mut self, message: NotificationMessage) -> PublicationStatus {
        let payload = message.payload();
        let destination = message.destination as u8;
        let descriptor = Descriptor {
            operation: NotificationMessage::MESSAGE_CODE, sender: message.sender.0, destination,
            length: payload.len() as u32, payload: payload.as_ptr(),
        };
        PublicationStatus(unsafe { xm1_power_service_post(u32::from(destination), &descriptor) })
    }
    fn trace_publication(&mut self, trace: PublicationTrace) { self.platform.trace(trace) }
    fn count_key(&mut self, key: KeyEvent) { self.platform.count_key(key) }
    fn initialize_ui_notification_source(&mut self) { self.platform.initialize_notification_source() }
}
