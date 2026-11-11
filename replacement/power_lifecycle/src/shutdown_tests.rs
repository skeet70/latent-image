use super::*;
use crate::area2::{Area2Record, RECORD_BYTES};
use crate::area2_codec::Area2Codec;
use crate::mirror::*;
use crate::record_store::*;
use crate::shutdown_completion::CompletionRequests;
use crate::shutdown_requests::{OwnedCompletionAddress, Task8Readiness};
use std::{boxed::Box, vec, vec::Vec};

#[derive(Default)]
struct Queue { messages: Vec<(RequestId, TaskId, QueueId, Vec<u8>)>, flags: Vec<(u32, u32)> }
impl ShutdownRtos for Queue {
    fn current_task(&mut self) -> TaskId { TaskId(2) }
    fn post(&mut self, message: ServiceMessage<'_>) -> Result<(), KernelStatus> {
        self.messages.push((message.operation, message.sender, message.destination, message.payload.to_vec()));
        Ok(())
    }
    fn set_event_flag(&mut self, flag: u32, bits: u32) -> Result<(), KernelStatus> { self.flags.push((flag, bits)); Ok(()) }
}
struct Medium;
impl RecordMedium for Medium {
    fn read(&mut self, _: RecordKind, _: Bank, _: BytePosition, output: &mut [u8]) -> Result<(), MediumError> { output.fill(0xff); Ok(()) }
    fn program(&mut self, _: RecordKind, _: Bank, _: BytePosition, _: &[u8]) -> Result<(), MediumError> { panic!("equal records must not program"); }
    fn erase(&mut self, _: RecordKind, _: Bank) -> Result<(), MediumError> { panic!("equal records must not erase"); }
}
struct Stage([u8; RECORD_BYTES]);
impl Staging for Stage { fn bytes(&mut self, _: RecordKind) -> &mut [u8] { &mut self.0 } }
struct Driver;
impl HeaderBackupDriver for Driver {
    fn context(&mut self) -> ExecutionContext { ExecutionContext::Task }
    fn acquire(&mut self, _: BackupSemaphore) {}
    fn release(&mut self, _: BackupSemaphore) {}
    fn diagnostic(&mut self, _: BackupDiagnostic) {}
    fn admit(&mut self, _: RequestDirection) -> Result<(), DeviceStatus> { Ok(()) }
    fn write_backup(&mut self, _: BackupKey, _: BackupValue, _: BackupWriteMode) -> DeviceStatus { DeviceStatus(0) }
    fn store_request_status(&mut self, _: RequestStatus) {}
    fn report_failure(&mut self, _: RequestStatus) {}
}
const LIVE_TRANSITION: TransitionToken = TransitionToken(0x1234);
fn saved_shutdown() -> Shutdown {
    let mut medium = Medium;
    let mut stage = Stage([0; RECORD_BYTES]);
    let mut blocks: Box<[[u8; BLOCK_BYTES]; 2]> = vec![[0; BLOCK_BYTES]; 2].into_boxed_slice().try_into().ok().unwrap();
    let mut cache = CacheState::new();
    let mut service = Service::new();
    service.open(&mut stage).unwrap();
    let a2 = service.register(RecordKind::Kind2, RecordBytes(512), RecordCount(62), SequenceOffset(0), Area2Codec).unwrap();
    let a3 = service.register(RecordKind::Kind3, RecordBytes(512), RecordCount(62), SequenceOffset(0), Area2Codec).unwrap();
    let mut live = [0; BACKING_BYTES];
    let mut shadow = [0; HEADER_BYTES];
    let keys = core::array::from_fn(|index| BackupKey(index as u16));
    let mut backing = SharedBacking::without_export_residue(&mut live, &mut shadow, &keys, Driver);
    let mut working = Area2Record([0; RECORD_BYTES]);
    let mut snapshot = Area2Record([0; RECORD_BYTES]);
    let mut area2 = Area2Lifecycle::adopt(&mut working, &mut snapshot, a2, a3);
    let mut shutdown = Shutdown::new();
    let observation = shutdown.save(LIVE_TRANSITION, SaveArm::Close, &mut area2, &mut backing, FirmwareVersion { major: 0, minor: 0 }, &mut service, &mut Entry::new(&mut medium, &mut stage, &mut blocks, &mut cache));
    assert!(observation.is_some());
    assert!(service.get_mut(a2).is_none());
    assert!(service.get_mut(a3).is_none());
    shutdown
}
extern "C" fn completion_address() -> i32 { 0 }
fn requests(payload: &mut [u8; 16]) -> CompletionRequests<'_> {
    CompletionRequests {
        task8_readiness: Task8Readiness(1),
        task8_completion: OwnedCompletionAddress::from_entry(completion_address),
        calendar_completion: OwnedCompletionAddress::from_entry(completion_address),
        task8_payload: payload,
    }
}

