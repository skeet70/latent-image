use super::{AdmissionError, CallbackRange, DispatchTicks, EepromEvents, EepromTask100};
use crate::backend::ConfigImages;
use crate::eeprom_detection::{DetectionIo, DetectionProvider, DetectionSource, DetectionState};
use crate::eeprom_profile_medium::ProfileMedium;
use crate::lens_queue_runtime::{Publication, Status};
use crate::lens_queue_runtime::packets::{CallbackId, ControlOperation};
use crate::lens_queue_runtime::replies::ReplyData;
use crate::lens_queue_runtime::physical_fixture::{self, Interrupts, Registers, Resources, Runtime};
use crate::startup_projection::StartupProjection;
use core::{cell::RefCell, future::Future, pin::Pin, task::{Context, Poll, Waker}};
use std::sync::Arc;
use xm1_lens_transport::control_actor::ControlActor;
use xm1_lens_transport::control_runtime::{CompletionMailbox, ControlPublications, ControlSender, Task99Client};
use xm1_lens_transport::control_types::{ControlError, ControlMessage, SharedReadiness};
use xm1_power_services::primitives::ServiceStatus;
use xm1_power_services::shutdown::RequestId;
use xm1_power_services::software_timer_service::{DetectionIrqToken, DetectionRegister};
use xm1_power_services::storage_shutdown_actor::{MediumStatus, NandBlock};
use xm1_power_services::storage_shutdown_defaults::{DefaultMediaOffset, DefaultStorageIo, IndexedBlock};
use xm1_power_services::storage_shutdown_lifecycle::{LensCompletion, LensDmaRegister};
use xm1_power_services::storage_shutdown_load::LensLoadSoftware;
use xm1_power_services::storage_shutdown_medium::{BufferAddress, NandBlockTransfer, NandByteTransfer, NandErase, NandSoftware, NandWait, StorageShutdownMedium};
use xm1_power_services::storage_shutdown_pages::{LensPageSoftware, PageBoundary};

