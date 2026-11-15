use xm1_kernel::{Backend, Header, MessageQueue, PostError, QueueId};
use xm1_saclac_interface::{stock::{callback_word, completion_code, optional_callback, PARAMETER_BLOCKS}, Completion};
use xm1_saclac_peers::Poster as _;
use xm1_saclac_shared::stock::Machine;
use crate::peer_context::Poster;

/// Queue delivery only; never synchronously re-enters a shot or controller.
pub fn post<B: Backend>(poster: &Poster<B>, completion: Completion, payload: &[u8]) -> Result<(), PostError> {
    let queue = MessageQueue::stock(QueueId::new(43).expect("saclac queue"));
    let header = Header {
        operation: 0x3b00 + u16::from(completion_code(completion)),
        sender: poster.running_task().map_or(0xff, |task| task.get()),
        destination: 43,
    };
    poster.post(queue, header, payload)
}

/// The statistics progress callback reads only its publication and posts calc0x3c12.
pub fn pipeline_statistics_progress<B: Backend>(poster: &Poster<B>, gate: &crate::statistics_results::ReportingGate) -> Result<(), PostError> {
    if !gate.is_open() { return Ok(()); }
    let route = xm1_saclac_interface::stock::peers::CALC;
    let (operation, payload) = xm1_saclac_interface::stock::calc_payload(&xm1_saclac_interface::calc::CalcPayload::Release);
    let queue = MessageQueue::stock(QueueId::new(route.queue.number()).expect("calc queue"));
    poster.post(queue, Header {
        operation: xm1_saclac_interface::stock::calc_operation(operation),
        sender: route.sender.byte(poster.running_task()),
        destination: route.queue.number(),
    }, payload.bytes())
}

/// The f2c9c4 boundary: status plus three supplied words, regardless of status.
pub fn primary_end<B: Backend>(poster: &Poster<B>, status: u32, result: [u32; 3]) -> Result<(), PostError> {
    let mut payload = [0; 16];
    for (bytes, word) in payload.chunks_exact_mut(4).zip([status, result[0], result[1], result[2]]) {
        bytes.copy_from_slice(&word.to_le_bytes());
    }
    post(poster, Completion::Image5e, &payload)
}

/// The independent f2c918 boundary: command07's status-8 notification, with no post.
/// efab18 has no record publication or command-ending effect for status 8.
pub fn primary_notice(machine: &impl Machine, status: u32) {
    if status != 0 { return; }
    if let Some(callback) = optional_callback(machine.read_u32(PARAMETER_BLOCKS[7].address)) {
        machine.enter(callback_word(callback), &[8]);
    }
}

/// f2903c/f2cad8: status followed by five pointed-to words. The primary callback's dump
/// precedes those reads, so an aliased result observes that native publication order.
pub fn capture_setting<B: Backend, M: Machine + Clone>(poster: &Poster<B>, machine: &M, completion: Completion, status: u32, result: u32) -> Result<(), PostError> {
    assert!(matches!(completion, Completion::Statistics85 | Completion::Statistics7d));
    if completion == Completion::Statistics7d && machine.enter(0x69_f8fc, &[0xa8]) & 0x80 != 0 {
        let sources = machine.read_u32(PARAMETER_BLOCKS[7].address + 0xc);
        let frame = machine.read_u32(sources.wrapping_add(xm1_shot_sequence::capture_stock::SOURCE_FRAME));
        let mut dumps = crate::geometry::GeometryBindings {
            stages: crate::stages::StageDriver { isp: (), arguments: crate::stock::stages::StageMemory, machine: machine.clone() },
            peers: (),
        };
        xm1_saclac_profile_geometry::controller::dumps::dump_result(&mut dumps, (frame != 0).then_some(xm1_saclac_profile_geometry::ports::DumpBuffer(frame)));
    }
    let mut payload = [0; 24];
    payload[..4].copy_from_slice(&status.to_le_bytes());
    for (index, out) in payload[4..].chunks_exact_mut(4).enumerate() {
        out.copy_from_slice(&machine.read_u32(result.wrapping_add(4 * index as u32)).to_le_bytes());
    }
    post(poster, completion, &payload)
}

/// f15070 posts the halfword and mode; its unused alignment halfword is zero here.
pub fn lens_status<B: Backend>(poster: &Poster<B>, position: u16, mode: u32) -> Result<(), PostError> {
    let mut payload = [0; 8];
    payload[..2].copy_from_slice(&position.to_le_bytes());
    payload[4..].copy_from_slice(&mode.to_le_bytes());
    post(poster, Completion::Lens3c, &payload)
}

