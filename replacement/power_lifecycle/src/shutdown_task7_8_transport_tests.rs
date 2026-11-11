use super::*;

struct Resources {
    primary: RegistryHandle,
    secondary: RegistryHandle,
    primary_active: bool,
    secondary_active: bool,
    mode: UsbSecondaryMode,
    completed: Option<ServiceStatus>,
    policy: UsbCompletionPolicy,
}
impl UsbRegistrationStorage for Resources {
    fn handle(&mut self, slot: UsbRegistrationSlot) -> RegistryHandle {
        match slot { UsbRegistrationSlot::Primary => self.primary, UsbRegistrationSlot::Secondary => self.secondary }
    }
    fn clear_handle(&mut self, slot: UsbRegistrationSlot) {
        match slot { UsbRegistrationSlot::Primary => self.primary = RegistryHandle(0), UsbRegistrationSlot::Secondary => self.secondary = RegistryHandle(0) }
    }
    fn clear_primary_activity(&mut self) { self.primary_active = false; }
    fn secondary_mode(&mut self) -> UsbSecondaryMode { self.mode }
    fn secondary_active(&mut self) -> bool { self.secondary_active }
    fn complete_secondary(&mut self, status: ServiceStatus) { self.completed = Some(status); }
    fn clear_secondary_activity(&mut self) { self.secondary_active = false; }
    fn set_completion_policy(&mut self, policy: UsbCompletionPolicy) { self.policy = policy; }
}
struct Registry(std::vec::Vec<RegistryHandle>);
impl RegistrationRegistry for Registry {
    fn release(&mut self, handle: RegistryHandle) -> ServiceStatus {
        self.0.retain(|registered| *registered != handle);
        ServiceStatus(0)
    }
}
fn resources(mode: UsbSecondaryMode) -> Resources {
    Resources { primary: RegistryHandle(7), secondary: RegistryHandle(8), primary_active: true, secondary_active: true, mode, completed: None, policy: UsbCompletionPolicy::Enabled }
}

#[test]
fn primary_unregister_removes_registration_without_completing_secondary() {
    let mut resources = resources(UsbSecondaryMode::Mode2);
    let mut registry = Registry(std::vec![RegistryHandle(7), RegistryHandle(8)]);
    assert_eq!(unregister_primary(&mut resources, &mut registry), ServiceStatus(0));
    assert_eq!(registry.0, [RegistryHandle(8)]);
    assert_eq!(resources.primary, RegistryHandle(0));
    assert!(!resources.primary_active);
    assert_eq!(resources.completed, None);
    assert!(resources.secondary_active);
}

#[test]
fn secondary_unregister_completes_pending_mode2_request_with_minus11() {
    let mut resources = resources(UsbSecondaryMode::Mode2);
    let mut registry = Registry(std::vec![RegistryHandle(7), RegistryHandle(8)]);
    assert_eq!(unregister_secondary(&mut resources, &mut registry), ServiceStatus(0));
    assert_eq!(resources.completed, Some(ServiceStatus(-11)));
    assert!(!resources.secondary_active);
    assert_eq!(resources.secondary, RegistryHandle(0));
    assert_eq!(registry.0, [RegistryHandle(7)]);
}

#[test]
fn secondary_unregister_does_not_touch_other_transport_modes() {
    let mut resources = resources(UsbSecondaryMode::Other);
    let mut registry = Registry(std::vec![RegistryHandle(7), RegistryHandle(8)]);
    assert_eq!(unregister_secondary(&mut resources, &mut registry), ServiceStatus(0));
    assert_eq!(resources.completed, None);
    assert!(resources.secondary_active);
    assert_eq!(resources.secondary, RegistryHandle(8));
    assert_eq!(registry.0, [RegistryHandle(7), RegistryHandle(8)]);
}

#[test]
fn completion_policy_is_read_back_as_requested() {
    let mut resources = resources(UsbSecondaryMode::Mode2);
    set_completion_policy(&mut resources, UsbCompletionPolicy::Suppressed);
    assert_eq!(resources.policy, UsbCompletionPolicy::Suppressed);
    set_completion_policy(&mut resources, UsbCompletionPolicy::Enabled);
    assert_eq!(resources.policy, UsbCompletionPolicy::Enabled);
}
