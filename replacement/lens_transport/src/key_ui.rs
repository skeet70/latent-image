//! X-M1 1.50 publication CPU, x70view 55436fc85c0560dd252b0410af6f2427fe198cd3.
//! Input tu_006a0658.c, OSAL device_notification_router.c and tu_006b6b60.c,
//! settings tu_0072ccf0.c; image-4 bytes at 6a1b40, 6a2aec, 72b210,
//! 72b680, 6b723c, 6b7694, 6b76e0, 6b7850 and 72cd40.
//! Image-4 SHA-256: f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16.
//! Queue ingress, stabilization hardware and A/B platform binding are caller-owned.
//! Owned payload-free publications zero the 68 bytes stock leaves unspecified.

use crate::control_device::KeyPublication;
use crate::control_types::{KeyEvent, KeyPublicationSink};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PublicationStatus(pub i32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PublicationTask(pub u8);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum UiQueue { Main = 1, Loadtest = 7 }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LoadtestState { Active, Inactive }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublicationTrace { PrimaryPress, FullPress, UpdateSuppressed }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NotificationMessage {
    pub sender: PublicationTask,
    pub destination: UiQueue,
    pub publication: KeyPublication,
}

impl NotificationMessage {
    pub const MESSAGE_CODE: u16 = 0;
    pub const PAYLOAD_LENGTH: usize = 76;

    pub fn payload(&self) -> [u8; Self::PAYLOAD_LENGTH] {
        let mut bytes = [0; Self::PAYLOAD_LENGTH];
        bytes[..4].copy_from_slice(&self.publication.key.0.to_le_bytes());
        bytes[4..8].copy_from_slice(&self.publication.argument.to_le_bytes());
        bytes[8..].copy_from_slice(&self.publication.data);
        bytes
    }
}

/// The message ingress copies the packet before returning.
pub trait PublicationServices {
    fn current_task(&mut self) -> PublicationTask;
    fn loadtest_state(&mut self) -> LoadtestState;
    fn send_notification(&mut self, message: NotificationMessage) -> PublicationStatus;
    fn trace_publication(&mut self, trace: PublicationTrace);
    fn count_key(&mut self, key: KeyEvent);
    fn initialize_ui_notification_source(&mut self);
}

#[derive(Debug, Eq, PartialEq)]
pub struct NotificationState {
    pub update_count: u32,
    pub expected_update_count: u32,
}

pub struct Publisher<'a, S> {
    pub services: &'a mut S,
    pub state: &'a mut NotificationState,
}

impl<S: PublicationServices> Publisher<'_, S> {
    fn input_effects(&mut self, key: KeyEvent) {
        match key.0 {
            7 => self.services.trace_publication(PublicationTrace::PrimaryPress),
            8 => self.services.trace_publication(PublicationTrace::FullPress),
            0x2b..=0x2d => self.services.count_key(key),
            _ => {}
        }
    }

    /// The initialized input callback and UI callback both route to the owned ingress.
    pub fn release(&mut self, key: KeyEvent, argument: u32) -> PublicationStatus {
        self.publish(KeyPublication { key, argument, data: [0; 68] })
    }

    pub fn publish(&mut self, publication: KeyPublication) -> PublicationStatus {
        self.input_effects(publication.key);
        self.notify(publication)
    }

    pub fn notify(&mut self, publication: KeyPublication) -> PublicationStatus {
        let sender = self.services.current_task();
        let destination = if self.services.loadtest_state() == LoadtestState::Active
            && publication.key.0 & 0x900 != 0 {
            UiQueue::Loadtest
        } else {
            UiQueue::Main
        };
        if publication.key == KeyEvent(0x82e) && destination == UiQueue::Main {
            if self.state.update_count != self.state.expected_update_count {
                self.services.trace_publication(PublicationTrace::UpdateSuppressed);
                return PublicationStatus(0);
            }
            self.state.update_count = self.state.update_count.wrapping_add(1);
        }
        let initialize = publication.key == KeyEvent(0x91b);
        let status = self.services.send_notification(NotificationMessage { sender, destination, publication });
        if initialize {
            self.services.initialize_ui_notification_source();
        }
        status
    }

    pub fn stabilization_changed(&mut self) -> PublicationStatus {
        self.notify(KeyPublication { key: KeyEvent(0x91d), argument: 0, data: [0; 68] })
    }
}

impl<S: PublicationServices> KeyPublicationSink for Publisher<'_, S> {
    fn publish_key(&mut self, publication: KeyPublication) {
        let _ = self.publish(publication);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UiCallback { StabilizationChanged }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StabilizationMode { Mode0, Mode1, Mode2, Mode3, Mode4, Mode5, Mode6, Mode7, Mode8, Mode9 }

impl TryFrom<u32> for StabilizationMode {
    type Error = PublicationStatus;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Ok(match value {
            0 => Self::Mode0, 1 => Self::Mode1, 2 => Self::Mode2, 3 => Self::Mode3,
            4 => Self::Mode4, 5 => Self::Mode5, 6 => Self::Mode6, 7 => Self::Mode7,
            8 => Self::Mode8, 9 => Self::Mode9,
            _ => return Err(PublicationStatus(-255)),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum StabilizationSelection { First = 0, Second = 1, Third = 2 }

pub trait StabilizationServices: PublicationServices {
    fn begin_stabilization_mode(&mut self, selection: StabilizationSelection);
}

#[derive(Debug, Default)]
pub struct StabilizationCallbacks {
    completion: Option<UiCallback>,
    change: Option<UiCallback>,
}

impl StabilizationCallbacks {
    pub const fn new() -> Self { Self { completion: None, change: None } }

    fn invoke<S: PublicationServices>(callback: Option<UiCallback>, publisher: &mut Publisher<'_, S>) {
        if let Some(UiCallback::StabilizationChanged) = callback {
            let _ = publisher.stabilization_changed();
        }
    }

    pub fn reset(&mut self) {
        self.completion = None;
        self.change = None;
    }

    pub fn register<S: StabilizationServices>(
        &mut self,
        mode: StabilizationMode,
        completion: Option<UiCallback>,
        change: Option<UiCallback>,
        publisher: &mut Publisher<'_, S>,
    ) -> PublicationStatus {
        let selection = match mode {
            StabilizationMode::Mode1 => StabilizationSelection::First,
            StabilizationMode::Mode2 => StabilizationSelection::Second,
            StabilizationMode::Mode3 => StabilizationSelection::Third,
            _ => {
                Self::invoke(completion, publisher);
                return PublicationStatus(0);
            }
        };
        self.completion = completion;
        publisher.services.begin_stabilization_mode(selection);
        if change.is_some() {
            self.change = change;
        }
        PublicationStatus(0)
    }

    /// Completion does not clear the registered callback in the stock interface.
    pub fn mode_completed<S: PublicationServices>(&mut self, publisher: &mut Publisher<'_, S>) {
        Self::invoke(self.completion, publisher);
    }

    /// Entry 6b7850 invokes the registered change callback with argument 4.
    pub fn changed<S: PublicationServices>(&mut self, publisher: &mut Publisher<'_, S>) {
        Self::invoke(self.change, publisher);
    }
}

#[cfg(test)]
#[path = "key_ui_tests.rs"]
mod tests;
