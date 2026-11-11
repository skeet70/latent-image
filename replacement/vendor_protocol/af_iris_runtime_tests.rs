use super::*;
use core::cell::{Cell, RefCell};
use crate::af_motion_runtime::ZoomPosition;
use crate::eeprom_detection::{DetectionSource, DetectionState};
use crate::lens_queue_runtime::{Publication, Status};
use crate::lens_queue_runtime::physical_fixture;
use xm1_lens_transport::control_actor::ControlActor;
use xm1_lens_transport::control_runtime::{CompletionMailbox, ControlPublications, ControlSender};
use xm1_lens_transport::control_types::*;
use xm1_power_services::software_timer_service::{DetectionIrqToken, DetectionRegister};
use xm1_power_services::storage_shutdown_lifecycle::LensDmaRegister;

struct Services;
impl ControlSender for Services { fn post(&self, _: ControlMessage) -> Result<(), ControlError> { Ok(()) } }
impl ControlServices for Services {
    fn delay_ticks(&mut self, _: TickCount) {}
    fn post(&mut self, _: ControlMessage) -> Result<(), ControlError> { Ok(()) }
    fn complete(&mut self, _: CallbackToken, _: CompletionStatus, _: &[u8]) {}
    fn notify(&mut self, _: WaitTarget) {}
    fn key_release(&mut self, _: KeyEvent) {}
    fn diagnostic(&mut self, _: u32, _: u32) {}
    fn start_timer(&mut self, _: TickCount, _: bool, _: ControlTimer) -> TimerToken { TimerToken(1) }
    fn cancel_timer(&mut self, _: TimerToken) {}
    fn set_detection_callback(&mut self, _: DetectionEdge, _: bool) {}
    fn reset_gpio(&mut self) {}
}
struct Device { input: Cell<u32>, next_irq: Cell<u32> }
impl DetectionIo for Device {
    type SavedCritical = ();
    fn enter(&self) {}
    fn leave(&self, _: ()) {}
    fn write_register(&self, _: DetectionRegister, _: u32) {}
    fn read_register(&self, _: DetectionRegister) -> u32 { self.input.get() }
    fn write_dma_register(&self, _: LensDmaRegister, _: u32) {}
    fn attach_irq(&self, _: DetectionSource) -> Result<DetectionIrqToken, ServiceStatus> {
        let token = self.next_irq.get(); self.next_irq.set(token + 1); Ok(DetectionIrqToken(token))
    }
    fn release_irq(&self, _: DetectionIrqToken) -> Result<(), ServiceStatus> { Ok(()) }
}
fn client(value: u32) -> ClientId { ClientId::new(value).unwrap() }
fn successful(publication: Publication, actuator: &LensActuatorCpu) -> ActuatorCompletion {
    let Publication::Command { callback, operation, data, .. } = publication else { panic!() };
    actuator.publication(Publication::Command { callback, operation, status: Status::Complete, data }).unwrap()
}
fn suspended(publications: &ControlPublications, readiness: &SharedReadiness) {
    let mut actor = ControlActor::new([0; 128], false, readiness);
    actor.apply_mode_completion(ModeCompletion::Suspend, &mut Services);
    publications.publish(&actor);
}

