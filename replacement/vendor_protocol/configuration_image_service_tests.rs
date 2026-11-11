use super::*;
use xm1_power_lifecycle::capture_task3::{Notification, Operation, TaskQueue};
use xm1_power_lifecycle::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

fn keys() -> CommonOutputKeys {
    CommonOutputKeys {
        state: ConfigurationKey::from_abi(0x81),
        mode: ConfigurationKey::from_abi(0x82),
        minutes: ConfigurationKey::from_abi(0x83),
        seconds: ConfigurationKey::from_abi(0x84),
        active: ConfigurationKey::from_abi(0x85),
    }
}

const MODES: [OutputMode; 7] = [
    OutputMode { mode_54: 11, mode_5c: 22, mode_60: 33 },
    OutputMode { mode_54: 41, mode_5c: 42, mode_60: 43 },
    OutputMode { mode_54: 51, mode_5c: 52, mode_60: 53 },
    OutputMode { mode_54: 61, mode_5c: 62, mode_60: 63 },
    OutputMode { mode_54: 71, mode_5c: 72, mode_60: 73 },
    OutputMode { mode_54: 81, mode_5c: 82, mode_60: 83 },
    OutputMode { mode_54: 91, mode_5c: 92, mode_60: 93 },
];
#[test]
fn common_output_queries_exact_active_value_without_starting_work() {
    for (active, expected) in [(0, 0), (1, 1), (2, 0), (-1, 0)] {
        let mut bytes = [0; 256];
        let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
        let mut runtime = CommonOutputRuntime::from_state(active, 19);
        let defaults = OutputConfiguration::from_abi([0xfeed; 250]);
        let mut output = OutputConfiguration::from_abi([0; 250]);
        assert_eq!(runtime.begin(keys(), &mut configuration, &defaults, &MODES, &mut output), Ok(OutputStep::Completed(ActionCompletion::service_success())));
        assert_eq!(configuration.byte(0x85), Some(expected));
        assert_eq!(runtime.period_seconds(), 19);
        assert_eq!(output.words()[0], 0);
    }
}

#[test]
fn common_output_clamps_mode_and_accepts_exactly_sixty_seconds() {
    let mut bytes = [0; 256];
    bytes[0x81..0x85].copy_from_slice(&[1, 255, 2, 60]);
    let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
    let mut runtime = CommonOutputRuntime::from_state(0, 0);
    let defaults = OutputConfiguration::from_abi([0xfeed; 250]);
    let mut output = OutputConfiguration::from_abi([0; 250]);
    assert_eq!(runtime.begin(keys(), &mut configuration, &defaults, &MODES, &mut output), Ok(OutputStep::SubmitConfiguration));
    assert_eq!(runtime.period_seconds(), 180);
    assert_eq!(output.words()[0], 0x2c);
    assert_eq!(output.words()[0x54 / 4], 11);
    assert_eq!(output.words()[0x58 / 4], 0xfeed);
    assert_eq!(output.words()[0x5c / 4], 22);
    assert_eq!(output.words()[0x60 / 4], 33);
    assert_eq!(runtime.publication(CpuReturn::from_abi(-1)), Ok(OutputStep::BeginOutput));
    assert_eq!(runtime.publication(CpuReturn::SUCCESS), Ok(OutputStep::PublishOutput));
    assert!(!runtime.active());
    assert_eq!(runtime.output_published(), Ok(ActionCompletion::service_success()));
    assert!(runtime.active());
}

#[test]
fn common_output_waits_for_cancel_before_rejecting_begin_failure() {
    let mut bytes = [0; 256];
    bytes[0x81] = 1;
    bytes[0x83] = 1;
    let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
    let mut runtime = CommonOutputRuntime::from_state(0, 0);
    let defaults = OutputConfiguration::from_abi([0; 250]);
    let mut output = OutputConfiguration::from_abi([0; 250]);
    assert_eq!(runtime.begin(keys(), &mut configuration, &defaults, &MODES, &mut output), Ok(OutputStep::SubmitConfiguration));
    assert_eq!(runtime.publication(CpuReturn::SUCCESS), Ok(OutputStep::BeginOutput));
    assert_eq!(runtime.publication(CpuReturn::REJECTED), Ok(OutputStep::CancelOutput));
    assert!(!runtime.active());
    assert_eq!(runtime.publication(CpuReturn::SUCCESS), Ok(OutputStep::Completed(ActionCompletion::service_rejected())));
    assert_eq!(runtime.publication(CpuReturn::SUCCESS), Err(OutputError::UnexpectedPublication));
}

#[test]
fn over_sixty_seconds_cannot_turn_zero_minutes_into_a_start() {
    let mut bytes = [0; 256];
    bytes[0x81] = 1;
    bytes[0x84] = 61;
    let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
    let mut runtime = CommonOutputRuntime::from_state(0, 19);
    let defaults = OutputConfiguration::from_abi([0; 250]);
    let mut output = OutputConfiguration::from_abi([0; 250]);
    assert_eq!(runtime.begin(keys(), &mut configuration, &defaults, &MODES, &mut output), Ok(OutputStep::Completed(ActionCompletion::service_rejected())));
    assert_eq!(runtime.period_seconds(), 19);
    assert_eq!(runtime.publication(CpuReturn::SUCCESS), Err(OutputError::UnexpectedPublication));
}

