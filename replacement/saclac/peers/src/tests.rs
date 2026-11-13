use xm1_kernel::fake::{Call, Fake};
use xm1_kernel::{Context, Form, Header, Kernel, PostError, QueueId, TaskId};
use xm1_saclac_interface::calc::{Calc1Payload, CalcPayload, Record, SharedAddress};
use xm1_saclac_interface::lens::{DriveOwner, LensPayload, ProfileSlotReference};
use xm1_saclac_interface::shared::{ImageProcessorTriplet, LensComCode, LensComCommand};
use xm1_saclac_interface::statistics::StatisticsPayload;
use xm1_saclac_interface::stock::{self, StockCallbackWord};
use xm1_saclac_interface::{
    Calc, Calc1, Calc1Request, CalcRequest, Flash, FlashOperation, FlashRequest, ImageProcessor, ImageProcessorOperation,
    ImageProcessorRequest, ImgDc, ImgDcOperation, ImgDcRequest, LensCom, LensComOperation, LensComRequest, LensDc,
    LensDcOperation, LensDcRequest, LensEffect, ParameterValue, Statistics, StatisticsRequest,
};

use super::{EmulatorOnly, LensDriveRefused, PeerError, PeerPorts};

static MARKER: u32 = 1;
static UNMARKED: u32 = 0;

const SACLAC: u8 = 43;

fn emulator_only() -> EmulatorOnly { EmulatorOnly::new(&MARKER).unwrap() }

fn saclac_task() -> Kernel<Fake> { Kernel::new(Fake::with_task(TaskId::new(SACLAC).unwrap())) }

fn posts(kernel: &Kernel<Fake>) -> Vec<Call> {
    kernel.backend().calls().into_iter().filter(|call| matches!(call, Call::Post { .. })).collect()
}

fn post(queue: u8, form: Form, operation: u16, sender: u8, payload: &[u8]) -> Call {
    Call::Post {
        id: QueueId::new(queue).unwrap(),
        form,
        header: Header { operation, sender, destination: queue },
        payload: payload.to_vec(),
    }
}

fn words(values: &[u32]) -> Vec<u8> { values.iter().flat_map(|value| value.to_le_bytes()).collect() }

#[test]
fn lens_drive_posts_its_payload_to_lensdc() {
    let kernel = saclac_task();
    let cx = kernel.task().unwrap();
    let payload = LensPayload::Drive {
        owner: DriveOwner::Live,
        parameters: [ParameterValue(1), ParameterValue(2), ParameterValue(3), ParameterValue(4)],
        slot: ProfileSlotReference(0x16b_5000),
    };
    let mut bytes = [0; stock::LENS_PAYLOAD_BYTES];
    LensDc::request(&mut PeerPorts::new(&cx, emulator_only()), stock::lens_dc_request(&payload, &mut bytes)).unwrap();
    assert_eq!(posts(&kernel), [post(50, Form::Task, 0x4207, SACLAC, &words(&[0xf14be4, 1, 2, 3, 4, 0x16b_5000]))]);
}

#[test]
fn calc_and_calc1_jobs_post_the_encoded_message() {
    let kernel = saclac_task();
    let cx = kernel.task().unwrap();
    let mut ports = PeerPorts::new(&cx, LensDriveRefused);
    let (operation, message) = stock::calc_payload(&CalcPayload::Calculation {
        block: SharedAddress(0x16b_5428),
        source: SharedAddress(0x16b_665c),
        callback: StockCallbackWord(0xf1d154),
    });
    Calc::request(&mut ports, CalcRequest { operation, parameters: stock::parameters(message.bytes()), completions: &[] }).unwrap();
    let (operation, message) = stock::calc1_payload(&Calc1Payload::Selection { record: Record([7; 228]), callback: StockCallbackWord(0xf37854) });
    Calc1::request(&mut ports, Calc1Request { operation, parameters: stock::parameters(message.bytes()), completions: &[] }).unwrap();

    let mut calculation = words(&[0xf1d154, 0x16b_5428, 0x16b_665c]);
    calculation.resize(232, 0);
    let mut selection = vec![7; 228];
    selection.extend(0xf37854u32.to_le_bytes());
    assert_eq!(
        posts(&kernel),
        [post(44, Form::Task, 0x3c02, SACLAC, &calculation), post(45, Form::Task, 0x3d01, SACLAC, &selection)]
    );
}

