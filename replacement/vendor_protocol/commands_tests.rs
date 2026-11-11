use crate::backend::{ConfigImages, DebugMemory};
use crate::backend_adapter::{CommandBackend, ConfigurationRule, DebugRing, RulePermission};
use crate::commands::*;

fn with_backend(gate: bool, run: impl FnOnce(&mut CommandBackend<'_>)) {
    let first = [0x5a; 512];
    let mut encoded = [0u8; 0x4000];
    encoded[0xf7] = u8::from(gate);
    encoded[0x15c..0x161].copy_from_slice(b"X-M1\0");
    encoded[0x204] = 1;
    encoded[0x205] = 0x50;
    encoded[0xc6] = 1;
    let mut memory = [0x91; 64];
    let ring_bytes = [0x42; 0x2000];
    let mut rules = [ConfigurationRule { selector: 0, permission: RulePermission::Reject }; 144];
    rules[0] = ConfigurationRule { selector: 7, permission: RulePermission::Dispatch };
    let config = ConfigImages::adopt(&first, &[], &[], &mut encoded, &[]);
    let memory = DebugMemory::adopt(0x1000, &mut memory).unwrap();
    let ring = DebugRing::adopt(&ring_bytes, 0, 3, false, 0).unwrap();
    let mut backend = CommandBackend::new(config, memory, ring, &rules).unwrap();
    run(&mut backend);
}

fn word(bytes: &mut [u8], at: usize, value: u32) { bytes[at..at + 4].copy_from_slice(&value.to_le_bytes()); }
fn read_word(bytes: &[u8], at: usize) -> u32 { u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) }
fn packet(opcode: Command) -> [u8; 512] { let mut request = [0; 512]; word(&mut request, 0, opcode as u32); request }

fn service_controller(state: ControllerState) -> crate::controller::QueueController {
    crate::controller::QueueController::new(state, crate::controller::CompletionHandle(0), crate::controller::RequestHandle(3))
}

fn completed(kind: u32, status: u32) -> VendorCompletion {
    let CompletionMessage::Completed(completion) = queue_message(0x3408, kind, status) else { panic!() };
    completion
}

fn queue_message(opcode: u16, kind: u32, status: u32) -> CompletionMessage {
    let mut payload = [0; 24];
    word(&mut payload, 0, kind);
    word(&mut payload, 4, status);
    CompletionMessage::parse(opcode, &payload).unwrap()
}

#[test]
fn product_reply_expands_cached_identity_and_zeroes_padding() {
    with_backend(false, |backend| {
        let mut commands = Commands::new();
        let mut request = packet(Command::DeviceInfo);
        word(&mut request, 16, 4);
        let mut reply = [0xff; 512];
        let result = commands.dispatch(&mut request, 32, &mut reply, &mut [], backend).unwrap();
        assert_eq!(result.reply_length, Some(512));
        assert!(result.ready);
        assert_eq!(&reply[16..26], b"X\0-\0M\01\0\0\0");
        assert!(reply[26..].iter().all(|byte| *byte == 0));
        assert!(backend.take_configuration_event().is_none());
        assert!(backend.take_application_event().is_none());
    });
}

#[test]
fn short_request_precedes_continuation_and_unknown_command_errors() {
    with_backend(false, |backend| {
        let mut commands = Commands::new();
        let mut request = packet(Command::Continuation);
        let mut reply = [0; 512];
        let short = commands.dispatch(&mut request, 31, &mut reply, &mut [], backend).unwrap();
        assert!(!short.continuation);
        assert_eq!((read_word(&reply, 4), read_word(&reply, 8)), (9, 1));
        let continuation = commands.dispatch(&mut request, 32, &mut reply, &mut [], backend).unwrap();
        assert!(continuation.continuation);
        assert_eq!(continuation.reply_length, None);
        word(&mut request, 0, 0x123456);
        commands.dispatch(&mut request, 32, &mut reply, &mut [], backend).unwrap();
        assert_eq!((read_word(&reply, 4), read_word(&reply, 8)), (0xffff, 0));
        assert_eq!(commands.dispatch(&mut request, 513, &mut reply, &mut [], backend), Err(CommandError::RequestTooLarge));
    });
}