/// f22730..f22760 reads the closed word at 16b5a40+74, then posts its one argument.
pub fn s1_result<B: Backend>(poster: &Poster<B>, gate: &crate::statistics_results::ReportingGate, value: u32) -> Result<(), PostError> {
    if gate.is_open() { post(poster, Completion::Statistics7f, &value.to_le_bytes()) } else { Ok(()) }
}

struct ExposureCallback<'a, M, B: Backend> {
    machine: &'a M,
    poster: &'a Poster<B>,
    result: Result<(), PostError>,
}
impl<M: Machine, B: Backend> xm1_shot_sequence::exposure_callbacks::CallbackPort for ExposureCallback<'_, M, B> {
    fn trace(&mut self, trace: xm1_shot_sequence::exposure_callbacks::CallbackTrace) {
        self.machine.enter(0x72872c, &[xm1_shot_sequence::stock::exposure::callback_trace_word(trace) as usize]);
    }
    fn post(&mut self, completed: xm1_saclac_interface::Completed<'_>) {
        self.result = post(self.poster, completed.completion, completed.parameters.bytes());
    }
}

fn exposure_callback<M: Machine, B: Backend>(machine: &M, poster: &Poster<B>, invoke: impl FnOnce(&mut ExposureCallback<'_, M, B>)) -> Result<(), PostError> {
    let mut callback = ExposureCallback { machine, poster, result: Ok(()) };
    invoke(&mut callback);
    callback.result
}

pub fn exposure_statistics<M: Machine, B: Backend>(machine: &M, poster: &Poster<B>, status: u32, entry: u32) -> Result<(), PostError> {
    exposure_callback(machine, poster, |port| xm1_shot_sequence::exposure_callbacks::statistics_completed(status, entry, port))
}

pub fn exposure_evaluation<M: Machine, B: Backend>(machine: &M, poster: &Poster<B>, evaluation: &[u8; xm1_saclac_interface::exposure::EVALUATION_BYTES]) -> Result<(), PostError> {
    exposure_callback(machine, poster, |port| xm1_shot_sequence::exposure_callbacks::evaluation_completed(evaluation, port))
}

pub fn exposure_correction<M: Machine, B: Backend>(machine: &M, poster: &Poster<B>, correction: Option<&[u8; xm1_saclac_interface::stock::exposure::CORRECTION_BYTES]>) -> Result<(), PostError> {
    exposure_callback(machine, poster, |port| xm1_shot_sequence::exposure_callbacks::correction_completed(correction, port))
}

pub fn pipeline_raw_check<B: Backend>(poster: &Poster<B>, value: u32) -> Result<(), PostError> {
    let payload = xm1_saclac_interface::stock::pipeline::raw_check_payload(xm1_saclac_interface::pipeline::RawCheck(value));
    post(poster, Completion::Image4e, &payload)
}

pub fn pipeline_tone_progress<B: Backend>(poster: &Poster<B>, status: u32, level: u16) -> Result<(), PostError> {
    let payload = xm1_saclac_interface::stock::pipeline::tone_progress_payload(xm1_saclac_interface::pipeline::ToneProgress { status, level });
    post(poster, Completion::Image50, &payload)
}

pub fn pipeline_tone_status<B: Backend>(poster: &Poster<B>, value: u32) -> Result<(), PostError> {
    let payload = xm1_saclac_interface::stock::pipeline::tone_status_payload(xm1_saclac_interface::pipeline::ToneStatus(value));
    post(poster, Completion::Image51, &payload)
}

pub fn pipeline_raw_calculation<B: Backend>(poster: &Poster<B>, result: &xm1_saclac_interface::pipeline::RawCalculation) -> Result<(), PostError> {
    post(poster, Completion::Calculation91, &result.0)
}

pub fn pipeline_tone_calculation<B: Backend>(poster: &Poster<B>, result: &xm1_saclac_interface::pipeline::ToneCalculation) -> Result<(), PostError> {
    post(poster, Completion::Calculation92, &result.0)
}

pub fn pipeline_gradation_calculation<B: Backend>(poster: &Poster<B>, result: &xm1_saclac_interface::pipeline::GradationCalculation) -> Result<(), PostError> {
    post(poster, Completion::Calculation93, &result.0)
}