struct Wake;
impl std::task::Wake for Wake { fn wake(self: Arc<Self>) {} }
fn poll<F: Future>(future: Pin<&mut F>) -> Poll<F::Output> {
    let waker = Waker::from(Arc::new(Wake));
    future.poll(&mut Context::from_waker(&waker))
}
struct Hardware;
impl DetectionIo for Hardware {
    type SavedCritical = ();
    fn enter(&self) {}
    fn leave(&self, _: ()) {}
    fn write_register(&self, _: DetectionRegister, _: u32) {}
    fn read_register(&self, _: DetectionRegister) -> u32 { 0 }
    fn write_dma_register(&self, _: LensDmaRegister, _: u32) {}
    fn attach_irq(&self, _: DetectionSource) -> Result<DetectionIrqToken, ServiceStatus> { Ok(DetectionIrqToken(1)) }
    fn release_irq(&self, _: DetectionIrqToken) -> Result<(), ServiceStatus> { Ok(()) }
}
struct Sender;
impl ControlSender for Sender { fn post(&self, _: ControlMessage) -> Result<(), ControlError> { panic!("unexpected task99 request") } }
struct Nand;
impl NandSoftware for Nand {
    fn initialized(&self) -> bool { false }
    fn configure_wait(&mut self, _: NandWait) -> MediumStatus { panic!("unexpected NAND request") }
    fn shutdown_if_initialized(&mut self) { panic!("unexpected NAND request") }
    fn erase_mapped_blocks(&mut self, _: &mut NandErase) -> MediumStatus { panic!("unexpected NAND request") }
    fn write_mapped_blocks(&mut self, _: &mut NandBlockTransfer, _: &[u8; 0x40000]) -> MediumStatus { panic!("unexpected NAND request") }
    fn read_mapped_blocks(&mut self, _: &mut NandBlockTransfer, _: &mut [u8; 0x40000]) -> MediumStatus { panic!("unexpected NAND request") }
    fn write_mapped_bytes(&mut self, _: &mut NandByteTransfer, _: &[u8; 0xa08]) -> MediumStatus { panic!("unexpected NAND request") }
    fn read_mapped_bytes(&mut self, _: &mut NandByteTransfer, _: &mut [u8; 0xa08]) -> MediumStatus { panic!("unexpected NAND request") }
    fn clean_cache(&mut self, _: BufferAddress, _: u32) { panic!("unexpected NAND request") }
    fn clean_invalidate_all(&mut self) { panic!("unexpected NAND request") }
}
struct Defaults;
impl DefaultStorageIo for Defaults {
    fn section_offset(&mut self) -> Result<DefaultMediaOffset, MediumStatus> { panic!("unexpected defaults request") }
    fn lock_cache(&mut self) -> MediumStatus { panic!("unexpected defaults request") }
    fn unlock_cache(&mut self) { panic!("unexpected defaults request") }
    fn read_cached(&mut self, _: IndexedBlock) -> MediumStatus { panic!("unexpected defaults request") }
    fn copy_cached(&mut self, _: u32, _: &mut [u8]) { panic!("unexpected defaults request") }
    fn read_direct(&mut self, _: IndexedBlock, _: u32, _: &mut [u8]) -> MediumStatus { panic!("unexpected defaults request") }
}
type Task<'a> = EepromTask100<'a, 'a, Registers, Resources, Interrupts, Nand, Defaults, Hardware>;
fn with_task(test: impl for<'a> FnOnce(&mut Task<'a>, &'a EepromEvents, &'a RefCell<Runtime>)) {
    let (mut runtime, _) = physical_fixture::setup();
    physical_fixture::initialize(&mut runtime);
    let runtime = RefCell::new(runtime);
    let events = EepromEvents::new(CallbackRange::new(CallbackId(0x20000000), CallbackId(0x2fffffff)).unwrap());
    let mut bytes = std::vec![0; 0x420000];
    let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
    let readiness = SharedReadiness::new();
    let publications = ControlPublications::new();
    publications.publish(&ControlActor::new([0; 128], false, &readiness));
    let mailbox = CompletionMailbox::new();
    let client = Task99Client::new(&Sender, &mailbox, &publications, &readiness);
    let mut medium = ProfileMedium::new(StorageShutdownMedium { driver: Nand }, Defaults, NandBlock(0));
    let state = RefCell::new(DetectionState::new());
    let detection = DetectionProvider::new(&state, &Hardware, &events);
    detection.initialize().unwrap();
    let mut task = EepromTask100::new(&runtime, &events, &mut configuration, &mut medium, client, StartupProjection::new(), detection).unwrap();
    test(&mut task, &events, &runtime);
}

#[test]
fn channel_completion_publishes_descriptor_fields() {
    with_task(|task, events, runtime| {
        task.request_channel(xm1_power_services::storage_shutdown_load::LensChannel::Full);
        let callback = events.active_callback().unwrap();
        let mut context = [0xa5; 0xc0];
        let mut wait = std::boxed::Box::pin(task.wait(LensCompletion::Information, &mut context));
        assert!(poll(wait.as_mut()).is_pending());
        let operation = ControlOperation::parse(RequestId(0x722e)).unwrap();
        assert!(!events.publication(Publication::Command { callback: CallbackId(callback.0 + 1), operation, status: Status::Complete, data: ReplyData::StatusOnly }, &runtime.borrow()));
        assert!(events.publication(Publication::Command { callback, operation, status: Status::Complete, data: ReplyData::Channel { pages: 17, format: 3, present: true } }, &runtime.borrow()));
        assert_eq!(poll(wait.as_mut()), Poll::Ready(MediumStatus(0)));
        drop(wait);
        assert_eq!(&context[0x30..0x32], &17u16.to_le_bytes());
        assert_eq!(context[0x3a], 3);
        assert_eq!(context[0x3b], 1);
    });
}