#[test]
fn home_requires_both_command_acknowledgments_and_a_physical_observation() {
    let mailbox = CompletionMailbox::new(); let publications = ControlPublications::new(); let readiness = SharedReadiness::new();
    suspended(&publications, &readiness);
    let task99 = Task99Client::new(&Services, &mailbox, &publications, &readiness);
    let device = Device { input: Cell::new(0), next_irq: Cell::new(1) };
    let state = RefCell::new(DetectionState::new()); let events = ActuatorEvents::new();
    let detection = DetectionProvider::for_actuator(&state, &device, &events); detection.initialize().unwrap();
    let (mut queue, _) = physical_fixture::setup(); let mut actuator = LensActuatorCpu::new();
    let mut context = MotionContext::initialized(ZoomPosition(0)); let mut runtime = IrisRuntime::new();
    assert_eq!(runtime.begin_iris_home(IrisMode::from_abi(0), client(7), &mut context, &mut actuator, &mut queue, &task99, &detection, &events).unwrap(), IrisStart::Waiting);
    let control = successful(queue.pop_publication().unwrap(), &actuator);
    let iris = successful(queue.pop_publication().unwrap(), &actuator);
    assert_eq!(runtime.command(control, &mut context, &mut actuator, &detection, &events, &task99).unwrap(), None);
    assert_eq!(runtime.command(iris, &mut context, &mut actuator, &detection, &events, &task99).unwrap(), None);
    device.input.set(0x80);
    assert!(detection.interrupt_current(DetectionSource::High));
    let observed = actuator.observer_completion(events.take().unwrap()).unwrap();
    let expected = Some(IrisCompletion { client: client(7), publication: IrisPublicationKind::Home423f, service_effect: None });
    assert_eq!(runtime.observed(observed, &mut context, &mut actuator, &detection, &events).unwrap(), expected);
    assert_eq!(runtime.observed(observed, &mut context, &mut actuator, &detection, &events).unwrap(), None);
    assert_eq!(runtime.command(iris, &mut context, &mut actuator, &detection, &events, &task99).unwrap(), None);
    assert!(!context.iris_in_progress);
}

#[test]
fn cancelled_command_cannot_complete_a_later_home_or_replace_its_client() {
    let mailbox = CompletionMailbox::new(); let publications = ControlPublications::new(); let readiness = SharedReadiness::new();
    suspended(&publications, &readiness);
    let task99 = Task99Client::new(&Services, &mailbox, &publications, &readiness);
    let device = Device { input: Cell::new(0), next_irq: Cell::new(1) };
    let state = RefCell::new(DetectionState::new()); let events = ActuatorEvents::new();
    let detection = DetectionProvider::for_actuator(&state, &device, &events); detection.initialize().unwrap();
    let (mut queue, _) = physical_fixture::setup(); let mut actuator = LensActuatorCpu::new();
    let mut context = MotionContext::initialized(ZoomPosition(0)); let mut runtime = IrisRuntime::new();
    runtime.begin_iris_home(IrisMode::from_abi(0), client(1), &mut context, &mut actuator, &mut queue, &task99, &detection, &events).unwrap();
    let stale = successful(queue.pop_publication().unwrap(), &actuator);
    assert_eq!(runtime.begin_iris_home(IrisMode::from_abi(0), client(2), &mut context, &mut actuator, &mut queue, &task99, &detection, &events).unwrap(), IrisStart::Duplicate);
    assert_eq!(runtime.cancel(&mut context, &mut actuator, &mut queue, &detection, &events, &task99).unwrap(), Some(IrisCompletion { client: client(1), publication: IrisPublicationKind::Home423f, service_effect: None }));
    while queue.pop_publication().is_some() {}
    runtime.begin_iris_home(IrisMode::from_abi(0), client(3), &mut context, &mut actuator, &mut queue, &task99, &detection, &events).unwrap();
    assert_eq!(runtime.command(stale, &mut context, &mut actuator, &detection, &events, &task99).unwrap(), None);
    let control = actuator.publication(queue.pop_publication().unwrap()).unwrap();
    let iris = actuator.publication(queue.pop_publication().unwrap()).unwrap();
    assert_eq!(runtime.command(control, &mut context, &mut actuator, &detection, &events, &task99).unwrap(), None);
    assert_eq!(runtime.command(iris, &mut context, &mut actuator, &detection, &events, &task99).unwrap(), Some(IrisCompletion { client: client(3), publication: IrisPublicationKind::Home423f, service_effect: None }));
}

