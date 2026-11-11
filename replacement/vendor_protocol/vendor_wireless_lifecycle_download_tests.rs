use super::{FirmwareAddress, FirmwareDownload};
use crate::sd_controller::{Completion, PhysicalResult, VendorCode};
use crate::vendor_wireless_lifecycle::DriverStatus;
use crate::vendor_wireless_power::RadioTick;

fn physical() -> Completion {
    Completion { code: VendorCode::Success, physical: PhysicalResult::Completed,
        transferred: 4, status: 0, response: 0 }
}

fn exchange(download: &mut FirmwareDownload<'_>, time: u32, argument: u32, value: u32) {
    let mut register = [0; 4];
    let mut scratch = [0; 2560];
    let request = download.request(RadioTick(time), &mut register, &mut scratch).unwrap();
    assert_eq!(request.argument.wire(), argument);
    drop(request);
    assert!(download.receive(physical(), value.to_le_bytes(), RadioTick(time), false));
}

#[test]
fn start_readiness_retry_does_not_restart_download() {
    let mut download = FirmwareDownload::new(&[1], FirmwareAddress(0x1000)).unwrap();
    exchange(&mut download, 0, 0x9001_1804, 0);
    exchange(&mut download, 0, 0x9001_1804, 0);
    exchange(&mut download, 120, 0x1001_1804, 0x800);
    exchange(&mut download, 120, 0x9001_2004, 0);
    exchange(&mut download, 130, 0x9001_1804, 0);
    exchange(&mut download, 130, 0x1001_1804, 0);
    exchange(&mut download, 140, 0x1001_1804, 0x2000);
    exchange(&mut download, 140, 0x9001_1004, 0);
    exchange(&mut download, 140, 0x9001_0804, 0);
    exchange(&mut download, 140, 0x1001_1004, 0x100);
    exchange(&mut download, 140, 0x9001_0008, 0);
    exchange(&mut download, 140, 0x1001_1804, 0x8000);
    exchange(&mut download, 140, 0x9001_1004, 0);
    assert_eq!(download.outcome(), Some(DriverStatus::SUCCESS));
}

#[test]
fn cancellation_during_chunk_read_still_transfers_chunk_before_failure() {
    let mut download = FirmwareDownload::new(&[1], FirmwareAddress(0x1000)).unwrap();
    for (time, argument, value) in [(0, 0x9001_1804, 0), (0, 0x9001_1804, 0),
        (120, 0x1001_1804, 0x800), (120, 0x9001_2004, 0), (130, 0x9001_1804, 0),
        (130, 0x1001_1804, 0x2000), (130, 0x9001_1004, 0), (130, 0x9001_0804, 0)] {
        exchange(&mut download, time, argument, value);
    }
    let mut register = [0; 4];
    let mut scratch = [0; 2560];
    drop(download.request(RadioTick(130), &mut register, &mut scratch).unwrap());
    download.receive(physical(), [0; 4], RadioTick(130), true);
    assert_eq!(download.outcome(), None);
    exchange(&mut download, 130, 0x9001_0008, 0);
    assert_eq!(download.outcome(), Some(DriverStatus::FAILURE));
}
