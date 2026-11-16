use super::*;
use std::{boxed::Box, cell::RefCell, rc::Rc, vec, vec::Vec};

#[derive(Clone, Copy, Default)]
enum Controller { #[default] Complete, Error(i32), Pending }
#[derive(Default)]
struct Device {
    tick: u64,
    driver_held: bool,
    channel_held: bool,
    active: bool,
    controller: Controller,
    stop_error: Option<i32>,
    close_error: Option<i32>,
    unlock_error: Option<i32>,
    classify_error: Option<i32>,
    consumer_error: Option<i32>,
    drain_pending: bool,
    bad: Vec<u32>,
    dropped_buffers: usize,
    dropped_leases: usize,
    destination: Option<(*mut u8, usize)>,
}
struct Lease(Rc<RefCell<Device>>);
impl Drop for Lease {
    fn drop(&mut self) { self.0.borrow_mut().dropped_leases += 1; }
}
struct Buffer { bytes: Box<[u8]>, device: Rc<RefCell<Device>> }
impl AsMut<[u8]> for Buffer {
    fn as_mut(&mut self) -> &mut [u8] { &mut self.bytes }
}
unsafe impl DmaDestination for Buffer {}
impl Drop for Buffer {
    fn drop(&mut self) { self.device.borrow_mut().dropped_buffers += 1; }
}
struct Ports(Rc<RefCell<Device>>);
fn status(code: i32) -> LowerStatus { LowerStatus::new(code).unwrap() }
fn fault(code: Option<i32>) -> Result<(), LowerStatus> { code.map_or(Ok(()), |code| Err(status(code))) }
unsafe impl ReadPorts for Ports {
    type Driver = Lease;
    type Channel = Lease;
    fn now(&mut self) -> Instant {
        let mut device = self.0.borrow_mut();
        let tick = device.tick;
        device.tick += 1;
        Instant::new(tick)
    }
    fn try_driver(&mut self) -> Poll<Result<Lease, LowerStatus>> {
        let mut device = self.0.borrow_mut();
        if device.driver_held { return Poll::Pending; }
        device.driver_held = true;
        Poll::Ready(Ok(Lease(self.0.clone())))
    }
    fn try_channel(&mut self, _: &mut Lease) -> Poll<Result<Lease, LowerStatus>> {
        let mut device = self.0.borrow_mut();
        if device.channel_held { return Poll::Pending; }
        device.channel_held = true;
        Poll::Ready(Ok(Lease(self.0.clone())))
    }
    fn classify(&mut self, block: PhysicalBlock) -> Result<BlockDisposition, LowerStatus> {
        let device = self.0.borrow();
        fault(device.classify_error)?;
        Ok(if device.bad.contains(&block.index()) { BlockDisposition::Bad } else { BlockDisposition::Usable })
    }
    fn start(&mut self, _: &mut Lease, block: PhysicalBlock, destination: &mut [u8]) -> Result<(), LowerStatus> {
        let mut device = self.0.borrow_mut();
        assert!(device.driver_held && device.channel_held && !device.active);
        assert_eq!(destination.len(), BLOCK_BYTES as usize);
        destination.fill(block.index() as u8 ^ 0xa5);
        device.destination = Some((destination.as_mut_ptr(), destination.len()));
        device.active = true;
        Ok(())
    }
    fn poll_controller(&mut self, _: &mut Lease) -> Poll<Result<(), LowerStatus>> {
        match self.0.borrow().controller {
            Controller::Complete => Poll::Ready(Ok(())),
            Controller::Error(code) => Poll::Ready(Err(status(code))),
            Controller::Pending => Poll::Pending,
        }
    }
    fn request_stop(&mut self, _: &mut Lease) -> Result<(), LowerStatus> { fault(self.0.borrow().stop_error) }
    fn poll_quiescent(&mut self, _: &mut Lease) -> Poll<Result<(), LowerStatus>> {
        let mut device = self.0.borrow_mut();
        if device.drain_pending { return Poll::Pending; }
        device.active = false;
        device.destination = None;
        Poll::Ready(Ok(()))
    }
    fn finish_transfer(&mut self, _: &mut Lease) -> Result<(), LowerStatus> {
        assert!(!self.0.borrow().active);
        Ok(())
    }
    fn close_channel(&mut self, _: &mut Lease) -> Result<(), LowerStatus> {
        let mut device = self.0.borrow_mut();
        assert!(!device.active);
        fault(device.close_error)?;
        device.channel_held = false;
        Ok(())
    }
    fn unlock_driver(&mut self, _: &mut Lease) -> Result<(), LowerStatus> {
        let mut device = self.0.borrow_mut();
        assert!(!device.active && !device.channel_held);
        fault(device.unlock_error)?;
        device.driver_held = false;
        Ok(())
    }
    fn progress(&mut self, _: ByteCount) -> Result<(), LowerStatus> {
        let device = self.0.borrow();
        assert!(!device.active);
        fault(device.consumer_error)
    }
}
fn fixture() -> (Rc<RefCell<Device>>, DmaReadOwner<Ports, Buffer>) {
    let device = Rc::new(RefCell::new(Device::default()));
    let owner = DmaReadOwner::new(Ports(device.clone()));
    (device, owner)
}
fn buffer(device: &Rc<RefCell<Device>>, blocks: usize) -> Buffer {
    Buffer { bytes: vec![0; blocks * BLOCK_BYTES as usize].into_boxed_slice(), device: device.clone() }
}
fn plan(blocks: u32) -> ReadPlan {
    ReadPlan::new(PhysicalBlock::new(0).unwrap(), BlockCount::new(blocks).unwrap(), SkipBudget::Unlimited).unwrap()
}
fn deadline(tick: u64) -> Deadline { Deadline::new(Instant::new(tick)).unwrap() }

#[test]
fn successful_table_read_returns_actual_bytes_and_skips_without_advancing_destination() {
    let (device, mut owner) = fixture();
    device.borrow_mut().bad.push(1);
    match owner.read(plan(3), buffer(&device, 3), deadline(100)) {
        ReadOutcome::Success { destination, report } => {
            assert_eq!(report.bytes.get(), 3 * BLOCK_BYTES as usize);
            assert_eq!(report.bad_blocks.get(), 1);
            assert!(destination.bytes[..BLOCK_BYTES as usize].iter().all(|byte| *byte == 0xa5));
            assert!(destination.bytes[BLOCK_BYTES as usize..2 * BLOCK_BYTES as usize].iter().all(|byte| *byte == 0xa7));
            assert!(destination.bytes[2 * BLOCK_BYTES as usize..].iter().all(|byte| *byte == 0xa6));
        }
        other => panic!("unexpected outcome {}", outcome_name(&other)),
    }
    assert_eq!(owner.availability(), Availability::Available);
    assert!(!device.borrow().driver_held && !device.borrow().channel_held);
}
fn outcome_name<B>(outcome: &ReadOutcome<B>) -> &'static str {
    match outcome {
        ReadOutcome::Success { .. } => "success",
        ReadOutcome::ReleasedFailure { .. } => "released failure",
        ReadOutcome::HeldQuiescent { .. } => "held quiescent",
        ReadOutcome::UnprovedDma { .. } => "unproved DMA",
    }
}
#[test]
fn unknown_nonzero_is_released_failure_not_success() {
    let (device, mut owner) = fixture();
    device.borrow_mut().controller = Controller::Error(0x15);
    match owner.read(plan(1), buffer(&device, 1), deadline(100)) {
        ReadOutcome::ReleasedFailure { failure, .. } => assert_eq!(failure, Failure::Primary { primary: PrimaryError::Lower(status(0x15)), cleanup: None }),
        other => panic!("unexpected outcome {}", outcome_name(&other)),
    }
    assert!(!device.borrow().active && !device.borrow().driver_held && !device.borrow().channel_held);
}
#[test]
fn primary_four_and_stop_fifteen_remain_distinct_and_pin_resources_after_drop() {
    let (device, mut owner) = fixture();
    device.borrow_mut().controller = Controller::Error(4);
    device.borrow_mut().stop_error = Some(0x15);
    match owner.read(plan(1), buffer(&device, 1), deadline(100)) {
        ReadOutcome::UnprovedDma { failure, .. } => assert_eq!(failure, Failure::Primary { primary: PrimaryError::Lower(status(4)), cleanup: Some(CleanupError::Lower { phase: Phase::Stop, status: status(0x15) }) }),
        other => panic!("unexpected outcome {}", outcome_name(&other)),
    }
    drop(owner);
    let device = device.borrow();
    assert!(device.active && device.channel_held && device.driver_held);
    assert_eq!(device.dropped_buffers, 0);
    assert_eq!(device.dropped_leases, 0);
}
#[test]
fn read_expiry_does_not_create_a_stop_budget() {
    let (device, mut owner) = fixture();
    device.borrow_mut().controller = Controller::Pending;
    match owner.read(plan(1), buffer(&device, 1), deadline(12)) {
        ReadOutcome::UnprovedDma { failure, .. } => assert_eq!(failure, Failure::Primary { primary: PrimaryError::Deadline(Phase::Controller), cleanup: Some(CleanupError::Deadline(Phase::Stop)) }),
        other => panic!("unexpected outcome {}", outcome_name(&other)),
    }
    assert!(device.borrow().active && device.borrow().driver_held && device.borrow().channel_held);
}
#[test]
fn pending_stop_expiry_refuses_new_request_and_preserves_backing_after_owner_drop() {
    let (device, mut owner) = fixture();
    device.borrow_mut().drain_pending = true;
    match owner.read(plan(1), buffer(&device, 1), deadline(12)) {
        ReadOutcome::UnprovedDma { failure, .. } => assert_eq!(failure, Failure::Cleanup(CleanupError::Deadline(Phase::Quiescence))),
        other => panic!("unexpected outcome {}", outcome_name(&other)),
    }
    assert_eq!(owner.availability(), Availability::Retained);
    let held_pointer = device.borrow().destination.unwrap();
    match owner.read(plan(1), buffer(&device, 1), deadline(100)) {
        ReadOutcome::ReleasedFailure { destination, failure, .. } => {
            assert_eq!(failure, Failure::Primary { primary: PrimaryError::Admission(AdmissionError::Retained), cleanup: None });
            assert!(destination.bytes.iter().all(|byte| *byte == 0));
            drop(destination);
        }
        other => panic!("unexpected outcome {}", outcome_name(&other)),
    }
    drop(owner);
    assert_eq!(device.borrow().destination, Some(held_pointer));
    assert_eq!(device.borrow().dropped_buffers, 1);
    assert_eq!(device.borrow().dropped_leases, 0);
    assert!(device.borrow().active && device.borrow().channel_held && device.borrow().driver_held);
    unsafe { held_pointer.0.write(0x5a); assert_eq!(held_pointer.0.read(), 0x5a); }
}
#[test]
fn failure_before_dma_returns_destination_and_releases_idle_resources() {
    let (device, mut owner) = fixture();
    device.borrow_mut().classify_error = Some(15);
    match owner.read(plan(1), buffer(&device, 1), deadline(100)) {
        ReadOutcome::ReleasedFailure { destination, failure, .. } => {
            assert_eq!(failure, Failure::Primary { primary: PrimaryError::Lower(status(15)), cleanup: None });
            assert!(destination.bytes.iter().all(|byte| *byte == 0));
        }
        other => panic!("unexpected outcome {}", outcome_name(&other)),
    }
    assert!(device.borrow().destination.is_none());
    assert!(!device.borrow().driver_held && !device.borrow().channel_held);
}
#[test]
fn failed_channel_release_retains_both_leases_and_failed_driver_release_retains_driver() {
    for fail_channel in [true, false] {
        let (device, mut owner) = fixture();
        if fail_channel { device.borrow_mut().close_error = Some(15); }
        else { device.borrow_mut().unlock_error = Some(15); }
        match owner.read(plan(1), buffer(&device, 1), deadline(100)) {
            ReadOutcome::HeldQuiescent { failure, .. } => assert_eq!(failure, Failure::Cleanup(CleanupError::Lower { phase: if fail_channel { Phase::ChannelRelease } else { Phase::DriverRelease }, status: status(15) })),
            other => panic!("unexpected outcome {}", outcome_name(&other)),
        }
        drop(owner);
        assert!(!device.borrow().active);
        assert!(device.borrow().driver_held);
        assert_eq!(device.borrow().channel_held, fail_channel);
        assert_eq!(device.borrow().dropped_buffers, 0);
    }
}
#[test]
fn recognized_statuses_keep_mapping_and_continuation() {
    for (lower, aggregate, completed) in [(4, 6, 0), (3, 7, 0), (7, 3, 2), (10, 4, 2), (15, 15, 0), (0x15, 0x15, 0), (-255, -255, 0)] {
        let (device, mut owner) = fixture();
        device.borrow_mut().controller = Controller::Error(lower);
        match owner.read(plan(2), buffer(&device, 2), deadline(100)) {
            ReadOutcome::ReleasedFailure { failure: Failure::Primary { primary: PrimaryError::Lower(status), cleanup: None }, report, .. } => {
                assert_eq!(status.code(), lower);
                assert_eq!(status.aggregate_code(), aggregate);
                assert_eq!(report.bytes.get(), completed * BLOCK_BYTES as usize);
            }
            other => panic!("unexpected outcome {}", outcome_name(&other)),
        }
    }
}
#[test]
fn mandatory_deadline_rejects_infinite_and_expiry_before_acquire_is_safe() {
    assert!(Deadline::new(Instant::new(u64::MAX)).is_none());
    let (device, mut owner) = fixture();
    match owner.read(plan(1), buffer(&device, 1), deadline(0)) {
        ReadOutcome::ReleasedFailure { failure, .. } => assert_eq!(failure, Failure::Primary { primary: PrimaryError::Deadline(Phase::DriverAcquire), cleanup: None }),
        other => panic!("unexpected outcome {}", outcome_name(&other)),
    }
    assert!(!device.borrow().active && !device.borrow().channel_held && !device.borrow().driver_held);
}

#[test]
fn rejected_prefix_stops_aggregate_and_returns_quiescent_backing_without_success() {
    let (device, mut owner) = fixture();
    device.borrow_mut().consumer_error = Some(0x15);
    match owner.read(plan(2), buffer(&device, 2), deadline(100)) {
        ReadOutcome::ReleasedFailure { destination, report, failure } => {
            assert_eq!(failure, Failure::Primary { primary: PrimaryError::Lower(status(0x15)), cleanup: None });
            assert_eq!(report.bytes.get(), BLOCK_BYTES as usize);
            assert!(destination.bytes[..BLOCK_BYTES as usize].iter().all(|byte| *byte == 0xa5));
            assert!(destination.bytes[BLOCK_BYTES as usize..].iter().all(|byte| *byte == 0));
        }
        other => panic!("unexpected outcome {}", outcome_name(&other)),
    }
    assert!(!device.borrow().active && !device.borrow().channel_held && !device.borrow().driver_held);
    assert_eq!(owner.availability(), Availability::Available);
}