#[test]
fn only_actual_persistence_completion_with_the_saved_token_arms_shutdown() {
    let mut shutdown = saved_shutdown();
    assert!(!shutdown.receive_barrier_start(RequestId(0x93e7), TransitionToken(3)));
    assert!(!shutdown.receive_barrier_start(RequestId(0x93e8), LIVE_TRANSITION));
    assert!(shutdown.receive_barrier_start(RequestId(0x93e7), LIVE_TRANSITION));
    assert!(!shutdown.receive_barrier_start(RequestId(0x93e7), LIVE_TRANSITION));
}

#[test]
fn duplicates_and_foreign_tokens_do_not_submit_follow_on_work() {
    let mut shutdown = saved_shutdown();
    assert!(shutdown.receive_barrier_start(RequestId(0x93e7), LIVE_TRANSITION));
    let mut queue = Queue::default();
    let mut events = UiEvents::new(UiRouting::Primary);
    let mut record = [0x6a; 76];
    let mut payload = [0x9d; 16];
    let mut requests = requests(&mut payload);
    let foreign = shutdown.completion(TransitionToken(4), CompletionSource::Settings, &mut queue, &mut events, &mut record, &mut requests).unwrap();
    assert_eq!(foreign.disposition, CompletionDisposition::IgnoredToken);
    assert!(queue.messages.is_empty());
    let first = shutdown.completion(LIVE_TRANSITION, CompletionSource::Settings, &mut queue, &mut events, &mut record, &mut requests).unwrap();
    assert_eq!(first.disposition, CompletionDisposition::Accepted);
    assert_eq!(queue.messages.iter().map(|message| (message.0, message.2)).collect::<Vec<_>>(), [(RequestId(0xf107), QueueId(2)), (RequestId(0x1801), QueueId(8))]);
    let duplicate = shutdown.completion(LIVE_TRANSITION, CompletionSource::Settings, &mut queue, &mut events, &mut record, &mut requests).unwrap();
    assert_eq!(duplicate.disposition, CompletionDisposition::IgnoredDuplicate);
    assert_eq!(queue.messages.len(), 2);
}

#[test]
fn nine_distinct_completions_gate_release_and_late_completions_post_only_91b() {
    let mut shutdown = saved_shutdown();
    assert!(shutdown.receive_barrier_start(RequestId(0x93e7), LIVE_TRANSITION));
    let mut queue = Queue::default();
    let mut events = UiEvents::new(UiRouting::Primary);
    let mut record = [0x6a; 76];
    let mut payload = [0x9d; 16];
    let mut requests = requests(&mut payload);
    let sources = [CompletionSource::Display, CompletionSource::Calendar, CompletionSource::Task7, CompletionSource::Device, CompletionSource::Task8, CompletionSource::Auxiliary, CompletionSource::Settings, CompletionSource::Optional, CompletionSource::Storage];
    for (index, source) in sources.into_iter().enumerate() {
        let observation = shutdown.completion(LIVE_TRANSITION, source, &mut queue, &mut events, &mut record, &mut requests).unwrap();
        assert_eq!(observation.disposition, if index == 8 { CompletionDisposition::Full } else { CompletionDisposition::Accepted });
    }
    assert_eq!(shutdown.phase(), ShutdownPhase::Releasing);
    assert!(queue.messages.iter().all(|message| message.3.get(..4) != Some(0x816u32.to_le_bytes().as_slice())));
    for source in sources {
        let observation = shutdown.completion(LIVE_TRANSITION, source, &mut queue, &mut events, &mut record, &mut requests).unwrap();
        assert_eq!(observation.disposition, CompletionDisposition::Late);
    }
    let late = queue.messages.iter().filter(|message| message.0 == RequestId(0)).collect::<Vec<_>>();
    assert_eq!(late.len(), 9);
    for message in late {
        assert_eq!(message.2, QueueId(7));
        assert_eq!(&message.3[..8], &[0x1b, 9, 0, 0, 0, 0, 0, 0]);
        assert_eq!(&message.3[8..], &[0x6a; 68]);
    }
    assert_eq!(events.first_request_task(), Some(TaskId(2)));
}

#[test]
fn terminal_envelope_writes_only_the_two_stock_words() {
    let mut queue = Queue::default();
    let mut payload = [0x9d; 16];
    post_lifecycle(&mut queue, LifecycleRequest::Terminal { argument: 0x3456 }, &mut payload).unwrap();
    assert_eq!(queue.messages[0].0, RequestId(3));
    assert_eq!(queue.messages[0].2, QueueId(2));
    assert_eq!(&queue.messages[0].3[..4], &[0x81, 0, 0, 0]);
    assert_eq!(&queue.messages[0].3[4..12], &[0x9d; 8]);
    assert_eq!(&queue.messages[0].3[12..], &0x3456u32.to_le_bytes());
}

