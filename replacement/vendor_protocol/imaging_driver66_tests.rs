use super::{Busy, CallerId, Channel, DmaAddress, DmaConfiguration, DmaEnabled, Driver, Error, InputMode, OutputMode, Register, RegisterIo, RowCount, StatusBytes, TransferConfiguration, TransferFlags, TransferFormat, TransferMode, Width};
use core::task::Poll;
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

#[derive(Clone, Default)]
struct Hardware(Rc<RefCell<BTreeMap<u32, u32>>>);
impl Hardware {
    fn store(&self, address: u32, value: u32) { self.0.borrow_mut().insert(address, value); }
    fn load(&self, address: u32) -> u32 { self.0.borrow().get(&address).copied().unwrap_or(0) }
}
impl RegisterIo for Hardware {
    fn read(&mut self, register: Register) -> u32 { self.load(register.address()) }
    fn write(&mut self, register: Register, value: u32) { self.store(register.address(), value); }
}
fn command_configuration() -> DmaConfiguration {
    DmaConfiguration {
        enabled: DmaEnabled::Disabled, input: InputMode::Mode0, output: OutputMode::Mode2,
        rows: RowCount::new(1).unwrap(), width: Width::new(3).unwrap(),
    }
}
fn transfer_configuration() -> TransferConfiguration {
    TransferConfiguration {
        mode: TransferMode::Mode1, format: TransferFormat::Format1,
        buffer: DmaAddress::new(0x1234_5000).unwrap(), offset: DmaAddress::new(0).unwrap(),
        flags: TransferFlags(0),
    }
}

#[test]
fn action78_configuration_reaches_channel_zero_hardware() {
    let hw = Hardware::default();
    let mut driver = Driver::new(hw.clone());
    driver.configure(Channel::Zero, command_configuration());
    assert_eq!(hw.load(0xfff1_0098), 2);
    assert_eq!(hw.load(0xfff1_009c), 2);
    driver.start(Channel::Zero);
    assert_eq!(hw.load(0xfff1_0094), 1);
    driver.cancel(Channel::Zero);
    assert_eq!(hw.load(0xfff1_0094), 0);
}

#[test]
fn second_channel_uses_distinct_command_and_transfer_banks() {
    let hw = Hardware::default();
    let mut driver = Driver::new(hw.clone());
    driver.configure(Channel::One, DmaConfiguration {
        enabled: DmaEnabled::Enabled, input: InputMode::Mode3, output: OutputMode::Mode7,
        rows: RowCount::new(512).unwrap(), width: Width::new(8).unwrap(),
    });
    assert_eq!(hw.load(0xfff1_00b0), 0x0103_0007);
    assert_eq!(hw.load(0xfff1_00b4), 0x01ff_0007);
    let handle = driver.transfer_open(Channel::One).unwrap();
    driver.transfer_configure(handle, transfer_configuration()).unwrap();
    assert_eq!(hw.load(0xfff1_0274), 0x1234_5000);
    assert_eq!(hw.load(0xfff1_0174), 0);
    driver.transfer_start(handle).unwrap();
    assert_eq!(hw.load(0xfff1_0254), 0x0001_0001);
}

#[test]
fn busy_and_six_byte_reply_come_from_hardware() {
    let hw = Hardware::default();
    let mut driver = Driver::new(hw.clone());
    hw.store(0xfff1_00a8, 0xffff_fffe);
    assert_eq!(driver.busy(Channel::Zero), Busy::Idle);
    hw.store(0xfff1_00a8, 3);
    assert_eq!(driver.busy(Channel::Zero), Busy::Active);
    hw.store(0xfff1_00a0, 0x7856_3412);
    hw.store(0xfff1_00a4, 0xdead_bc9a);
    assert_eq!(driver.read6(Channel::Zero), StatusBytes([0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc]));
}

#[test]
fn invalid_boundary_values_are_rejected() {
    assert_eq!(Channel::from_raw(2), Err(Error::InvalidArgument));
    assert_eq!(RowCount::new(0), Err(Error::InvalidArgument));
    assert_eq!(RowCount::new(513), Err(Error::InvalidArgument));
    assert_eq!(Width::new(0), Err(Error::InvalidArgument));
    assert_eq!(Width::new(9), Err(Error::InvalidArgument));
    assert_eq!(DmaAddress::new(3), Err(Error::InvalidArgument));
    assert_eq!(DmaAddress::new(0x4000_0000), Err(Error::InvalidArgument));
}

#[test]
fn lock_contention_and_wrong_release_leave_owner_held() {
    let mut driver = Driver::new(Hardware::default());
    let first = driver.acquire(CallerId(3)).unwrap();
    assert_eq!(driver.acquire(CallerId(6)), Err(Error::LockHeld));
    driver.release(&first).unwrap();
    let second = driver.acquire(CallerId(6)).unwrap();
    assert_eq!(driver.release(&first), Err(Error::WrongLock));
    assert_eq!(driver.acquire(CallerId(3)), Err(Error::LockHeld));
    driver.release(&second).unwrap();
    assert!(driver.acquire(CallerId(3)).is_ok());
}

