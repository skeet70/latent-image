use super::{ConfigurationRuntime, Error, Progress, Request};
use crate::backend::ConfigImages;
use crate::configuration_actions_high::{CallbackState, DisplayCompletionCode, PageBufferAddress, PageIdentifier};
use crate::configuration_actions_low::{ActionCompletion, CpuReturn};
use crate::configuration_actions_middle::{MiddleAction, MiddleArguments};
use crate::configuration_diagnostic_registers::DiagnosticTick;
use crate::configuration_i2c_actor::{RegisterAddress, RegisterIo};
use crate::configuration_serial_actors::{PeripheralFlags, PeripheralStatusBytes};
use crate::lens_queue_runtime::{packets::CallbackId, physical_fixture::setup};
use xm1_lens_transport::control_actor::ControlActor;
use xm1_lens_transport::control_types::SharedReadiness;

struct Registers;
impl RegisterIo for Registers {
    fn read(&mut self, _: RegisterAddress) -> u32 { panic!("unexpected physical register read") }
    fn write(&mut self, _: RegisterAddress, _: u32) { panic!("unexpected physical register write") }
}

fn callbacks() -> CallbackState {
    CallbackState { result: CpuReturn::SUCCESS, high_byte: 0, low_byte: 0, page: PageIdentifier(0), page_buffer: PageBufferAddress(0), write_page: PageIdentifier(0), display_result: DisplayCompletionCode(0) }
}

#[test]
fn dispatcher_delivers_real_queue_property_publication_to_configuration() {
    let (mut queue, _) = setup();
    let readiness = SharedReadiness::new();
    let device = ControlActor::new([0; 128], false, &readiness);
    let mut runtime = ConfigurationRuntime::adopt(PeripheralStatusBytes([0; 9]), PeripheralFlags([0; 3]), CallbackId(7));
    let mut bytes = [0xa5; 256];
    let mut images = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
    let mut callbacks = callbacks();
    let request = Request::Middle { action: MiddleAction::parse(64).unwrap(), arguments: MiddleArguments::from_abi(&[0x81, 0, 0x82, 0, 0x83, 0, 0x84, 0]) };
    assert_eq!(runtime.begin(request, &mut images, &mut Registers, &device, &mut callbacks, &mut queue).unwrap(), Progress::Waiting);
    let unrelated = Request::Middle { action: MiddleAction::parse(36).unwrap(), arguments: MiddleArguments::from_abi(&[]) };
    assert!(matches!(runtime.begin(unrelated, &mut images, &mut Registers, &device, &mut callbacks, &mut queue), Err(Error::Busy)));
    let publication = queue.pop_publication().unwrap();
    let completion = runtime.publication(publication, &mut images, &device, &mut callbacks, 0, DiagnosticTick(0), &mut queue).unwrap().unwrap();
    assert_eq!(completion.return_code, CpuReturn::REJECTED);
    assert_eq!(completion.notification, ActionCompletion::service_success().notification);
    assert_eq!(images.byte(0x82), Some(0));
    assert_eq!(images.byte(0x83), Some(0));
    assert_eq!(images.byte(0x84), Some(1));
    assert_eq!(runtime.begin(unrelated, &mut images, &mut Registers, &device, &mut callbacks, &mut queue).unwrap(), Progress::Completed(ActionCompletion::service_success()));
}

#[test]
fn record_parameter_completes_without_device_or_register_access() {
    let (mut queue, _) = setup();
    let readiness = SharedReadiness::new();
    let device = ControlActor::new([0; 128], false, &readiness);
    let mut runtime = ConfigurationRuntime::adopt(PeripheralStatusBytes([0; 9]), PeripheralFlags([0; 3]), CallbackId(7));
    let mut bytes = [0xa5; 256];
    let mut images = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
    for action in [71, 73] {
        let request = Request::Middle { action: MiddleAction::parse(action).unwrap(), arguments: MiddleArguments::from_abi(&[0x81, 0, 0x82, 0]) };
        assert_eq!(runtime.begin(request, &mut images, &mut Registers, &device, &mut callbacks(), &mut queue).unwrap(), Progress::Completed(ActionCompletion::service_success()));
        assert_eq!(images.byte(0x82), Some(0));
        assert_eq!(queue.pop_publication(), None);
    }
}

#[test]
fn unrelated_lifecycle_publication_does_not_complete_a_lens_transaction() {
    let (mut queue, _) = setup();
    let readiness = SharedReadiness::new();
    let device = ControlActor::new([0; 128], false, &readiness);
    let mut runtime = ConfigurationRuntime::adopt(PeripheralStatusBytes([0; 9]), PeripheralFlags([0; 3]), CallbackId(7));
    let mut bytes = [0; 256];
    let mut images = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
    let mut callbacks = callbacks();
    let request = Request::Transfer(crate::configuration_actions_high::lens::TransferQuery::Word);
    assert_eq!(runtime.begin(request, &mut images, &mut Registers, &device, &mut callbacks, &mut queue).unwrap(), Progress::Waiting);
    let unrelated = crate::lens_queue_runtime::Publication::Lifecycle { callback: CallbackId(99) };
    assert_eq!(runtime.publication(unrelated, &mut images, &device, &mut callbacks, 0, DiagnosticTick(0), &mut queue).unwrap(), None);
    let publication = queue.pop_publication().unwrap();
    assert_eq!(runtime.publication(publication, &mut images, &device, &mut callbacks, 0, DiagnosticTick(0), &mut queue).unwrap(), Some(ActionCompletion::service_rejected()));
}

#[test]
fn dispatcher_publishes_plane_samples_and_preserves_an_absent_secondary() {
    use crate::configuration_actions_imaging::planes::{PlaneDimension, PlaneSamples, RecordPlanes};
    let mut runtime = ConfigurationRuntime::adopt(PeripheralStatusBytes([0; 9]), PeripheralFlags([0; 3]), CallbackId(7));
    let mut bytes = [0xa5; 256];
    bytes[0x81] = 0;
    let mut accumulators = [0; 1024];
    accumulators[0] = 8;
    accumulators[256] = 16;
    accumulators[512] = 24;
    accumulators[768] = 32;
    let mut images = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
    let primary = PlaneSamples { accumulators: &accumulators, width: PlaneDimension(4), height: PlaneDimension(2) };
    assert_eq!(runtime.plane_samples(&mut images, RecordPlanes { primary: Some(primary), secondary: None }).unwrap(), Progress::Completed(ActionCompletion::service_success()));
    let output: [u8; 16] = core::array::from_fn(|index| images.byte(0x83 + index as u32).unwrap());
    assert_eq!(output, [0, 8, 0, 4, 0, 16, 0, 12, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5]);
}
