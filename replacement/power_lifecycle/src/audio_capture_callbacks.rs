use crate::primitives::ServiceStatus;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureAction { CompleteFirst, CompleteSecond, Noop, Gpio33Low, Gpio13Low, Gpio13High, Gpio2Low, Gpio1Low, Gpio2High, Gpio1High, ReleaseFields, AcquireClock, ClearSixThroughEleven, EnableSixAndSeven, Notify17, Notify15, Notify14 }
impl CaptureAction {
    pub const fn from_abi(address: u32) -> Option<Self> {
        Some(match address {
            0x1002dbc | 0x1002e04 => Self::CompleteFirst, 0x1002de0 => Self::CompleteSecond,
            0x1002e28 => Self::Noop, 0x1002e3c => Self::Gpio33Low,
            0x1002e54 => Self::Gpio13Low, 0x1002e6c => Self::Gpio13High,
            0x1002e84 => Self::Gpio2Low, 0x1002e9c => Self::Gpio1Low,
            0x1002eb4 => Self::Gpio2High, 0x1002ecc => Self::Gpio1High,
            0x1002ee4 => Self::ReleaseFields, 0x1002ef4 => Self::AcquireClock,
            0x1002f04 => Self::ClearSixThroughEleven, 0x1002fa0 => Self::EnableSixAndSeven,
            0xfeafe0 | 0xffdfe8 => Self::Notify17, 0xfeaffc | 0xffe004 => Self::Notify15, 0xfeb018 => Self::Notify14,
            _ => return None,
        })
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct CaptureGpio(pub u8);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GpioLevel { Low, High }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureCompletion { First, Second }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureField { ClockGate090f, Secondary0202 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct FieldValue(pub u8);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClockSourceState { Disabled, Enabled }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureEvent { Completed, Intermediate, Ready }
impl CaptureEvent {
    pub const fn command(self) -> u16 { match self { Self::Completed => 0x3f17, Self::Intermediate => 0x3f15, Self::Ready => 0x3f14 } }
}

pub trait CaptureHardware {
    fn complete(&mut self, completion: CaptureCompletion) -> Result<(), u32>;
    fn gpio_level(&mut self, gpio: CaptureGpio, level: GpioLevel);
    fn gpio_mode_zero(&mut self, gpio: CaptureGpio);
    fn gpio_mode_one(&mut self, gpio: CaptureGpio);
    fn clock_mode(&mut self) -> u8;
    fn select_clock_source(&mut self, state: ClockSourceState);
    fn set_clock_divider(&mut self, divider: u32);
    fn acquire_clock_resource(&mut self) -> ServiceStatus;
    fn release_clock_resource(&mut self) -> ServiceStatus;
    fn release_field_value(&self, field: CaptureField) -> FieldValue;
    fn request_field(&mut self, field: CaptureField, value: FieldValue);
    fn post_event(&mut self, event: CaptureEvent);
}

pub fn invoke(hardware: &mut impl CaptureHardware, action: CaptureAction) -> Result<(), u32> {
    use CaptureAction::*;
    match action {
        CompleteFirst => hardware.complete(CaptureCompletion::First)?,
        CompleteSecond => hardware.complete(CaptureCompletion::Second)?,
        Noop => (),
        Gpio33Low => hardware.gpio_level(CaptureGpio(33), GpioLevel::Low),
        Gpio13Low => hardware.gpio_level(CaptureGpio(13), GpioLevel::Low),
        Gpio13High => hardware.gpio_level(CaptureGpio(13), GpioLevel::High),
        Gpio2Low => hardware.gpio_level(CaptureGpio(2), GpioLevel::Low),
        Gpio1Low => hardware.gpio_level(CaptureGpio(1), GpioLevel::Low),
        Gpio2High => hardware.gpio_level(CaptureGpio(2), GpioLevel::High),
        Gpio1High => hardware.gpio_level(CaptureGpio(1), GpioLevel::High),
        ReleaseFields => {
            let first = hardware.release_field_value(CaptureField::ClockGate090f);
            hardware.request_field(CaptureField::ClockGate090f, first);
            let second = hardware.release_field_value(CaptureField::Secondary0202);
            hardware.request_field(CaptureField::Secondary0202, second);
            let divider = match hardware.clock_mode() {
                2 => { hardware.select_clock_source(ClockSourceState::Disabled); 2 }
                4 => { hardware.gpio_level(CaptureGpio(12), GpioLevel::Low); 4 }
                _ => { hardware.select_clock_source(ClockSourceState::Disabled); 1 }
            };
            hardware.set_clock_divider(divider);
            let _ = hardware.release_clock_resource();
        }
        AcquireClock => {
            let divider = match hardware.clock_mode() {
                2 => { hardware.select_clock_source(ClockSourceState::Enabled); 2 }
                4 => { hardware.gpio_level(CaptureGpio(12), GpioLevel::High); 4 }
                _ => { hardware.select_clock_source(ClockSourceState::Enabled); 1 }
            };
            hardware.set_clock_divider(divider);
            let _ = hardware.acquire_clock_resource();
        }
        ClearSixThroughEleven => {
            for pin in 6..=11 { hardware.gpio_mode_zero(CaptureGpio(pin)); }
            for pin in [7, 6, 8, 9, 10, 11] { hardware.gpio_level(CaptureGpio(pin), GpioLevel::Low); }
        }
        EnableSixAndSeven => {
            hardware.gpio_mode_zero(CaptureGpio(6));
            hardware.gpio_mode_zero(CaptureGpio(7));
            hardware.gpio_level(CaptureGpio(6), GpioLevel::High);
            hardware.gpio_level(CaptureGpio(7), GpioLevel::High);
            hardware.gpio_mode_one(CaptureGpio(6));
            hardware.gpio_mode_one(CaptureGpio(7));
        }
        Notify17 => hardware.post_event(CaptureEvent::Completed),
        Notify15 => hardware.post_event(CaptureEvent::Intermediate),
        Notify14 => hardware.post_event(CaptureEvent::Ready),
    }
    Ok(())
}

#[cfg(target_arch = "arm")]
pub mod native {
    use super::*;
    use core::ptr::{read_volatile, write_volatile};
    pub trait CaptureRouteFields {
        fn select_source(&mut self, state: ClockSourceState);
        fn request(&mut self, field: CaptureField, value: FieldValue);
    }
    pub struct InstalledRouteFields;
    impl CaptureRouteFields for InstalledRouteFields {
        fn select_source(&mut self, state: ClockSourceState) {
            unsafe { let _ = xm1_scanout_avout_request(0, if state == ClockSourceState::Enabled { 1 } else { 0 }); }
        }
        fn request(&mut self, field: CaptureField, value: FieldValue) {
            let (selector, slot) = match field { CaptureField::ClockGate090f => (0x90f, 1), CaptureField::Secondary0202 => (0x202, 10) };
            unsafe { let _ = xm1_power_resource_route_request(selector, slot, u32::from(value.0)); }
        }
    }
    pub struct NativeCaptureHardware<R> { pub routes: R }
    extern "C" {
        fn xm1_power_configuration_byte(offset: u32) -> u32;
        fn xm1_power_resource_acquire(identifier: u32, output: *mut u32) -> i32;
        fn xm1_power_resource_release(handle: u32) -> i32;
        fn xm1_power_resource_route_request(selector: u32, slot: u32, level: u32) -> i32;
        fn xm1_scanout_avout_request(resource: u32, request: u32) -> i32;
        fn xm1_power_interrupt_lock() -> u32;
        fn xm1_power_interrupt_unlock(saved: u32);
        fn xm1_power_operation_action(address: u32) -> i32;
    }
    unsafe fn word(address: u32) -> u32 { read_volatile(address as *const u32) }
    fn gpio_mapping(gpio: CaptureGpio) -> Option<(u32, u32)> {
        if gpio.0 > 97 { return None; }
        let mapping = unsafe { word(0x81aa98 + u32::from(gpio.0) * 4) };
        let port = mapping >> 16;
        let bit = mapping & 0xffff;
        if port > 17 || bit > 15 { None } else { Some((port & 15, bit & 15)) }
    }
    impl<R: CaptureRouteFields> CaptureHardware for NativeCaptureHardware<R> {
        fn complete(&mut self, completion: CaptureCompletion) -> Result<(), u32> {
            let address = match completion { CaptureCompletion::First => 0x16f853c, CaptureCompletion::Second => 0x16f8540 };
            let callback = unsafe { word(address) };
            if callback == 0 || unsafe { xm1_power_operation_action(callback) } == 0 { Ok(()) } else { Err(callback) }
        }
        fn gpio_level(&mut self, gpio: CaptureGpio, level: GpioLevel) {
            let Some((port, bit)) = gpio_mapping(gpio) else { return; };
            let value = (1 << (bit + 16)) | if level == GpioLevel::High { 1 << bit } else { 0 };
            unsafe { write_volatile((0xfff89000 + port * 64) as *mut u32, value) };
        }
        fn gpio_mode_zero(&mut self, gpio: CaptureGpio) {
            let Some((port, bit)) = gpio_mapping(gpio) else { return; };
            let offset = if bit <= 7 { 12 } else { 16 };
            unsafe { write_volatile((0xfff89000 + port * 64 + offset) as *mut u32, 3 << ((bit & 7) * 2 + 16)) };
        }
        fn gpio_mode_one(&mut self, gpio: CaptureGpio) {
            let Some((port, bit)) = gpio_mapping(gpio) else { return; };
            let offset = if bit <= 7 { 12 } else { 16 };
            let value = (3 << ((bit & 7) * 2 + 16)) | (1 << ((bit & 7) * 2));
            unsafe { write_volatile((0xfff89000 + port * 64 + offset) as *mut u32, value) };
        }
        fn clock_mode(&mut self) -> u8 { unsafe { xm1_power_configuration_byte(0x18ff) as u8 } }
        fn select_clock_source(&mut self, state: ClockSourceState) { self.routes.select_source(state); }
        fn set_clock_divider(&mut self, divider: u32) {
            unsafe {
                let saved = xm1_power_interrupt_lock();
                write_volatile(0xfff88024 as *mut u32, word(0xfff88024) & !7 | divider);
                xm1_power_interrupt_unlock(saved);
            }
        }
        fn acquire_clock_resource(&mut self) -> ServiceStatus {
            if unsafe { word(0x169c1cc) } != 0 { return ServiceStatus(0); }
            let status = unsafe { xm1_power_resource_acquire(39, 0x169c1cc as *mut u32) };
            ServiceStatus(if status == 0 { 0 } else { 134 })
        }
        fn release_clock_resource(&mut self) -> ServiceStatus {
            let handle = unsafe { word(0x169c1cc) };
            if handle == 0 { return ServiceStatus(0); }
            let status = unsafe { xm1_power_resource_release(handle) };
            if status == 0 { unsafe { write_volatile(0x169c1cc as *mut u32, 0) }; }
            ServiceStatus(if status == 0 { 0 } else { 134 })
        }
        fn release_field_value(&self, field: CaptureField) -> FieldValue {
            let address = match field { CaptureField::ClockGate090f => 0xef3340, CaptureField::Secondary0202 => 0xef3348 };
            FieldValue(unsafe { read_volatile(address as *const u8) })
        }
        fn request_field(&mut self, field: CaptureField, value: FieldValue) { self.routes.request(field, value); }
        fn post_event(&mut self, event: CaptureEvent) {
            use crate::shutdown::{QueueId, RequestId, ServiceMessage, ShutdownRtos};
            let mut rtos = crate::shutdown_rtos::NativeShutdownRtos;
            let sender = rtos.current_task();
            let destination = QueueId(unsafe { word(0x16f0aa4) } as u8);
            let _ = rtos.post(ServiceMessage { operation: RequestId(event.command()), sender, destination, payload: &[] });
        }
    }

    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_capture_operation_action(address: u32) -> i32 {
        let Some(action) = CaptureAction::from_abi(address) else { return -255; };
        match invoke(&mut NativeCaptureHardware { routes: InstalledRouteFields }, action) {
            Ok(()) => 0,
            Err(_) => -255,
        }
    }
}