#[test]
fn transmit_configuration_preserves_unrelated_hardware_bits() {
    let hw = Hardware::default();
    hw.store(0xfff1_0170, 0x88ff_ffff);
    let mut driver = Driver::new(hw.clone());
    let handle = driver.transfer_open(Channel::Zero).unwrap();
    driver.transfer_configure(handle, transfer_configuration()).unwrap();
    assert_eq!(hw.load(0xfff1_0170), 0x889f_dfff);
    assert_eq!(hw.load(0xfff1_0174), 0x1234_5000);
    assert_eq!(hw.load(0xfff1_0178), 0);
}

#[test]
fn stopping_waits_for_physical_quiescence_before_release() {
    let hw = Hardware::default();
    let mut driver = Driver::new(hw.clone());
    let handle = driver.transfer_open(Channel::Zero).unwrap();
    driver.transfer_configure(handle, transfer_configuration()).unwrap();
    driver.transfer_start(handle).unwrap();
    assert_eq!(driver.transfer_start(handle), Err(Error::AlreadyStarted));
    hw.store(0xfff1_0160, 2);
    assert_eq!(driver.transfer_wait(handle), Ok(Poll::Pending));
    assert_eq!(hw.load(0xfff1_0154), 0);
    assert_eq!(driver.transfer_release(handle), Err(Error::AlreadyStarted));
    hw.store(0xfff1_0160, 0);
    assert_eq!(driver.transfer_wait(handle), Ok(Poll::Ready(())));
    driver.transfer_release(handle).unwrap();
    assert_eq!(driver.transfer_release(handle), Err(Error::InvalidHandle));
}

#[test]
fn timeout_and_late_interrupt_cannot_release_live_dma() {
    let hw = Hardware::default();
    let mut driver = Driver::new(hw.clone());
    let handle = driver.transfer_open(Channel::Zero).unwrap();
    driver.transfer_configure(handle, transfer_configuration()).unwrap();
    driver.transfer_start(handle).unwrap();
    hw.store(0xfff1_0160, 1);
    hw.store(0xfff1_00a8, 1);
    assert_eq!(driver.timeout(Channel::Zero, handle), Err(Error::Timeout));
    assert_eq!(hw.load(0xfff1_0094), 0);
    assert_eq!(driver.acknowledge_interrupt(Channel::Zero), Busy::Active);
    assert_eq!(driver.transfer_release(handle), Err(Error::AlreadyStarted));
    assert_eq!(driver.transfer_wait(handle), Ok(Poll::Pending));
    hw.store(0xfff1_0160, 0);
    assert_eq!(driver.transfer_wait(handle), Ok(Poll::Ready(())));
    driver.transfer_release(handle).unwrap();
}

#[test]
fn stale_handle_cannot_stop_new_transfer_after_late_interrupt() {
    let hw = Hardware::default();
    let mut driver = Driver::new(hw.clone());
    let old = driver.transfer_open(Channel::Zero).unwrap();
    driver.transfer_release(old).unwrap();
    assert_eq!(driver.acknowledge_interrupt(Channel::Zero), Busy::Idle);
    let current = driver.transfer_open(Channel::Zero).unwrap();
    driver.transfer_configure(current, transfer_configuration()).unwrap();
    driver.transfer_start(current).unwrap();
    assert_eq!(driver.transfer_wait(old), Err(Error::InvalidHandle));
    assert_eq!(driver.transfer_release(old), Err(Error::InvalidHandle));
    assert_eq!(hw.load(0xfff1_0154), 0x0001_0001);
}

#[test]
fn unstarted_transfer_reports_error_and_can_be_released() {
    let mut driver = Driver::new(Hardware::default());
    let handle = driver.transfer_open(Channel::Zero).unwrap();
    assert_eq!(driver.transfer_wait(handle), Err(Error::NotStarted));
    assert_eq!(driver.transfer_start(handle), Err(Error::InvalidArgument));
    driver.transfer_release(handle).unwrap();
}

#[test]
fn physical_admission_requires_the_imaging_register_domain() {
    assert!(super::ImagingRegion::new(0xfff1_0000, 0x400).is_ok());
    assert_eq!(super::ImagingRegion::new(0xfff1_0000, 0x3ff), Err(Error::InvalidArgument));
    assert_eq!(super::ImagingRegion::new(0xfff6_0000, 0x400), Err(Error::InvalidArgument));
}

#[test]
fn timeout_on_another_channel_has_no_hardware_effect() {
    let hw = Hardware::default();
    let mut driver = Driver::new(hw.clone());
    let handle = driver.transfer_open(Channel::Zero).unwrap();
    driver.transfer_configure(handle, transfer_configuration()).unwrap();
    driver.transfer_start(handle).unwrap();
    hw.store(0xfff1_00ac, 1);
    assert_eq!(driver.timeout(Channel::One, handle), Err(Error::InvalidArgument));
    assert_eq!(hw.load(0xfff1_00ac), 1);
    assert_eq!(hw.load(0xfff1_0154), 0x0001_0001);
}