fn finish_with_produced_completion(shutdown: &mut Shutdown, produced: CompletionSource) {
    let mut queue = Queue::default();
    let mut events = UiEvents::new(UiRouting::Primary);
    let mut record = [0x6a; 76];
    let mut payload = [0x9d; 16];
    let mut requests = requests(&mut payload);
    let sources = [
        CompletionSource::Display, CompletionSource::Calendar, CompletionSource::Task7,
        CompletionSource::Device, CompletionSource::Task8, CompletionSource::Auxiliary,
        CompletionSource::Settings, CompletionSource::Optional, CompletionSource::Storage,
    ];
    for source in sources.into_iter().filter(|source| *source != produced) {
        let observation = shutdown.completion(LIVE_TRANSITION, source, &mut queue, &mut events, &mut record, &mut requests).unwrap();
        assert_eq!(observation.disposition, CompletionDisposition::Accepted);
    }
    let observation = shutdown.completion(LIVE_TRANSITION, produced, &mut queue, &mut events, &mut record, &mut requests).unwrap();
    assert_eq!(observation.disposition, CompletionDisposition::Full);
    assert_eq!(shutdown.phase(), ShutdownPhase::Releasing);
}

#[test]
fn task7_producer_word_reaches_the_release_gate() {
    static EVENTS: std::sync::Mutex<Vec<RequestId>> = std::sync::Mutex::new(Vec::new());
    struct Rtos;
    impl ShutdownRtos for Rtos {
        fn current_task(&mut self) -> TaskId { TaskId(2) }
        fn post(&mut self, message: ServiceMessage<'_>) -> Result<(), KernelStatus> {
            EVENTS.lock().unwrap().push(message.operation);
            Ok(())
        }
        fn set_event_flag(&mut self, _: u32, _: u32) -> Result<(), KernelStatus> { Ok(()) }
    }
    extern "C" fn entry() -> i32 { crate::shutdown_requests::task7_completion(&mut Rtos) }
    let registry = crate::callback_registry::CallbackRegistry::new();
    let address = OwnedCompletionAddress::register(&registry, entry).unwrap();
    crate::shutdown_requests::invoke_status_completion(&registry, address.callback()).unwrap();
    let source = CompletionSource::from_event(EVENTS.lock().unwrap().pop().unwrap()).unwrap();
    assert_eq!(source, CompletionSource::Task7);
    let mut shutdown = saved_shutdown();
    assert!(shutdown.receive_barrier_start(RequestId(0x93e7), LIVE_TRANSITION));
    finish_with_produced_completion(&mut shutdown, source);
}

#[test]
fn task8_producer_word_reaches_the_release_gate() {
    static EVENTS: std::sync::Mutex<Vec<RequestId>> = std::sync::Mutex::new(Vec::new());
    struct Rtos;
    impl ShutdownRtos for Rtos {
        fn current_task(&mut self) -> TaskId { TaskId(2) }
        fn post(&mut self, message: ServiceMessage<'_>) -> Result<(), KernelStatus> {
            EVENTS.lock().unwrap().push(message.operation);
            Ok(())
        }
        fn set_event_flag(&mut self, _: u32, _: u32) -> Result<(), KernelStatus> { Ok(()) }
    }
    extern "C" fn entry() -> i32 { crate::shutdown_requests::task8_completion(&mut Rtos) }
    let registry = crate::callback_registry::CallbackRegistry::new();
    let address = OwnedCompletionAddress::register(&registry, entry).unwrap();
    crate::shutdown_requests::invoke_status_completion(&registry, address.callback()).unwrap();
    let source = CompletionSource::from_event(EVENTS.lock().unwrap().pop().unwrap()).unwrap();
    assert_eq!(source, CompletionSource::Task8);
    let mut shutdown = saved_shutdown();
    assert!(shutdown.receive_barrier_start(RequestId(0x93e7), LIVE_TRANSITION));
    finish_with_produced_completion(&mut shutdown, source);
}