#[test]
fn statistics_imaging_flash_lenscom_and_image_processor_requests_reach_their_queues() {
    let kernel = saclac_task();
    let cx = kernel.task().unwrap();
    let mut ports = PeerPorts::new(&cx, emulator_only());

    let (operation, message) = stock::statistics::statistics_payload(&StatisticsPayload::Stop {
        pause: true,
        shot_idle: false,
        callback: StockCallbackWord(0xf2f6ac),
    });
    Statistics::request(&mut ports, StatisticsRequest { operation, parameters: stock::parameters(message.bytes()), completions: &[] }).unwrap();
    let adjustment = words(&[0xf2f684, 5, 6, 7, 8]);
    ImgDc::request(&mut ports, ImgDcRequest { operation: ImgDcOperation::Request34, parameters: stock::parameters(&adjustment), completions: &[] }).unwrap();
    let mask = words(&[4]);
    Flash::request(&mut ports, FlashRequest { operation: FlashOperation::Request0e, parameters: stock::parameters(&mask), completions: &[] }).unwrap();
    let command = stock::shared::lens_com(&LensComCommand { code: LensComCode::new(2).unwrap(), completion: StockCallbackWord(0x6bbf98) });
    LensCom::request(&mut ports, LensComRequest { operation: LensComOperation::Request2d, parameters: stock::parameters(&command), completions: &[] }).unwrap();
    let triplet = stock::shared::image_processor_triplet(&ImageProcessorTriplet { mode: 1, values: [2, 3, 4] });
    ImageProcessor::request(&mut ports, ImageProcessorRequest { operation: ImageProcessorOperation::Request03, parameters: stock::parameters(&triplet) }).unwrap();

    assert_eq!(
        posts(&kernel),
        [
            post(48, Form::Task, 0x4007, SACLAC, &words(&[1, 0, 0xf2f6ac])),
            post(47, Form::Task, 0x3f34, SACLAC, &adjustment),
            post(10, Form::Task, 0x1a0e, SACLAC, &mask),
            post(98, Form::Task, 0x722d, SACLAC, &command),
            post(55, Form::Task, 0x4703, SACLAC, &words(&[1, 2, 3, 4])),
        ]
    );
}

#[test]
fn a_refused_post_is_returned_after_one_attempt() {
    let kernel = Kernel::new(Fake::with_task(TaskId::new(SACLAC).unwrap()).post_result(Err(PostError::NoBlock)));
    let cx = kernel.task().unwrap();
    let request = LensDcRequest { operation: LensDcOperation::Request12, parameters: stock::parameters(&[]), completions: &[] };
    assert_eq!(LensDc::request(&mut PeerPorts::new(&cx, emulator_only()), request), Err(PeerError::Refused(PostError::NoBlock)));
    assert_eq!(posts(&kernel), [post(50, Form::Task, 0x4212, SACLAC, &[])]);
}

#[test]
fn lens_reaching_requests_are_not_posted_without_the_emulator_marker() {
    let kernel = saclac_task();
    let cx = kernel.task().unwrap();
    let mut ports = PeerPorts::new(&cx, LensDriveRefused);
    let drive = LensDcRequest { operation: LensDcOperation::Request07, parameters: stock::parameters(&[0; 24]), completions: &[] };
    assert_eq!(LensDc::request(&mut ports, drive), Err(PeerError::LensSafetyContract(LensEffect::DirectDrive)));
    let bridge = ImgDcRequest { operation: ImgDcOperation::Request2e, parameters: stock::parameters(&[0; 44]), completions: &[] };
    assert_eq!(ImgDc::request(&mut ports, bridge), Err(PeerError::LensSafetyContract(LensEffect::DriveThroughImageBridge)));
    let flash = FlashRequest { operation: FlashOperation::Request0d, parameters: stock::parameters(&[0; 8]), completions: &[] };
    assert_eq!(Flash::request(&mut ports, flash), Err(PeerError::LensSafetyContract(LensEffect::Unclassified)));
    assert_eq!(posts(&kernel), []);
}

#[test]
fn only_a_marker_of_one_grants_lens_access() {
    assert!(EmulatorOnly::new(&UNMARKED).is_none());
}

#[test]
fn handler_context_posts_in_handler_form_with_each_sender_byte_rule() {
    let kernel = Kernel::new(Fake::new().in_handler());
    let Context::Interrupt(cx) = kernel.context() else { panic!("not handler context") };
    let mut ports = PeerPorts::new(&cx, emulator_only());
    LensDc::request(&mut ports, LensDcRequest { operation: LensDcOperation::Request09, parameters: stock::parameters(&[]), completions: &[] }).unwrap();
    let release = StatisticsRequest {
        operation: xm1_saclac_interface::StatisticsOperation::Request0c,
        parameters: stock::parameters(&[]),
        completions: &[],
    };
    Statistics::request(&mut ports, release).unwrap();
    let triplet = [0; 16];
    ImageProcessor::request(&mut ports, ImageProcessorRequest { operation: ImageProcessorOperation::Request03, parameters: stock::parameters(&triplet) }).unwrap();
    assert_eq!(
        posts(&kernel),
        [
            post(50, Form::Handler, 0x4209, 0xff, &[]),
            post(48, Form::Handler, 0x400c, 0, &[]),
            post(55, Form::Handler, 0x4703, 0xff, &triplet),
        ]
    );
}
