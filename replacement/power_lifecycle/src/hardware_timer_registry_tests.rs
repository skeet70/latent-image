use super::*;
use std::vec::Vec;
#[derive(Default)]
struct Device { writes: Vec<(Channel, Register, u32)>, attached: Vec<Channel>, detached: Vec<IrqToken>, attach_error: i32, detach_error: i32 }
impl Hardware for Device {
    fn write(&mut self, channel: Channel, register: Register, value: u32) { self.writes.push((channel, register, value)); }
    fn attach(&mut self, channel: Channel) -> Result<IrqToken, i32> { self.attached.push(channel); if self.attach_error == 0 { Ok(IrqToken(72)) } else { Err(self.attach_error) } }
    fn detach(&mut self, token: IrqToken) -> i32 { self.detached.push(token); self.detach_error }
}
fn configuration(period: u32, units: Units, mode: u32) -> Configuration { Configuration { period: Period(period), units, mode: Mode(mode), callback: Some(Callback(0x123400)) } }
#[test]
fn executor_wake_uses_physical_channel_one_without_synthetic_completion() {
    let mut registry = Registry::new();
    let mut generation = Generation(0);
    let channel = Channel::parse(1).unwrap();
    let handle = registry.acquire(channel, &mut generation).unwrap();
    let mut device = Device::default();
    assert_eq!(registry.start(&mut device, handle, configuration(1000, Units::Microseconds, 1)), 0);
    assert_eq!(device.writes, [(channel, Register::Counter, 0), (channel, Register::Reload, 12000), (channel, Register::Control, 3)]);
    assert_eq!(device.attached, [channel]);
    assert_eq!(registry.interrupt(&mut device, channel), Callback(0x123400));
    assert!(device.detached.is_empty());
    assert_eq!(registry.stop(&mut device, handle), 0);
    assert_eq!(device.detached, [IrqToken(72)]);
    assert_eq!(registry.release(handle), Ok(()));
    assert_eq!(registry.stop(&mut device, handle), -20);
    let next = registry.acquire(channel, &mut generation).unwrap();
    assert_ne!(next, handle);
}
#[test]
fn one_shot_stops_before_returning_callback_even_when_detach_fails() {
    let mut registry = Registry::new();
    let channel = Channel::parse(2).unwrap();
    let handle = registry.acquire(channel, &mut Generation(5)).unwrap();
    let mut device = Device { detach_error: -255, ..Device::default() };
    assert_eq!(registry.start(&mut device, handle, configuration(20, Units::Microseconds, 0)), 0);
    assert_eq!(registry.interrupt(&mut device, channel), Callback(0x123400));
    assert_eq!(device.writes[3..], [(channel, Register::Control, 0), (channel, Register::Reload, 0)]);
    assert_eq!(registry.stop(&mut device, handle), -11);
}
#[test]
fn registration_failure_keeps_started_device_until_explicit_stop() {
    let mut registry = Registry::new();
    let handle = registry.acquire(Channel::parse(0).unwrap(), &mut Generation(0)).unwrap();
    let mut device = Device { attach_error: -255, ..Device::default() };
    let request = configuration(1000, Units::Microseconds, 1);
    assert_eq!(registry.start(&mut device, handle, request), -255);
    assert_eq!(device.writes.last().unwrap().2, 3);
    assert_eq!(registry.start(&mut device, handle, request), -12);
    assert_eq!(registry.stop(&mut device, handle), 0);
    assert!(device.detached.is_empty());
}
#[test]
fn generation_wrap_skips_zero_and_live_handle_collision() {
    let mut registry = Registry::new();
    let first = registry.acquire(Channel::parse(0).unwrap(), &mut Generation(0)).unwrap();
    let second = registry.acquire(Channel::parse(1).unwrap(), &mut Generation(u32::MAX)).unwrap();
    assert_eq!(first, Handle(1));
    assert_eq!(second, Handle(2));
    assert_eq!(registry.acquire(Channel::parse(0).unwrap(), &mut Generation(7)), Err(Error::InvalidHandle));
}
#[test]
fn period_limits_match_integer_clock_conversion() {
    assert_eq!(configuration(0, Units::Microseconds, 1).timing(), Err(Error::InvalidConfiguration));
    assert_eq!(configuration(0x15555554, Units::Microseconds, 1).timing(), Ok(Timing { reload: 0xfffffff0, prescaler: 0 }));
    assert_eq!(configuration(0x15555555, Units::Microseconds, 1).timing(), Ok(Timing { reload: 0x7ffffffe, prescaler: 1 }));
    assert_eq!(configuration(u32::MAX, Units::Microseconds, 1).timing(), Ok(Timing { reload: 0xbfffffff, prescaler: 4 }));
    assert_eq!(configuration(83, Units::Nanoseconds, 1).timing(), Err(Error::InvalidConfiguration));
    assert_eq!(configuration(84, Units::Nanoseconds, 1).timing(), Ok(Timing { reload: 1, prescaler: 0 }));
    assert_eq!(configuration(0x2bb0cf8, Units::Milliseconds, 1).timing(), Err(Error::InvalidConfiguration));
    assert!(Channel::parse(4).is_none());
    assert!(Units::parse(3).is_none());
}
#[derive(Default)]
struct DiagnosticSink(Vec<Diagnostic>);
impl Diagnostics for DiagnosticSink {
    fn command_error(&mut self, diagnostic: Diagnostic) { self.0.push(diagnostic); }
}
#[test]
fn handle_failures_reach_command_diagnostic_consumer() {
    let mut registry = Registry::new();
    let mut generation = Generation(0);
    let channel = Channel::parse(1).unwrap();
    let mut sink = DiagnosticSink::default();
    let handle = report_acquire(registry.acquire(channel, &mut generation), &mut sink).unwrap();
    assert!(sink.0.is_empty());
    assert_eq!(report_acquire(registry.acquire(channel, &mut generation), &mut sink), Err(Error::InvalidHandle));
    assert_eq!(sink.0, [Diagnostic::Acquire]);
    assert_eq!(release(&mut registry, handle, &mut sink), Ok(()));
    assert_eq!(sink.0, [Diagnostic::Acquire]);
    assert_eq!(release(&mut registry, handle, &mut sink), Err(Error::InvalidHandle));
    assert_eq!(sink.0, [Diagnostic::Acquire, Diagnostic::Release]);
}
