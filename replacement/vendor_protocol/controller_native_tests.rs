use super::*;

#[derive(Default)]
struct Device { writes: Vec<(Register, u32)> }
impl ControllerIo for Device {
    fn read32(&mut self, _: Register) -> u32 { 0 }
    fn read16(&mut self, _: Register) -> u16 { 0 }
    fn write32(&mut self, register: Register, value: u32) { self.writes.push((register, value)); }
    fn write16(&mut self, register: Register, value: u16) { self.writes.push((register, u32::from(value))); }
}

#[derive(Default)]
struct Controller { statuses: u32, runs: u32 }
impl NativeController for Controller {
    fn transport_status(&mut self) { self.statuses += 1; }
    fn run(&mut self) -> bool { self.runs += 1; false }
}

#[test]
fn the_interrupt_entry_writes_the_stock_prologue_before_signalling() {
    let mut device = Device::default();
    let mut controller = Controller::default();
    interrupt(&mut device, Some(&mut controller));
    assert_eq!(device.writes, [
        (Register::InterruptReset, 0x8181_0303),
        (Register::EventClearLow, 0xffff),
        (Register::EventClearMiddle, 0xffff),
        (Register::EventClearHigh, 0xffff),
    ]);
    assert_eq!(controller.statuses, 1);
}

#[test]
fn an_uninstalled_interrupt_writes_the_prologue_only() {
    let mut device = Device::default();
    interrupt(&mut device, None);
    assert_eq!(device.writes.len(), 4);
}

#[test]
fn the_task_entry_runs_the_installed_controller_once() {
    let mut controller = Controller::default();
    task(Some(&mut controller));
    assert_eq!(controller.runs, 1);
    task(None);
}