#[test]
fn cancelling_wait_invalidates_generation_and_stops_wire() {
    with_task(|task, events, runtime| {
        task.request_channel(xm1_power_services::storage_shutdown_load::LensChannel::Full);
        let callback = events.active_callback().unwrap();
        let mut context = [0; 0xc0];
        let mut wait = std::boxed::Box::pin(task.wait(LensCompletion::Information, &mut context));
        assert!(poll(wait.as_mut()).is_pending());
        drop(wait);
        assert_eq!(events.active_callback(), None);
        assert!(!events.signal(callback, LensCompletion::Information, MediumStatus(0)));
        assert_eq!(runtime.borrow().state(), xm1_power_services::storage_shutdown_lens_machine::LensServiceState::Stopped);
    });
}

#[test]
fn primary_dma_registration_survives_multiple_page_waits() {
    with_task(|task, events, _| {
        let callback = events.arm(LensCompletion::Dma).unwrap();
        assert!(events.signal(callback, LensCompletion::Dma, MediumStatus(0)));
        let mut first = std::boxed::Box::pin(task.page_boundary(PageBoundary::Primary));
        assert_eq!(poll(first.as_mut()), Poll::Ready(MediumStatus(0)));
        drop(first);
        assert_eq!(events.active_callback(), Some(callback));
        task.clear_notifications();
        let mut second = std::boxed::Box::pin(task.page_boundary(PageBoundary::Primary));
        assert!(poll(second.as_mut()).is_pending());
        assert!(events.signal(callback, LensCompletion::Dma, MediumStatus(0)));
        assert_eq!(poll(second.as_mut()), Poll::Ready(MediumStatus(0)));
        drop(second);
        events.cancel_dma();
        assert!(!events.signal(callback, LensCompletion::Dma, MediumStatus(0)));
    });
}

#[test]
fn dma_registration_does_not_occupy_command_completion() {
    with_task(|task, events, runtime| {
        let dma = events.arm(LensCompletion::Dma).unwrap();
        task.request_channel(xm1_power_services::storage_shutdown_load::LensChannel::Full);
        let command = events.active_callback().unwrap();
        assert_ne!(dma, command);
        assert!(events.signal(dma, LensCompletion::Dma, MediumStatus(0)));
        let operation = ControlOperation::parse(RequestId(0x722e)).unwrap();
        assert!(events.publication(Publication::Command { callback: command, operation, status: Status::Complete, data: ReplyData::Channel { pages: 1, format: 0, present: true } }, &runtime.borrow()));
        let mut context = [0; 0xc0];
        let mut wait = std::boxed::Box::pin(task.wait(LensCompletion::Information, &mut context));
        assert_eq!(poll(wait.as_mut()), Poll::Ready(MediumStatus(0)));
        drop(wait);
        assert_eq!(events.active_callback(), Some(dma));
    });
}

#[test]
fn delay_uses_dispatch_ticks_and_drop_releases_timer() {
    let events = EepromEvents::new(CallbackRange::new(CallbackId(1), CallbackId(20)).unwrap());
    let mut delay = std::boxed::Box::pin(events.delay(10));
    assert!(poll(delay.as_mut()).is_pending());
    events.advance_ticks(DispatchTicks(9));
    assert!(poll(delay.as_mut()).is_pending());
    events.advance_ticks(DispatchTicks(1));
    assert_eq!(poll(delay.as_mut()), Poll::Ready(()));
    drop(delay);
    let mut cancelled = std::boxed::Box::pin(events.delay(10));
    assert!(poll(cancelled.as_mut()).is_pending());
    drop(cancelled);
    assert_eq!(events.begin_request(), Ok(()));
}

#[test]
fn software_failure_survives_wait_cleanup_until_new_request() {
    with_task(|task, events, _| {
        events.fail_software(AdmissionError::Configuration);
        let mut context = [0; 0xc0];
        let mut wait = std::boxed::Box::pin(task.wait(LensCompletion::Information, &mut context));
        assert_eq!(poll(wait.as_mut()), Poll::Ready(MediumStatus(-255)));
        drop(wait);
        assert_eq!(task.software_status(), MediumStatus(-255));
        assert_eq!(events.begin_request(), Ok(()));
        assert_eq!(task.software_status(), MediumStatus(0));
    });
}
