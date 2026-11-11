use super::*;
const IDENTITY: Identity<'static> = Identity { vendor: "Synthetic Vendor", product: "Synthetic Camera", serial: "synthetic-not-a-camera" };
fn objects() -> [DpsObject; 2] { [DpsObject { id: FileId(0), file: DpsFile::Request, enabled: false, length: 0 }; 2] }
fn response(body: &str) -> std::string::String { std::format!("<?xml version=\"1.0\"?><dps><output><result>10000000</result>{body}</output></dps>") }
fn ready(runtime: &mut PictBridge, transport: &mut DpsTransport<'_>) {
    runtime.initialize(); transport.observe_host_discovery("Z:\\HDISCVRY.DPS");
    runtime.service(&IDENTITY, transport).unwrap(); transport.complete_send();
    runtime.receive_response(response("<configurePrintService><printServiceAvailable>30010000</printServiceAvailable><vendorName>Synthetic Printer</vendorName><productName>Printer</productName></configurePrintService>").as_bytes());
    runtime.service(&IDENTITY, transport).unwrap(); transport.complete_send();
    runtime.receive_response(response("<getCapability><capability><datePrints>54010000 54020000</datePrints></capability></getCapability>").as_bytes());
    runtime.service(&IDENTITY, transport).unwrap(); transport.complete_send();
    runtime.receive_response(response("<getCapability><capability><fileTypes>53030000 53180000</fileTypes></capability></getCapability>").as_bytes());
    assert!(matches!(runtime.take_publication(), Some(Publication::PrintState { completion: Completion::Success, date: true, jpeg: true, .. })));
}
#[test]
fn discovery_configure_and_capabilities_publish_real_xml() {
    let mut entries = objects(); let mut transport = DpsTransport::new(&mut entries, &[]); let mut runtime = PictBridge::new();
    runtime.initialize(); runtime.service(&IDENTITY, &mut transport).unwrap(); assert!(runtime.request_bytes().is_empty());
    transport.observe_host_discovery("Z:\\HDISCVRY.DPS"); runtime.service(&IDENTITY, &mut transport).unwrap();
    let text = core::str::from_utf8(runtime.request_bytes()).unwrap();
    assert!(text.contains("<vendorName>Synthetic Vendor</vendorName>")); assert!(text.contains("<serialNo>synthetic-not-a-camera</serialNo>"));
    assert_eq!(transport.pending_object(), Some(FileId(1))); assert_eq!(transport.objects[0].length as usize, runtime.request_bytes().len());
    assert_eq!(runtime.phase(), Phase::Configuring);
}
#[test]
fn start_uses_printer_permission_real_file_id_date_and_clamped_copies() {
    let mut entries = objects(); let mut transport = DpsTransport::new(&mut entries, &[]); let mut runtime = PictBridge::new(); ready(&mut runtime, &mut transport);
    runtime.set_request(PrintRequest { file: FileId(0x1234), copies: Copies::Count(1001), date: Some("2026/11/10") });
    runtime.service(&IDENTITY, &mut transport).unwrap(); assert_eq!(runtime.take_publication(), Some(Publication::Start(Completion::Failed)));
    runtime.receive_request(b"<?xml version=\"1.0\"?><dps><input><notifyDeviceStatus><newJobOK>76010000</newJobOK></notifyDeviceStatus></input></dps>", &transport).unwrap();
    runtime.service(&IDENTITY, &mut transport).unwrap(); transport.complete_send(); runtime.take_publication();
    runtime.set_request(PrintRequest { file: FileId(0x1234), copies: Copies::Count(1001), date: Some("2026/11/10") }); runtime.service(&IDENTITY, &mut transport).unwrap();
    let text = core::str::from_utf8(runtime.request_bytes()).unwrap(); assert!(text.contains("<fileID>00001234</fileID>")); assert!(text.contains("<copies>999</copies>")); assert!(text.contains("<date>2026/11/10</date>"));
    transport.complete_send(); runtime.receive_response(response("<startJob/>").as_bytes()); assert_eq!(runtime.take_publication(), Some(Publication::Start(Completion::Success)));
}
#[test]
fn abort_and_continue_follow_device_status_and_completion() {
    let mut entries = objects(); let mut transport = DpsTransport::new(&mut entries, &[]); let mut runtime = PictBridge::new(); ready(&mut runtime, &mut transport);
    runtime.receive_request(b"<?xml version=\"1.0\"?><dps><input><notifyDeviceStatus><dpsPrintServiceStatus>70020000</dpsPrintServiceStatus><errorStatus>72010000</errorStatus></notifyDeviceStatus></input></dps>", &transport).unwrap();
    runtime.service(&IDENTITY, &mut transport).unwrap(); transport.complete_send(); runtime.take_publication();
    runtime.abort(AbortStyle::Immediate); runtime.service(&IDENTITY, &mut transport).unwrap(); assert!(core::str::from_utf8(runtime.request_bytes()).unwrap().contains("<abortStyle>90000000</abortStyle>"));
    transport.complete_send(); runtime.receive_response(response("<abortJob/>").as_bytes()); assert_eq!(runtime.take_publication(), Some(Publication::Abort(Completion::Success)));
    runtime.continue_job(); runtime.service(&IDENTITY, &mut transport).unwrap(); assert!(core::str::from_utf8(runtime.request_bytes()).unwrap().contains("<continueJob/>"));
    transport.complete_send(); runtime.receive_response(b"not xml"); assert_eq!(runtime.take_publication(), Some(Publication::Continue(Completion::Failed)));
}
#[test]
fn file_id_request_resolves_only_owner_supplied_catalog() {
    let files = [CatalogFile { id: FileId(73), path: "A:\\DCIM\\SYNTH.JPG" }];
    let mut entries = objects(); let mut transport = DpsTransport::new(&mut entries, &files); let mut runtime = PictBridge::new(); ready(&mut runtime, &mut transport);
    runtime.receive_request(b"<?xml version=\"1.0\"?><dps><input><getFileID><filePath>A:/DCIM/SYNTH.JPG</filePath></getFileID></input></dps>", &transport).unwrap();
    assert!(core::str::from_utf8(runtime.response_bytes()).unwrap().contains("<fileID>00000049</fileID>"));
    runtime.receive_request(b"<?xml version=\"1.0\"?><dps><input><getFileID><filePath>A:/missing</filePath></getFileID></input></dps>", &transport).unwrap();
    assert!(core::str::from_utf8(runtime.response_bytes()).unwrap().contains("<result>10010000</result>"));
}
#[test]
fn busy_preserves_pending_request_and_send_lock_completes_explicitly() {
    let mut entries = objects(); let mut transport = DpsTransport::new(&mut entries, &[]); let mut runtime = PictBridge::new(); runtime.initialize(); transport.observe_host_discovery("Z:\\HDISCVRY.DPS"); transport.set_transfer_active(true);
    runtime.service(&IDENTITY, &mut transport).unwrap(); assert_eq!(runtime.phase(), Phase::Configure); assert_eq!(transport.pending_object(), None);
    transport.set_transfer_active(false); runtime.service(&IDENTITY, &mut transport).unwrap(); assert_eq!(runtime.phase(), Phase::Configuring);
    assert_eq!(transport.send(DpsFile::Response, 10), SendStatus::Busy); transport.complete_send(); assert_eq!(transport.send(DpsFile::Response, 10), SendStatus::Sent);
}
#[test]
fn malformed_notifications_do_not_acknowledge_success_and_reset_silences_inputs() {
    let mut entries = objects(); let mut transport = DpsTransport::new(&mut entries, &[]); let mut runtime = PictBridge::new(); ready(&mut runtime, &mut transport);
    runtime.receive_request(b"broken", &transport).unwrap(); assert!(core::str::from_utf8(runtime.response_bytes()).unwrap().contains("10030000"));
    runtime.reset(); runtime.set_request(PrintRequest { file: FileId(3), copies: Copies::Count(2), date: None }); runtime.service(&IDENTITY, &mut transport).unwrap(); assert_eq!(transport.pending_object(), None);
}
#[test]
fn hex_scanner_matches_eight_contiguous_digits_and_ignores_comments_for_elements() {
    assert_eq!(hex("bad! 12345678"), Some(0x12345678)); assert_eq!(hex("1234567 8"), None);
    assert_eq!(element("<!-- <result>FFFFFFFF</result> --><result>10000000</result>", "result"), Some("10000000"));
}
#[test]
fn notifications_publish_bounded_job_text_and_reject_invalid_status() {
    let mut entries = objects(); let mut transport = DpsTransport::new(&mut entries, &[]); let mut runtime = PictBridge::new(); ready(&mut runtime, &mut transport);
    runtime.receive_request(b"<?xml version=\"1.0\"?><dps><input><notifyJobStatus><progress>012/100</progress><imagesPrinted>007</imagesPrinted></notifyJobStatus></input></dps>", &transport).unwrap();
    assert_eq!(runtime.take_publication(), Some(Publication::Job { completion: Completion::Success, progress: *b"012/100\0", images: *b"007\0" }));
    runtime.receive_request(b"<?xml version=\"1.0\"?><dps><input><notifyDeviceStatus><newJobOK>DEAD0000</newJobOK></notifyDeviceStatus></input></dps>", &transport).unwrap();
    assert!(matches!(runtime.take_publication(), Some(Publication::Device { completion: Completion::Failed, .. })));
    assert!(core::str::from_utf8(runtime.response_bytes()).unwrap().contains("<result>10020000</result>"));
}
#[test]
fn mpo_job_omits_date_and_copies_and_unknown_packet_paths_fail() {
    let mut entries = objects(); let mut transport = DpsTransport::new(&mut entries, &[]); let mut runtime = PictBridge::new(); ready(&mut runtime, &mut transport);
    runtime.receive_request(b"<?xml version=\"1.0\"?><dps><input><notifyDeviceStatus><newJobOK>76010000</newJobOK></notifyDeviceStatus></input></dps>", &transport).unwrap();
    runtime.service(&IDENTITY, &mut transport).unwrap(); transport.complete_send();
    runtime.set_request(PrintRequest { file: FileId(17), copies: Copies::Mpo, date: Some("2026/11/10") }); runtime.service(&IDENTITY, &mut transport).unwrap();
    let text = core::str::from_utf8(runtime.request_bytes()).unwrap();
    assert!(text.contains("<fileType>53180000</fileType>")); assert!(!text.contains("<date>")); assert!(!text.contains("<copies>"));
    assert_eq!(runtime.dispatch_packet("Z:\\UNKNOWN.DPS", b"", &transport).unwrap(), 0x80000000);
}
