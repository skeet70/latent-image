use super::{DriverAction, DriverReply, DriverSemaphore, DriverStatus, RadioDriverRequest,
    SerializedRadioDriver, Synchronization, SynchronizationReply};
use crate::vendor_wireless_power::{RadioTick, RegisterTransfer, SdioReply};

#[test]
fn stopped_driver_releases_global_lock_without_touching_adapter() {
    let mut driver = SerializedRadioDriver::new(0);
    driver.set_stopping(true);
    driver.submit(RadioDriverRequest::RegisterHandshake).unwrap();
    assert_eq!(driver.synchronize(SynchronizationReply::Acquired), DriverReply::Applied);
    assert!(matches!(driver.action(RadioTick(0)), Some(DriverAction::Synchronize(
        Synchronization::Release(DriverSemaphore::Global)))));
    assert_eq!(driver.outcome(), None);
    driver.synchronize(SynchronizationReply::Released);
    assert_eq!(driver.outcome(), Some(DriverStatus::STOPPED));
}

#[test]
fn adapter_lock_timeout_releases_only_global_lock() {
    let mut driver = SerializedRadioDriver::new(0);
    driver.submit(RadioDriverRequest::RegisterHandshake).unwrap();
    driver.synchronize(SynchronizationReply::Acquired);
    assert!(matches!(driver.action(RadioTick(0)), Some(DriverAction::Synchronize(
        Synchronization::Acquire { semaphore: DriverSemaphore::Adapter, timeout_ticks: 2000 }))));
    driver.synchronize(SynchronizationReply::TimedOut);
    assert!(matches!(driver.action(RadioTick(0)), Some(DriverAction::Synchronize(
        Synchronization::Release(DriverSemaphore::Global)))));
    driver.synchronize(SynchronizationReply::Released);
    assert_eq!(driver.outcome(), Some(DriverStatus::FAILURE));
}

#[test]
fn handshake_completion_is_published_after_both_unlocks() {
    let mut driver = SerializedRadioDriver::new(0);
    driver.submit(RadioDriverRequest::RegisterHandshake).unwrap();
    driver.synchronize(SynchronizationReply::Acquired);
    driver.synchronize(SynchronizationReply::Acquired);
    for (now, reply) in [(0, SdioReply::Written), (500, SdioReply::Read(0x0020_0000u32.to_le_bytes())),
        (500, SdioReply::Read([0; 4])), (500, SdioReply::Written)] {
        let Some(DriverAction::Register(transfer)) = driver.action(RadioTick(now)) else { panic!() };
        if now == 0 { assert_eq!(transfer.operation, RegisterTransfer::WriteControl(2)); }
        driver.register_reply(transfer.token, reply, RadioTick(now));
    }
    assert!(matches!(driver.action(RadioTick(500)), Some(DriverAction::Synchronize(
        Synchronization::Release(DriverSemaphore::Adapter)))));
    assert_eq!(driver.outcome(), None);
    driver.synchronize(SynchronizationReply::Released);
    assert_eq!(driver.outcome(), None);
    driver.synchronize(SynchronizationReply::Released);
    assert_eq!(driver.outcome(), Some(DriverStatus::SUCCESS));
}