#[test]
fn simple_commands_publish_byte_rooted_lengths_and_speed() {
    with_backend(false, |backend| {
        let mut commands = Commands::new();
        let mut reply = [0; 512];
        let mut request = packet(Command::Acknowledge);
        assert_eq!(commands.dispatch(&mut request, 32, &mut reply, &mut [], backend).unwrap().reply_length, Some(32));
        request = packet(Command::UsbSpeed);
        commands.usb_status = UsbLinkStatus::Bit14Set;
        commands.dispatch(&mut request, 32, &mut reply, &mut [], backend).unwrap();
        assert_eq!(reply[20], 2);
        commands.usb_status = UsbLinkStatus::Bit14Clear;
        commands.dispatch(&mut request, 32, &mut reply, &mut [], backend).unwrap();
        assert_eq!(reply[20], 1);
        request = packet(Command::Capabilities);
        let result = commands.dispatch(&mut request, 32, &mut reply, &mut [], backend).unwrap();
        assert_eq!(result.transfer, TransferEffect::Download { region: PayloadRegion::Capabilities, length: ByteCount(48) });
        assert_eq!(read_word(&reply, 20), 48);
        assert_eq!(&capabilities()[16..24], &[1, 0, 0x55, 0, 0xe8, 3, 0, 0]);
        assert!(backend.take_configuration_event().is_none());
    });
}

#[test]
fn configuration_cached_write_is_visible_even_when_reply_reports_failure() {
    with_backend(false, |backend| {
        let mut commands = Commands::new();
        let mut request = packet(Command::Configuration);
        word(&mut request, 16, 3);
        word(&mut request, 28, 0x90);
        word(&mut request, 32, 1);
        request[36] = 0x72;
        let mut reply = [0; 512];
        commands.dispatch(&mut request, 37, &mut reply, &mut [], backend).unwrap();
        assert_eq!(backend.identity_byte(0x90), Some(0x72));
        assert_eq!((read_word(&reply, 4), read_word(&reply, 8)), (1, 0xffff));
        assert!(backend.take_configuration_event().is_none());
        word(&mut request, 16, 2);
        commands.dispatch(&mut request, 36, &mut reply, &mut [], backend).unwrap();
        assert_eq!(reply[36], 0x5a);
    });
}

#[test]
fn admitted_configuration_write_waits_for_message_completion() {
    with_backend(false, |backend| {
        let mut commands = Commands::new();
        let mut request = packet(Command::Configuration);
        word(&mut request, 16, 3);
        word(&mut request, 20, 1);
        word(&mut request, 28, 0x80);
        word(&mut request, 32, 1);
        request[36] = 7;
        let mut reply = [0; 512];
        let result = commands.dispatch(&mut request, 37, &mut reply, &mut [], backend).unwrap();
        assert!(!result.ready);
        let event = backend.take_configuration_event().unwrap();
        assert_eq!((event.destination.get(), event.selector), (0xff, crate::configuration_queue::ConfigurationSelector::parse(7).unwrap()));
        assert_eq!(backend.identity_byte(0x80), Some(7));
        let mut controller = service_controller(ControllerState::PendingVendor);
        assert_eq!(controller.consume_completion(completed(4, 0), &mut commands, &mut reply, None), Ok(CompletionDisposition::Command { ready: true }));
        assert_eq!(controller.consume_completion(completed(4, 0), &mut commands, &mut reply, None), Ok(CompletionDisposition::Deferred));
    });
}

