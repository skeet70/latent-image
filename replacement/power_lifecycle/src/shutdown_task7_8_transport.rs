use crate::primitives::ServiceStatus;
use crate::shutdown_task7_8::RegistryHandle;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UsbRegistrationSlot { Primary, Secondary }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UsbSecondaryMode { Mode2, Other }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UsbCompletionPolicy { Enabled, Suppressed }

pub trait UsbRegistrationStorage {
    fn handle(&mut self, slot: UsbRegistrationSlot) -> RegistryHandle;
    fn clear_handle(&mut self, slot: UsbRegistrationSlot);
    fn clear_primary_activity(&mut self);
    fn secondary_mode(&mut self) -> UsbSecondaryMode;
    fn secondary_active(&mut self) -> bool;
    fn complete_secondary(&mut self, status: ServiceStatus);
    fn clear_secondary_activity(&mut self);
    fn set_completion_policy(&mut self, policy: UsbCompletionPolicy);
}

pub trait RegistrationRegistry {
    fn release(&mut self, handle: RegistryHandle) -> ServiceStatus;
}

pub fn unregister_primary(
    storage: &mut impl UsbRegistrationStorage,
    registry: &mut impl RegistrationRegistry,
) -> ServiceStatus {
    let handle = storage.handle(UsbRegistrationSlot::Primary);
    let status = registry.release(handle);
    storage.clear_handle(UsbRegistrationSlot::Primary);
    storage.clear_primary_activity();
    status
}

pub fn unregister_secondary(
    storage: &mut impl UsbRegistrationStorage,
    registry: &mut impl RegistrationRegistry,
) -> ServiceStatus {
    if storage.secondary_mode() != UsbSecondaryMode::Mode2 { return ServiceStatus(0); }
    if storage.secondary_active() {
        storage.complete_secondary(ServiceStatus(-11));
        storage.clear_secondary_activity();
    }
    let handle = storage.handle(UsbRegistrationSlot::Secondary);
    let status = registry.release(handle);
    storage.clear_handle(UsbRegistrationSlot::Secondary);
    status
}

pub fn set_completion_policy(storage: &mut impl UsbRegistrationStorage, policy: UsbCompletionPolicy) {
    storage.set_completion_policy(policy);
}

#[cfg(target_arch = "arm")]
mod native {
    use super::*;
    use core::ptr::{read_volatile, write_volatile};

    pub type OwnedSecondaryCompletion = unsafe extern "C" fn(ServiceStatus);
    pub struct NativeUsbRegistrations {
        pub secondary_completion: Option<OwnedSecondaryCompletion>,
    }
    pub struct NativeRegistrationRegistry;
    impl RegistrationRegistry for NativeRegistrationRegistry {
        fn release(&mut self, handle: RegistryHandle) -> ServiceStatus {
            unsafe { crate::shutdown_task7_8::unregister_registry_handle(handle) }
        }
    }
    impl UsbRegistrationSlot {
        const fn handle_address(self) -> u32 {
            match self { Self::Primary => 0x146a09c, Self::Secondary => 0x146a0b4 }
        }
    }
    impl UsbRegistrationStorage for NativeUsbRegistrations {
        fn handle(&mut self, slot: UsbRegistrationSlot) -> RegistryHandle {
            RegistryHandle(unsafe { read_volatile(slot.handle_address() as *const u32) })
        }
        fn clear_handle(&mut self, slot: UsbRegistrationSlot) {
            unsafe { write_volatile(slot.handle_address() as *mut u32, 0) };
        }
        fn clear_primary_activity(&mut self) { unsafe { write_volatile(0x146a094 as *mut u32, 0) }; }
        fn secondary_mode(&mut self) -> UsbSecondaryMode {
            if unsafe { read_volatile(0x146a0c0 as *const u32) } == 2 { UsbSecondaryMode::Mode2 } else { UsbSecondaryMode::Other }
        }
        fn secondary_active(&mut self) -> bool { (unsafe { read_volatile(0x146a0a8 as *const u32) }) == 1 }
        fn complete_secondary(&mut self, status: ServiceStatus) {
            if let Some(completion) = self.secondary_completion { unsafe { completion(status) }; }
        }
        fn clear_secondary_activity(&mut self) { unsafe { write_volatile(0x146a0a8 as *mut u32, 0) }; }
        fn set_completion_policy(&mut self, policy: UsbCompletionPolicy) {
            unsafe { write_volatile(0x1469d1c as *mut u8, 0) };
            if policy == UsbCompletionPolicy::Enabled {
                unsafe { write_volatile(0x1469d1c as *mut u8, 1) };
            }
        }
    }

    #[no_mangle]
    pub extern "C" fn xm1_power_usb_unregister_primary() -> i32 {
        unregister_primary(&mut NativeUsbRegistrations { secondary_completion: None }, &mut NativeRegistrationRegistry).0
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_usb_unregister_secondary(completion: Option<OwnedSecondaryCompletion>) -> i32 {
        unregister_secondary(&mut NativeUsbRegistrations { secondary_completion: completion }, &mut NativeRegistrationRegistry).0
    }
    #[no_mangle]
    pub extern "C" fn xm1_power_usb_set_completion_policy(suppress: u32) {
        let policy = if suppress == 0 { UsbCompletionPolicy::Enabled } else { UsbCompletionPolicy::Suppressed };
        set_completion_policy(&mut NativeUsbRegistrations { secondary_completion: None }, policy);
    }
}
#[cfg(target_arch = "arm")]
pub use native::{NativeRegistrationRegistry, NativeUsbRegistrations, OwnedSecondaryCompletion};

#[cfg(test)]
#[path = "shutdown_task7_8_transport_tests.rs"]
mod tests;