#[test]
fn configuration_rejection_finishes_without_beginning_or_cancelling_output() {
    let mut bytes = [0; 256];
    bytes[0x81] = 1;
    bytes[0x83] = 1;
    let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
    let mut runtime = CommonOutputRuntime::from_state(1, 0);
    let defaults = OutputConfiguration::from_abi([0; 250]);
    let mut output = OutputConfiguration::from_abi([0; 250]);
    assert_eq!(runtime.begin(keys(), &mut configuration, &defaults, &MODES, &mut output), Ok(OutputStep::SubmitConfiguration));
    assert_eq!(runtime.begin(keys(), &mut configuration, &defaults, &MODES, &mut output), Err(OutputError::Busy));
    assert_eq!(runtime.publication(CpuReturn::REJECTED), Ok(OutputStep::Completed(ActionCompletion::service_rejected())));
    assert!(runtime.active());
    assert_eq!(runtime.publication(CpuReturn::SUCCESS), Err(OutputError::UnexpectedPublication));
}

#[test]
fn other_state_bytes_do_not_change_active_period_or_configuration() {
    for state in [2, 255] {
        let mut bytes = [0; 256];
        bytes[0x81] = state;
        let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
        let mut runtime = CommonOutputRuntime::from_state(1, 19);
        let defaults = OutputConfiguration::from_abi([0; 250]);
        let mut output = OutputConfiguration::from_abi([0xfeed; 250]);
        assert_eq!(runtime.begin(keys(), &mut configuration, &defaults, &MODES, &mut output), Ok(OutputStep::Completed(ActionCompletion::service_success())));
        assert!(runtime.active());
        assert_eq!(runtime.period_seconds(), 19);
        assert_eq!(output.words(), &[0xfeed; 250]);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PostedFrame {
    queue: TaskQueue,
    operation: Operation,
    payload: [u32; 5],
    length: usize,
}

struct QueueEndpoint {
    posted: [Option<PostedFrame>; 4],
    count: usize,
    notifications: [Notification; 3],
    received: usize,
}

impl QueueEndpoint {
    fn new(operations: [u16; 3]) -> Self {
        Self {
            posted: [None; 4], count: 0, received: 0,
            notifications: operations.map(|operation| Notification { receive_status: 0, operation: u32::from(operation), value: 0 }),
        }
    }
}

impl Task3Queues for QueueEndpoint {
    fn post_words(&mut self, queue: TaskQueue, operation: Operation, payload: &[u32]) -> Result<(), i32> {
        let mut words = [0; 5];
        words[..payload.len()].copy_from_slice(payload);
        self.posted[self.count] = Some(PostedFrame { queue, operation, payload: words, length: payload.len() });
        self.count += 1;
        Ok(())
    }

    fn receive_notification(&mut self, _: TaskQueue) -> Notification {
        let notification = self.notifications[self.received];
        self.received += 1;
        notification
    }
}

fn admitted_region(base: u32, bytes: &mut [u8]) -> MemoryRegion<'_> {
    let descriptor = RegionDescriptor::new(GuestAddress::new(base), ByteCount(bytes.len() as u32), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
    MemoryRegion::new(descriptor, bytes).unwrap()
}

#[test]
fn shared_cpu_copies_output_snapshot_and_completes_after_real_notification_classification() {
    let mut snapshot = [0; 1000];
    let mut regions = [admitted_region(0x1000, &mut snapshot)];
    let memory = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
    let mut context = RecordContext::new(memory, QueueEndpoint::new([0x838, 0x816, 0x901]));
    let mut cpu = core::pin::pin!(Task3Runtime::new(GuestAddress::new(0x1000)));
    let mut runtime = ImageServiceRuntime::new(cpu.as_mut(), 0, 0);
    let mut bytes = [0; 256];
    bytes[0x81] = 1;
    bytes[0x83] = 1;
    let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
    let mut defaults = [0xfeed; 250];
    defaults[0x148 / 4] = 0x12345678;
    let defaults = OutputConfiguration::from_abi(defaults);
    let mut output = OutputConfiguration::from_abi([0; 250]);
    assert_eq!(runtime.begin_common(&mut context, keys(), &mut configuration, &defaults, &MODES, &mut output, StackResidue(0x1234)), Ok(Progress::Waiting));
    assert_eq!(context.read_u32(GuestAddress::new(0x1000)), Ok(0x2c));
    assert_eq!(context.read_u32(GuestAddress::new(0x1144)), Ok(0x12345678));
    assert_eq!(output.words()[0x144 / 4], 0x12345678);
    assert_eq!(context.effects.posted[0], Some(PostedFrame { queue: TaskQueue(3), operation: Operation::Configure, payload: [0x1000, 0, 0, 0, 0], length: 2 }));
    assert_eq!(runtime.poll(&mut context), Ok(Progress::Waiting));
    assert!(!runtime.active());
    assert_eq!(context.effects.count, 1);
    assert_eq!(runtime.poll(&mut context), Ok(Progress::Waiting));
    assert_eq!(context.effects.posted[1], Some(PostedFrame { queue: TaskQueue(3), operation: Operation::Begin, payload: [0x9f, 0, 0, 0, 0], length: 4 }));
    assert_eq!(runtime.poll(&mut context), Ok(Progress::Completed(ActionCompletion::service_success())));
    assert_eq!(context.effects.posted[2], Some(PostedFrame { queue: TaskQueue(3), operation: Operation::Output, payload: [0x9f, 0, 0, 0, 0], length: 4 }));
    assert!(runtime.active());
}

#[test]
fn shared_cpu_exposure_clears_guest_flags_and_uses_recording_destination() {
    let mut task = 43u32.to_le_bytes();
    let mut flags = [7, 9];
    let mut regions = [admitted_region(0x0157_bdf0, &mut task), admitted_region(0x0157_dd34, &mut flags)];
    let memory = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
    let mut context = RecordContext::new(memory, QueueEndpoint::new([0x934, 0x90a, 0]));
    let mut cpu = core::pin::pin!(Task3Runtime::new(GuestAddress::new(0x1000)));
    let mut runtime = ImageServiceRuntime::new(cpu.as_mut(), 0, 0);
    assert_eq!(runtime.submit_exposure(&mut context, ExposureRequest { callback: ExposureCallbackAddress(GuestAddress::new(7)), mode: 0x1234, context: 9 }, StackResidue(0xabcdffff)), Ok(()));
    assert_eq!(context.read_u16(GuestAddress::new(0x0157_dd34)), Ok(0));
    assert_eq!(context.effects.posted[0], Some(PostedFrame { queue: TaskQueue(43), operation: Operation::Exposure, payload: [7, 0xabcd1234, 9, 0, 0], length: 3 }));
    assert_eq!(runtime.poll(&mut context), Err(ServiceError::Idle));
    assert_eq!(context.effects.received, 0);
}

#[test]
fn shared_cpu_image_requests_preserve_wire_fields_and_open_configuration_mirror() {
    let mut image_configuration = [0; 1000];
    let mut regions = [admitted_region(0x2000, &mut image_configuration)];
    let memory = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
    let mut context = RecordContext::new(memory, QueueEndpoint::new([0x816; 3]));
    context.write_u32(GuestAddress::new(0x2148), 0x12345678).unwrap();
    let mut cpu = core::pin::pin!(Task3Runtime::new(GuestAddress::new(0x1000)));
    let mut runtime = ImageServiceRuntime::new(cpu.as_mut(), 0, 0);
    let open = ImageRequest::Open(ImageOpenRequest {
        input: ImageStateAddress(0x1000), configuration: ImageStateAddress(0x2000),
        resources: ImageStateAddress(0x3000), option: 4, notification: NotificationCode(0x81),
    });
    assert_eq!(runtime.begin_image(&mut context, open), Ok(Progress::Waiting));
    assert_eq!(context.read_u32(GuestAddress::new(0x2144)), Ok(0x12345678));
    assert_eq!(context.effects.posted[0], Some(PostedFrame { queue: TaskQueue(3), operation: Operation::Open, payload: [0x1000, 0x2000, 0x3000, 0x81, 4], length: 5 }));
    assert_eq!(runtime.begin_image(&mut context, open), Err(ServiceError::Busy));
    assert_eq!(runtime.poll(&mut context), Ok(Progress::Completed(ActionCompletion::service_success())));
    let close = ImageRequest::Close(ImageCloseRequest { operation: 0xe6, option: 0x81, context: 0, notification: NotificationCode(0x81), reserved: 9 });
    assert_eq!(runtime.begin_image(&mut context, close), Ok(Progress::Waiting));
    assert_eq!(context.effects.posted[1], Some(PostedFrame { queue: TaskQueue(3), operation: Operation::Close, payload: [0xe6, 0x81, 0, 0x81, 9], length: 5 }));
    assert_eq!(runtime.poll(&mut context), Ok(Progress::Completed(ActionCompletion::service_success())));
    let output = ImageRequest::Output(ImageOutputRequest { operation: 0x9f, option: 7, notification: NotificationCode(0x81), context: 3 });
    assert_eq!(runtime.begin_image(&mut context, output), Ok(Progress::Waiting));
    assert_eq!(context.effects.posted[2], Some(PostedFrame { queue: TaskQueue(3), operation: Operation::Output, payload: [0x9f, 7, 0x81, 3, 0], length: 4 }));
    assert_eq!(runtime.poll(&mut context), Ok(Progress::Completed(ActionCompletion::service_success())));
}