#[test]
fn fast_irq_move_arms_the_owned_periodic_slot_and_accepts_only_its_registered_callback() {
    use xm1_calibration_core::configuration::{ConfigAddress, Configuration, ConfigurationRegion};
    use xm1_power_lifecycle::record_address_space::{CallbackHandler, CallbackKey, CallbackRegistry};
    use std::sync::Mutex;
    use std::vec::Vec;
    struct Sender(Mutex<Vec<ControlMessage>>);
    impl ControlSender for Sender {
        fn post(&self, message: ControlMessage) -> Result<(), ControlError> { self.0.lock().unwrap().push(message); Ok(()) }
    }
    fn completed(_: &mut ()) -> Result<(), xm1_power_lifecycle::record_address_space::RecordError> { Ok(()) }
    let mut registry = CallbackRegistry::<(), 2>::new();
    let callback = registry.register(CallbackKey::new(0x76000001).unwrap(), CallbackHandler::Completion(completed)).unwrap();
    let unrelated = registry.register(CallbackKey::new(0x76000002).unwrap(), CallbackHandler::Completion(completed)).unwrap();
    let mailbox = CompletionMailbox::new(); let publications = ControlPublications::new(); let readiness = SharedReadiness::new();
    suspended(&publications, &readiness);
    let sender = Sender(Mutex::new(Vec::new()));
    let task99 = Task99Client::new(&sender, &mailbox, &publications, &readiness);
    let device = Device { input: Cell::new(0), next_irq: Cell::new(1) };
    let state = RefCell::new(DetectionState::new()); let events = ActuatorEvents::new();
    let detection = DetectionProvider::for_actuator(&state, &device, &events); detection.initialize().unwrap();
    let (mut queue, _) = physical_fixture::setup(); let mut actuator = LensActuatorCpu::new();
    let mut context = MotionContext::initialized(ZoomPosition(0)); context.iris_fast_irq = true;
    let mut runtime = IrisRuntime::new(); runtime.register_timer(callback).unwrap();
    let mut bytes = std::vec![0; 0x3e2742]; bytes[0x3e0f82] = 3;
    let region = ConfigurationRegion::new(ConfigAddress(0), &bytes).unwrap();
    let mut calibration = Calibration::new(Configuration::new(region, ConfigAddress(0)));
    let request = IrisMoveRequest { position: IrisPosition(1), mode: IrisMode::from_abi(1), policy: IrisPolicy::from_abi(1), source: IrisSource::from_abi(4), origin: IrisOrigin::Standard4213 };
    assert_eq!(runtime.begin_iris_move(request, client(9), &mut context, &mut calibration, &mut actuator, &mut queue, &task99, &detection, &events).unwrap(), IrisStart::Waiting);
    assert!(sender.0.lock().unwrap().is_empty());
    let command = successful(queue.pop_publication().unwrap(), &actuator);
    assert_eq!(command.operation, ActuatorOperation::IrisMove);
    assert_eq!(runtime.command(command, &mut context, &mut actuator, &detection, &events, &task99).unwrap(), None);
    assert_eq!(&*sender.0.lock().unwrap(), &[ControlMessage {
        opcode: ControlOpcode::Poll,
        payload: ControlPayload::ConfigurePeriodic { index: PeriodicIndex::new(2).unwrap(), ticks: TickCount(10), modes: PeriodicModes::new(1).unwrap(), callback: Some(CallbackToken(0x76000001)) },
    }]);
    assert_eq!(runtime.periodic_completed(unrelated, &mut context, &mut actuator, &detection, &events, &task99).unwrap(), None);
    assert_eq!(runtime.periodic_completed(callback, &mut context, &mut actuator, &detection, &events, &task99).unwrap(), Some(IrisCompletion { client: client(9), publication: IrisPublicationKind::Move4241, service_effect: None }));
    assert_eq!(sender.0.lock().unwrap().last(), Some(&ControlMessage { opcode: ControlOpcode::Poll, payload: ControlPayload::CancelPeriodic(PeriodicIndex::new(2).unwrap()) }));
    assert_eq!(runtime.periodic_completed(callback, &mut context, &mut actuator, &detection, &events, &task99).unwrap(), None);
}