#[test]
fn settings_producer_word_reaches_the_release_gate() {
    static EVENTS: std::sync::Mutex<Vec<RequestId>> = std::sync::Mutex::new(Vec::new());
    struct Rtos;
    impl ShutdownRtos for Rtos {
        fn current_task(&mut self) -> TaskId { TaskId(2) }
        fn post(&mut self, message: ServiceMessage<'_>) -> Result<(), KernelStatus> {
            EVENTS.lock().unwrap().push(message.operation);
            Ok(())
        }
        fn set_event_flag(&mut self, _: u32, _: u32) -> Result<(), KernelStatus> { Ok(()) }
    }
    extern "C" fn entry() -> i32 { crate::shutdown_requests::settings_completion(&mut Rtos) }
    let registry = crate::callback_registry::CallbackRegistry::new();
    let address = OwnedCompletionAddress::register(&registry, entry).unwrap();
    crate::shutdown_requests::invoke_status_completion(&registry, address.callback()).unwrap();
    let source = CompletionSource::from_event(EVENTS.lock().unwrap().pop().unwrap()).unwrap();
    assert_eq!(source, CompletionSource::Settings);
    let mut shutdown = saved_shutdown();
    assert!(shutdown.receive_barrier_start(RequestId(0x93e7), LIVE_TRANSITION));
    finish_with_produced_completion(&mut shutdown, source);
}

#[test]
fn device_producer_word_reaches_the_shutdown_mask() {
    static EVENTS: std::sync::Mutex<Vec<RequestId>> = std::sync::Mutex::new(Vec::new());
    struct Rtos;
    impl ShutdownRtos for Rtos {
        fn current_task(&mut self) -> TaskId { TaskId(2) }
        fn post(&mut self, message: ServiceMessage<'_>) -> Result<(), KernelStatus> {
            EVENTS.lock().unwrap().push(message.operation);
            Ok(())
        }
        fn set_event_flag(&mut self, _: u32, _: u32) -> Result<(), KernelStatus> { Ok(()) }
    }
    extern "C" fn entry() -> i32 { crate::shutdown_requests::device_completion(&mut Rtos) }
    let registry = crate::callback_registry::CallbackRegistry::new();
    let address = OwnedCompletionAddress::device(&registry, entry).unwrap();
    crate::shutdown_requests::invoke_status_completion(&registry, address.callback()).unwrap();
    let source = CompletionSource::from_event(EVENTS.lock().unwrap().pop().unwrap()).unwrap();
    assert_eq!(source, CompletionSource::Device);
    let mut shutdown = saved_shutdown();
    assert!(shutdown.receive_barrier_start(RequestId(0x93e7), LIVE_TRANSITION));
    finish_with_produced_completion(&mut shutdown, source);
    assert_eq!(shutdown.completed, 0x1ff);
}

#[test]
fn audio_shutdown_producer_word_sets_the_release_flag_and_the_mask_completes() {
    static FLAGS: std::sync::Mutex<Vec<(u32, u32)>> = std::sync::Mutex::new(Vec::new());
    struct Rtos;
    impl ShutdownRtos for Rtos {
        fn current_task(&mut self) -> TaskId { TaskId(41) }
        fn post(&mut self, _: ServiceMessage<'_>) -> Result<(), KernelStatus> { Ok(()) }
        fn set_event_flag(&mut self, flag: u32, bits: u32) -> Result<(), KernelStatus> {
            FLAGS.lock().unwrap().push((flag, bits));
            Ok(())
        }
    }
    extern "C" fn entry() -> i32 { crate::shutdown::audio_release_completion(&mut Rtos).map_or(-1, |()| 0) }
    let registry = crate::callback_registry::CallbackRegistry::new();
    let word = crate::shutdown_requests::register_audio_completion(&registry, entry as unsafe extern "C" fn() -> i32).unwrap();
    crate::shutdown_requests::invoke_status_completion(&registry, word).unwrap();
    assert_eq!(FLAGS.lock().unwrap().as_slice(), &[(2, 1)]);
    let mut shutdown = saved_shutdown();
    assert!(shutdown.receive_barrier_start(RequestId(0x93e7), LIVE_TRANSITION));
    finish_with_produced_completion(&mut shutdown, CompletionSource::Device);
    assert_eq!(shutdown.completed, 0x1ff);
}

#[test]
fn storage_dma_producer_word_reaches_the_release_gate() {
    static CALLS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    extern "C" fn entry() { CALLS.fetch_add(1, std::sync::atomic::Ordering::SeqCst); }
    let registry = crate::callback_registry::CallbackRegistry::new();
    let word = crate::storage_shutdown_dma::register(&registry, entry).unwrap();
    crate::software_timer_service::invoke_detection_callback(&registry, word).unwrap();
    assert_eq!(CALLS.load(std::sync::atomic::Ordering::SeqCst), 1);
    let mut shutdown = saved_shutdown();
    assert!(shutdown.receive_barrier_start(RequestId(0x93e7), LIVE_TRANSITION));
    finish_with_produced_completion(&mut shutdown, CompletionSource::Storage);
}
