#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PinValue(u16);
impl PinValue {
    pub const fn from_abi(value: u32) -> Self { Self((value & 0x3ff) as u16) }
    pub const fn word(self) -> u32 { self.0 as u32 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PinRegister { Data, Control }
impl PinRegister {
    pub const fn address(self) -> u32 {
        match self { Self::Data => 0xfff86008, Self::Control => 0xfff86004 }
    }
}
pub trait PinDevice { fn write(&mut self, register: PinRegister, value: u32); }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PinWriteError { NotInitialized, UnsupportedPin }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PinIndex(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PinLifecycle(u32);
impl PinLifecycle {
    pub const fn from_abi(value: u32) -> Self { Self(value) }
    pub const fn abi(self) -> u32 { self.0 }
    pub fn set(&self, pin: PinIndex, value: PinValue, device: &mut impl PinDevice) -> Result<(), PinWriteError> {
        if self.0 != 1 { return Err(PinWriteError::NotInitialized); }
        if pin.0 != 0 { return Err(PinWriteError::UnsupportedPin); }
        device.write(PinRegister::Data, value.word());
        Ok(())
    }
    pub fn release(&mut self, device: &mut impl PinDevice) {
        let _ = self.set(PinIndex(0), PinValue(0), device);
        device.write(PinRegister::Control, 0);
        self.0 = 2;
    }
}

#[cfg(target_arch = "arm")]
pub struct NativePins;
#[cfg(target_arch = "arm")]
impl PinDevice for NativePins {
    fn write(&mut self, register: PinRegister, value: u32) {
        unsafe { core::ptr::write_volatile(register.address() as *mut u32, value) }
    }
}

pub const fn transfer_pipeline_noop() -> i32 { 0 }

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Device(std::vec::Vec<(PinRegister, u32)>);
    impl PinDevice for Device { fn write(&mut self, register: PinRegister, value: u32) { self.0.push((register, value)); } }
    #[test]
    fn release_drives_initialized_pin_low_and_disables_controller() {
        let mut lifecycle = PinLifecycle::from_abi(1);
        let mut device = Device::default();
        lifecycle.release(&mut device);
        assert_eq!(device.0, [(PinRegister::Data, 0), (PinRegister::Control, 0)]);
        assert_eq!(lifecycle.abi(), 2);
        assert_eq!(lifecycle.set(PinIndex(0), PinValue::from_abi(9), &mut device), Err(PinWriteError::NotInitialized));
    }
    #[test]
    fn release_still_disables_controller_when_not_initialized() {
        let mut lifecycle = PinLifecycle::from_abi(0);
        let mut device = Device::default();
        lifecycle.release(&mut device);
        assert_eq!(device.0, [(PinRegister::Control, 0)]);
        assert_eq!(lifecycle.abi(), 2);
    }
    #[test]
    fn data_values_are_ten_bits_and_only_pin_zero_is_supported() {
        let lifecycle = PinLifecycle::from_abi(1);
        let mut device = Device::default();
        assert_eq!(lifecycle.set(PinIndex(1), PinValue::from_abi(0), &mut device), Err(PinWriteError::UnsupportedPin));
        assert!(device.0.is_empty());
        assert_eq!(lifecycle.set(PinIndex(0), PinValue::from_abi(0xfffffcab), &mut device), Ok(()));
        assert_eq!(device.0, [(PinRegister::Data, 0xab)]);
    }
}
