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