#[test]
fn function_five_requires_both_gate_and_registration() {
    for enabled in [false, true] {
        with_backend(enabled, |backend| {
            let mut commands = Commands::new();
            let mut request = packet(Command::Debug);
            word(&mut request, 16, 5);
            word(&mut request, 32, 10);
            request[36..46].copy_from_slice(b"POWER OFF\0");
            let mut reply = [0; 512];
            commands.dispatch(&mut request, 46, &mut reply, &mut [], backend).unwrap();
            assert_eq!((read_word(&reply, 4), read_word(&reply, 8)), (1, 3));
            assert!(backend.take_application_event().is_none());
            if enabled {
                backend.register_application_executor().unwrap();
                commands.dispatch(&mut request, 46, &mut reply, &mut [], backend).unwrap();
                assert_eq!((read_word(&reply, 4), read_word(&reply, 8)), (0, 0));
                assert_eq!(backend.take_application_event(), Some(crate::debug_events::ApplicationEvent::PowerOffRequested));
            }
        });
    }
}

#[test]
fn upload_and_generated_payload_share_only_caller_owned_bytes() {
    with_backend(false, |backend| {
        let mut commands = Commands::new();
        let mut payload = [0x13; 65_536];
        let mut reply = [0; 512];
        let mut request = packet(Command::Upload);
        word(&mut request, 20, 20);
        let result = commands.dispatch(&mut request, 32, &mut reply, &mut payload, backend).unwrap();
        assert_eq!(result.transfer, TransferEffect::Upload { length: ByteCount(20) });
        assert!(payload.iter().all(|byte| *byte == 0x13));
        request = packet(Command::PrepareUpload);
        word(&mut request, 20, 20);
        word(&mut request, 24, 256);
        commands.dispatch(&mut request, 32, &mut reply, &mut payload, backend).unwrap();
        request = packet(Command::GeneratedPayload);
        word(&mut request, 16, 256);
        let saved = commands.dispatch(&mut request, 32, &mut reply, &mut payload, backend).unwrap();
        assert_eq!(saved.transfer, TransferEffect::Download { region: PayloadRegion::CallerBuffer, length: ByteCount(20) });
        word(&mut request, 16, 0);
        commands.dispatch(&mut request, 32, &mut reply, &mut payload, backend).unwrap();
        assert_eq!(&payload[254..259], &[254, 255, 0, 1, 2]);
        assert_eq!(read_word(&reply, 20), 65_536);
        assert_eq!(commands.dispatch(&mut request, 32, &mut reply, &mut payload[..100], backend), Err(CommandError::PayloadBufferTooSmall { required: ByteCount(65_536) }));
    });
}

#[test]
fn provider_request_cannot_publish_success_before_real_data() {
    with_backend(false, |backend| {
        let mut commands = Commands::new();
        let mut request = packet(Command::ProviderPayload);
        word(&mut request, 16, 0x12);
        let mut reply = [0; 512];
        let result = commands.dispatch(&mut request, 32, &mut reply, &mut [], backend).unwrap();
        assert!(!result.ready);
        assert_eq!(result.transfer, TransferEffect::ProviderRequest { selector: PayloadSelector(0x12) });
        let mut frame = [0; 0x94];
        word(&mut frame, 0x8c, 0x1000);
        word(&mut frame, 0x90, 30);
        let image = [0x71; 30];
        let publication = ImagePublication::from_frame(ImagePublicationMode::Intermediate, &frame).unwrap();
        let published = PublishedImage::new(publication, PublishedRegion::adopt(0x1000, &image).unwrap()).unwrap();
        let regions = ProviderRegions { primary_log: None, secondary_log: None, image: Some(published),
            diagnostic: None, descriptor_image: None, device_info: None, diagnostic_layout: DiagnosticLayout::Ordinary };
        let prepared = provider_bytes(ProviderRoute::PublishedImage, &regions, backend).unwrap();
        assert_eq!(prepared.bytes(), &image);
        assert_eq!(prepared.address(), 0x1000);
        assert_eq!(prepared.region().length(), ByteCount(30));
        let mut controller = service_controller(ControllerState::PendingVendor);
        assert_eq!(controller.consume_completion(completed(4, 0), &mut commands, &mut reply, Some(&prepared)), Ok(CompletionDisposition::Command { ready: true }));
        assert_eq!(read_word(&reply, 20), 30);
    });
}

