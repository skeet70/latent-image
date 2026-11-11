use super::{AttachmentError, AttachmentStep, RadioSdioAttachment};
use crate::sd_controller::{ClockPower, ClockRegistration, ControlError, ControlPlane, HalfRegister,
    Host, InterruptRouter, Mmio, TransferUnit, VendorCode, WordRegister};

struct Hardware { halves: [u16; 32], installed: bool, clock_gate: bool }
impl Hardware { fn new() -> Self { Self { halves: [0; 32], installed: false, clock_gate: false } } }
impl Mmio for Hardware {
    fn read_half(&mut self, register: HalfRegister) -> u16 {
        let value = self.halves[register as usize / 2];
        if register == HalfRegister::Clock { value | 2 } else { value }
    }
    fn write_half(&mut self, register: HalfRegister, value: u16) { self.halves[register as usize / 2] = value; }
    fn read_word(&mut self, _: WordRegister) -> u32 { 0 }
    fn write_word(&mut self, _: WordRegister, _: u32) {}
    fn response_half(&mut self, _: u8) -> u16 { 0 }
}
impl ClockPower for Hardware {
    fn unregister(&mut self, _: ClockRegistration) -> bool { true }
    fn set_gate(&mut self, enabled: bool) { self.clock_gate = enabled; }
    fn register(&mut self) -> Option<ClockRegistration> { Some(ClockRegistration(1)) }
    fn select_source(&mut self, _: bool) {}
}
impl InterruptRouter for Hardware {
    fn remove(&mut self) -> Result<(), ControlError> { self.installed = false; Ok(()) }
    fn install(&mut self) -> Result<(), ControlError> { self.installed = true; Ok(()) }
}

#[test]
fn attachment_enables_function_bus_and_512_byte_blocks_before_fast_clock() {
    let mut control = ControlPlane::new();
    let mut host = Host::new(TransferUnit::BLOCK_512);
    let mut registers = Hardware::new();
    let mut power = Hardware::new();
    let mut irq = Hardware::new();
    let mut attachment = RadioSdioAttachment::new();
    for (argument, response) in [(0x400, 0x40), (0x8000_0442, 0), (0xe00, 0x80),
        (0x8000_0e82, 0), (0x8002_2000, 0), (0x8002_2202, 0)] {
        for _ in 0..3 { attachment.advance(&mut control, &mut host, &mut registers, &mut power, &mut irq).unwrap(); }
        let request = attachment.request().unwrap();
        assert_eq!(request.argument.wire(), argument);
        attachment.receive(VendorCode::Success, response).unwrap();
    }
    for _ in 0..3 { attachment.advance(&mut control, &mut host, &mut registers, &mut power, &mut irq).unwrap(); }
    assert!(attachment.finished());
    assert_eq!(registers.halves[HalfRegister::Length as usize / 2] & 0xfff, 512);
    assert_eq!(registers.halves[HalfRegister::Config as usize / 2] & 2, 2);
    assert!(power.clock_gate);
    assert!(!irq.installed);
}

#[test]
fn first_function_read_failure_is_not_a_halt_but_write_failure_is() {
    let mut control = ControlPlane::new();
    let mut host = Host::new(TransferUnit::BLOCK_512);
    let mut registers = Hardware::new();
    let mut power = Hardware::new();
    let mut irq = Hardware::new();
    let mut attachment = RadioSdioAttachment::new();
    for _ in 0..3 { attachment.advance(&mut control, &mut host, &mut registers, &mut power, &mut irq).unwrap(); }
    attachment.request().unwrap();
    assert_eq!(attachment.receive(VendorCode::Timeout, 0x40), Ok(true));
    assert_eq!(attachment.request().unwrap().argument.wire(), 0x8000_0442);
    assert_eq!(attachment.receive(VendorCode::Timeout, 0), Err(AttachmentError::Transfer {
        step: AttachmentStep::EnableWrite, code: VendorCode::Timeout }));
    assert!(attachment.request().is_none());
    assert!(!attachment.finished());
}
