use super::*;

#[test]
fn enabled_configuration_preserves_the_caller_tail_and_waits_for_start() {
    let mut runtime = RecordRuntime::new();
    let Progress::Submit(command) = runtime.begin(Request::Configure(ConfigurationParameters::EnabledCallerTail(0x31415926))).unwrap() else { panic!() };
    assert_eq!(command.operation(), 0x17);
    assert_eq!(command.length(), 10);
    assert_eq!(command.arguments(), [5, 5, 0, 0, 0, 0, 0, 0, 0x31415926]);
    assert_eq!(runtime.publication(CompletionStatus(-7)), Ok(Progress::Submit(Command::Start)));
    assert_eq!(runtime.begin(Request::Restart), Err(Error::Busy));
    assert_eq!(runtime.publication(CompletionStatus(-9)), Ok(Progress::Completed(ActionCompletion::service_success())));
}

#[test]
fn disabled_configuration_preserves_all_seven_low_memory_words() {
    let mut runtime = RecordRuntime::new();
    let source = ConfigurationParameters::DisabledAddressZero([11, 22, 33, 44, 55, 66, 77]);
    let Progress::Submit(command) = runtime.begin(Request::Configure(source)).unwrap() else { panic!() };
    assert_eq!(command.operation(), 0x17);
    assert_eq!(command.arguments(), [15, 0, 11, 22, 33, 44, 55, 66, 77]);
    assert_eq!(runtime.publication(CompletionStatus(25)), Ok(Progress::Submit(Command::Stop)));
    assert_eq!(runtime.publication(CompletionStatus(0)), Ok(Progress::Completed(ActionCompletion::service_success())));
    assert_eq!(runtime.publication(CompletionStatus(0)), Err(Error::NotWaiting));
}

#[test]
fn restart_requires_resume_status_zero_before_start_and_final_publication() {
    let mut runtime = RecordRuntime::new();
    assert_eq!(runtime.begin(Request::Restart), Ok(Progress::Submit(Command::Suspend)));
    assert_eq!(runtime.publication(CompletionStatus(12)), Ok(Progress::Submit(Command::Resume)));
    for status in [-1, 1, 254] {
        assert_eq!(runtime.publication(CompletionStatus(status)), Ok(Progress::Waiting));
        assert_eq!(runtime.begin(Request::Restart), Err(Error::Busy));
    }
    assert_eq!(runtime.publication(CompletionStatus(0)), Ok(Progress::Submit(Command::Start)));
    assert_eq!(runtime.publication(CompletionStatus(2)), Ok(Progress::Completed(ActionCompletion::service_success())));
    assert_eq!(runtime.begin(Request::Restart), Ok(Progress::Submit(Command::Suspend)));
}

#[test]
fn command_packets_preserve_the_original_operation_parameters() {
    for (command, operation, length, arguments) in [
        (Command::Start, 2, 7, [2, 0, 0, 0, 0, 0, 0, 0, 0]),
        (Command::Stop, 3, 6, [0, 2, 0, 0, 0, 0, 0, 0, 0]),
        (Command::Suspend, 3, 6, [3, 2, 0, 0, 0, 0, 0, 0, 0]),
        (Command::Resume, 0x29, 2, [1, 0, 0, 0, 0, 0, 0, 0, 0]),
    ] {
        assert_eq!(command.operation(), operation);
        assert_eq!(command.length(), length);
        assert_eq!(command.arguments(), arguments);
    }
}

#[test]
fn selector_reads_key_81_and_rejects_the_wrong_source_generation() {
    for enabled in [0, 1, 255] {
        let mut bytes = [0; 256];
        bytes[0x81] = enabled;
        let configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
        let matching = if enabled == 0 { ConfigurationParameters::DisabledAddressZero([1; 7]) } else { ConfigurationParameters::EnabledCallerTail(42) };
        let wrong = if enabled == 0 { ConfigurationParameters::EnabledCallerTail(42) } else { ConfigurationParameters::DisabledAddressZero([1; 7]) };
        assert_eq!(Request::from_queue_action(96, &configuration, matching), Ok(Some(Request::Configure(matching))));
        assert_eq!(Request::from_queue_action(96, &configuration, wrong), Err(BackendError::InvalidCommand));
        assert_eq!(Request::from_queue_action(103, &configuration, matching), Ok(Some(Request::Restart)));
        assert_eq!(Request::from_queue_action(95, &configuration, matching), Ok(None));
    }
}

#[test]
fn configuration_sources_read_the_admitted_address_space_without_null_substitution() {
    use xm1_power_lifecycle::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};
    let mut low = [0u8; 28];
    for (index, byte) in low.iter_mut().enumerate() { *byte = index as u8 + 1; }
    let mut tail = [0x12, 0x34, 0x56, 0x78];
    let mut regions = [
        MemoryRegion::new(RegionDescriptor::new(GuestAddress::new(0), ByteCount(28), RegionDomain::Shared, RegionPermission::ReadOnly).unwrap(), &mut low).unwrap(),
        MemoryRegion::new(RegionDescriptor::new(GuestAddress::new(0x1000), ByteCount(4), RegionDomain::Shared, RegionPermission::ReadOnly).unwrap(), &mut tail).unwrap(),
    ];
    let mut memory = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
    let caller = ConfigurationCallerFrame::new(GuestAddress::new(0x100c));
    assert_eq!(ConfigurationParameters::read(true, &mut memory, caller), Ok(ConfigurationParameters::EnabledCallerTail(0x78563412)));
    assert_eq!(ConfigurationParameters::read(false, &mut memory, caller), Ok(ConfigurationParameters::DisabledAddressZero([0x04030201, 0x08070605, 0x0c0b0a09, 0x100f0e0d, 0x14131211, 0x18171615, 0x1c1b1a19])));
    let missing = ConfigurationCallerFrame::new(GuestAddress::new(0x200c));
    assert_eq!(ConfigurationParameters::read(true, &mut memory, missing), Err(MemoryError::Unmapped(GuestAddress::new(0x2000))));
}