#[test]
fn capture_copies_device_pages_and_derives_published_length() {
    let (mut capture, start) = Capture::begin();
    assert_eq!(start, CaptureEffect::RegisterObserverAndWait);
    assert!(capture.ready(Ok(())).unwrap().is_some());
    capture.initialized(Ok(())).unwrap();
    let mut output = [0; 0x3000];
    let page = [0x31; 0x800];
    for _ in 0..5 { capture.page(Ok(&page), &mut output).unwrap(); }
    let mut header_page = [0x81; 0x800];
    header_page[..5].copy_from_slice(&[0xff, 0xf0, 1, 2, 3]);
    capture.page(Ok(&header_page), &mut output).unwrap();
    assert_eq!(capture.page(Ok(&[]), &mut output), Ok(CaptureEffect::UnregisterObserver));
    assert_eq!(capture.result(), Some(CaptureResult::Payload(ByteCount(0xb02))));
    assert_eq!(&output[..0x800], &page);
    let payload = capture.prepared(PublishedRegion::adopt(0x014e6b08, &output).unwrap()).unwrap();
    assert_eq!(payload.address(), 0x014e6b08);
    assert_eq!(payload.length(), ByteCount(0xb02));
}

#[test]
fn invalid_provider_selectors_do_not_acquire_arbitrary_regions() {
    with_backend(false, |backend| {
        let descriptor = ImageDescriptor::new(1).unwrap();
        for selector in [0, 0x134, 0x25a, 0x25b, 0x25d, 0x261, 0x262, 0x26c, 0x26d, 0x276] {
            assert_eq!(provider_route(PayloadSelector(selector), descriptor, backend), Err(ProviderError::InvalidSelector));
        }
        assert_eq!(provider_route(PayloadSelector(0x2b3), descriptor, backend), Ok(ProviderRoute::Empty));
        let extent = image_extent(descriptor, ImagePacking::Unpacked, backend).unwrap();
        assert_eq!(extent.columns, 4992);
        assert_eq!(extent.rows, 3348);
        assert_eq!(extent.stride, ByteCount(9984));
    });
}

#[test]
fn debug_ring_memory_read_and_write_observe_gate_and_admitted_region() {
    with_backend(true, |backend| {
        let mut commands = Commands::new();
        let mut request = packet(Command::Debug);
        let mut reply = [0; 512];
        word(&mut request, 16, 1);
        word(&mut request, 20, 5);
        commands.dispatch(&mut request, 32, &mut reply, &mut [], backend).unwrap();
        assert_eq!(read_word(&reply, 24), 3);
        assert_eq!(&reply[28..33], &[0x42, 0x42, 0x42, 0, 0]);
        word(&mut request, 16, 4);
        commands.dispatch(&mut request, 32, &mut reply, &mut [], backend).unwrap();
        assert_eq!(read_word(&reply, 24), 0);
        word(&mut request, 16, 3);
        word(&mut request, 28, 0x1002);
        word(&mut request, 32, 2);
        request[36..38].copy_from_slice(&[0x24, 0x78]);
        commands.dispatch(&mut request, 38, &mut reply, &mut [], backend).unwrap();
        word(&mut request, 16, 2);
        commands.dispatch(&mut request, 36, &mut reply, &mut [], backend).unwrap();
        assert_eq!(&reply[36..38], &[0x24, 0x78]);
        word(&mut request, 28, 0x2000);
        commands.dispatch(&mut request, 36, &mut reply, &mut [], backend).unwrap();
        assert_eq!((read_word(&reply, 4), read_word(&reply, 8)), (9, 3));
        assert!(backend.take_configuration_event().is_none());
        assert!(backend.take_application_event().is_none());
    });
}

#[derive(Default)]
struct Messages {
    queued: std::vec::Vec<u8>,
    opcode: Option<u16>,
    pending: Option<UploadPending>,
    fail: bool,
}

