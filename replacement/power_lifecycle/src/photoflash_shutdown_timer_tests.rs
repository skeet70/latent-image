use super::*;
use crate::photoflash_shutdown::timer::{self, SavedInterrupts, TimerRuntime};

impl TimerRuntime for Device {
    fn release_irq_handle(&mut self, handle: TimerHandle) -> i32 {
        if self.irq_release_status == 0 { self.irq_handles.remove(&handle.0); }
        self.irq_release_status
    }
    fn lock_interrupts(&mut self) -> SavedInterrupts {
        let previous = self.interrupt_state;
        self.interrupt_state = 0xc0;
        SavedInterrupts(previous)
    }
    fn unlock_interrupts(&mut self, saved: SavedInterrupts) { self.interrupt_state = saved.0; }
    fn timer_diagnostic(&mut self, category: u32, code: u32) { self.diagnostics.push((category, code)); }
}
fn registered(index: u32) -> Device {
    let mut device = Device::default();
    device.write32(Address(0x955a60 + index * 4), 123);
    device.write32(Address(0x955a00 + index * 4), 2);
    device.write32(Address(0x955a30 + index * 4), 9);
    device.write32(Address(0x955a90 + index * 4), 456);
    device.write32(Address(0x955ac0), 0xfff);
    device.interrupt_state = 0x13;
    device.irq_handles.insert(456);
    device
}
#[test]
fn stops_and_deletes_each_of_the_twelve_timer_slots() {
    for index in 0..12 {
        let mut device = registered(index);
        assert_eq!(timer::stop(&mut device, TimerHandle(123)), 0);
        assert_eq!(device.read32(Address(0xfffa9000)), 0x1_0000 << index);
        assert_eq!(device.read32(Address(0x955ac0)), 0xfff & !(1 << index));
        assert_eq!(device.interrupt_state, 0x13);
        assert!(device.irq_handles.is_empty());
        assert_eq!(device.read32(Address(0x955a00 + index * 4)), 1);
        assert_eq!(timer::delete(&mut device, TimerHandle(123)), 0);
        assert_eq!(device.read32(Address(0x955a60 + index * 4)), 0);
        assert_eq!(device.read32(Address(0x955a00 + index * 4)), 0);
        assert_eq!(device.read32(Address(0x955a30 + index * 4)), u32::MAX);
    }
}
#[test]
fn stale_handles_are_rejected_and_delete_reports_the_stock_diagnostic() {
    let mut device = registered(0);
    for handle in [TimerHandle(0), TimerHandle(999)] {
        assert_eq!(timer::stop(&mut device, handle), -20);
        assert_eq!(timer::delete(&mut device, handle), -20);
    }
    assert_eq!(device.diagnostics, [(2, 18), (2, 18)]);
    assert_eq!(device.read32(Address(0xfffa9000)), 0);
    assert!(device.irq_handles.contains(&456));
    assert_eq!(device.read32(Address(0x955a60)), 123);
}
#[test]
fn repeated_stop_returns_not_started_without_releasing_again() {
    let mut device = registered(0);
    assert_eq!(timer::stop(&mut device, TimerHandle(123)), 0);
    assert_eq!(timer::stop(&mut device, TimerHandle(123)), -11);
    assert_eq!(timer::delete(&mut device, TimerHandle(123)), 0);
}
#[test]
fn failed_irq_release_still_clears_slot_and_mask_but_returns_error() {
    let mut device = registered(3);
    device.irq_release_status = -20;
    assert_eq!(timer::stop(&mut device, TimerHandle(123)), -20);
    assert!(device.irq_handles.contains(&456));
    assert_eq!(device.read32(Address(0x955a90 + 3 * 4)), 0);
    assert_eq!(device.read32(Address(0x955ac0)), 0xff7);
    assert_eq!(device.interrupt_state, 0x13);
    assert_eq!(device.read32(Address(0x955a00 + 3 * 4)), 1);
}
#[test]
fn no_irq_token_and_empty_mask_are_valid_stop_paths() {
    let mut device = registered(11);
    device.write32(Address(0x955a90 + 11 * 4), 0);
    device.write32(Address(0x955ac0), 0);
    assert_eq!(timer::stop(&mut device, TimerHandle(123)), 0);
    assert_eq!(device.read32(Address(0x955ac0)), 0);
    assert!(device.irq_handles.contains(&456));
    assert_eq!(device.interrupt_state, 0x13);
}