impl UploadMessages for Messages {
    fn configuration(&mut self, message: ConfigurationUpload<'_>) -> Result<(), i32> {
        if self.fail { return Err(-1); }
        assert_eq!((message.source, message.destination), (0xff, 7));
        self.opcode = Some(message.opcode);
        self.queued.extend_from_slice(message.payload);
        Ok(())
    }
    fn pending_transport(&mut self, message: UploadPending) -> Result<(), i32> {
        self.pending = Some(message);
        Ok(())
    }
}

#[test]
fn upload_completion_queues_actual_bytes_and_waits_for_configuration_completion() {
    with_backend(false, |backend| {
        let mut commands = Commands::new();
        let mut request = packet(Command::Upload);
        word(&mut request, 20, 3);
        let mut reply = [0; 512];
        let mut payload = [0; 8];
        commands.dispatch(&mut request, 32, &mut reply, &mut payload, backend).unwrap();
        let mut messages = Messages::default();
        let result = commands.complete_upload(&[3, 4, 5], UploadProvider::Configuration, &mut reply, &mut messages).unwrap();
        assert!(!result.ready);
        assert!(result.clear_receive_state);
        assert_eq!(messages.queued, [3, 4, 5]);
        assert_eq!(messages.opcode, Some(0x1708));
        assert_eq!(messages.pending, Some(UploadPending { destination: 0x24, opcode: 0x3407, kind: 2 }));
        assert_eq!(commands.service_state, ServiceState::Processing);
        assert_eq!(read_word(&reply, 0), 0x3fffff);
        let mut controller = service_controller(ControllerState::PendingVendor);
        assert_eq!(controller.consume_completion(completed(4, 0), &mut commands, &mut reply, None), Ok(CompletionDisposition::Command { ready: true }));
        assert_eq!(commands.service_state, ServiceState::Success);
        assert_eq!(commands.uploaded_length(), ByteCount(3));
        assert!(matches!(commands.complete_upload(&[3, 4, 5], UploadProvider::Configuration, &mut reply, &mut messages), Err(CommandError::NoPendingOperation)));
    });
}

#[test]
fn failed_upload_submission_is_not_a_successful_configuration_commit() {
    with_backend(false, |backend| {
        let mut commands = Commands::new();
        let mut request = packet(Command::Upload);
        word(&mut request, 20, 1);
        let mut reply = [0; 512];
        commands.dispatch(&mut request, 32, &mut reply, &mut [0; 4], backend).unwrap();
        let mut messages = Messages { fail: true, ..Messages::default() };
        let result = commands.complete_upload(&[7], UploadProvider::Configuration, &mut reply, &mut messages).unwrap();
        assert!(result.ready);
        assert_eq!(read_word(&reply, 20), u32::MAX);
        assert_eq!(commands.service_state, ServiceState::Failed { code: 0 });
        assert!(messages.queued.is_empty());
        assert!(messages.pending.is_none());
    });
}

#[test]
fn early_configuration_completion_waits_for_actual_pending_message() {
    with_backend(false, |backend| {
        let mut commands = Commands::new();
        let mut request = packet(Command::Configuration);
        word(&mut request, 16, 3);
        word(&mut request, 20, 1);
        word(&mut request, 28, 0x80);
        word(&mut request, 32, 1);
        request[36] = 7;
        let mut reply = [0; 512];
        commands.dispatch(&mut request, 37, &mut reply, &mut [], backend).unwrap();
        assert!(backend.take_configuration_event().is_some());
        let mut controller = service_controller(ControllerState::ActiveVendor);
        assert_eq!(controller.consume_completion(completed(4, 1), &mut commands, &mut reply, None), Ok(CompletionDisposition::Deferred));
        assert_eq!(read_word(&reply, 4), 0);
        controller.transition(ControllerState::PendingVendor).unwrap();
        assert_eq!(controller.consume_completion(completed(4, 1), &mut commands, &mut reply, None), Ok(CompletionDisposition::Command { ready: true }));
        assert_eq!(controller.state(), ControllerState::ActiveVendor);
        assert_eq!((read_word(&reply, 4), read_word(&reply, 8)), (1, 0xffff));
    });
}
