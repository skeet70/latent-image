#![forbid(unsafe_code)]

use xm1_saclac_interface::{capture_job::{CaptureRequest, DisplayRequest}, stock::{self, capture_job as wire}, CommandKind, Completion, ImgDc, ImgDcOperation, ImgDcRequest};
use xm1_saclac_shared::stock::{Argument::{Buffer, Word}, Machine};
use xm1_shot_sequence::{calculations_work::Processing, primary_boundary::{CaptureCompletions, CapturePipeline}};
use crate::{shot_services::Libraries, stock::acquisition::Aligned};

fn image<Q: ImgDc>(peers: &mut Q, operation: ImgDcOperation, payload: &[u8], completions: &[Completion]) {
    assert!(peers.request(ImgDcRequest { operation, parameters: stock::parameters(payload), completions }).is_ok(), "capture image enqueue");
}
impl<M: Machine + Clone, R, E, P, I, T, A, Q: ImgDc> CapturePipeline for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn capture(&mut self, request: &CaptureRequest, completions: CaptureCompletions) {
        let callbacks = self.callbacks.capture_image;
        // feac00: end, optional outputs, optional readout, optional delivery, complete,
        // independent client notice, then the152-byte request. The middle two register
        // arguments are deliberately stored in their native reversed order.
        let words = [callbacks.end.0, if completions.outputs { callbacks.outputs.0 } else { 0 },
            if completions.readout { callbacks.readout.0 } else { 0 },
            if completions.delivery { callbacks.delivery.0 } else { 0 }, callbacks.complete.0, callbacks.notice.0];
        let mut payload = [0; 176];
        for (out, word) in payload[..24].chunks_exact_mut(4).zip(words) { out.copy_from_slice(&word.to_le_bytes()); }
        payload[24..].copy_from_slice(&wire::capture_request(request));
        let mut expected = [Completion::Image5e; 6];
        let mut count = 1;
        if completions.outputs { expected[count..count + 2].copy_from_slice(&[Completion::Image5f, Completion::Image60]); count += 2; }
        if completions.readout { expected[count] = Completion::Image61; count += 1; }
        if completions.delivery { expected[count] = Completion::Image64; count += 1; }
        expected[count] = Completion::Image62;
        image(&mut self.peers, ImgDcOperation::Request49, &payload, &expected[..count + 1]);
    }
    fn request_command_c(&mut self) {
        let machine = self.services.borrow().machine.clone();
        let mut payload = [0; 12];
        payload[..4].copy_from_slice(&self.callbacks.capture_image.command_c.0.to_le_bytes());
        payload[4..8].copy_from_slice(&crate::controller_replies::parameter_word(&machine, CommandKind::Command0c, 1).to_le_bytes());
        payload[8..10].copy_from_slice(&(crate::controller_replies::parameter_word(&machine, CommandKind::Command0c, 2) as u16).to_le_bytes());
        image(&mut self.peers, ImgDcOperation::Request4a, &payload, &[]);
    }
    fn request_command_28(&mut self) {
        image(&mut self.peers, ImgDcOperation::Request4f, &self.callbacks.capture_image.command_28.0.to_le_bytes(), &[]);
    }
    fn display(&mut self, request: &DisplayRequest) {
        image(&mut self.peers, ImgDcOperation::Request4e, &wire::display_request(request), &[]);
    }
    fn readout_processing(&mut self, source: u8) -> Option<Processing> {
        // feacd4 passes an unused raw-source output, the processing output, and a null
        // record-block output to the actual completed-capture primitive ffcfd0.
        let mut bytes = Aligned([0; 4 + 0x98]);
        let found = self.services.borrow().machine.enter_with(0xff_cfd0, &[Word(u32::from(source)), Buffer(0), Buffer(4), Word(0)], &mut bytes.0);
        (found != 0).then(|| Processing(bytes.0[4..].try_into().expect("completed processing extent")))
    }
    fn keeps_template(&mut self, subject_source: u32, options: u32, mode: u8, profile_class: u32) -> bool {
        xm1_saclac_imaging::capture::requests::request_enabled(subject_source, options, u32::from(mode), profile_class)
    }
    fn statistics_ready(&mut self) -> bool { self.capture.borrow().statistics_ready() }
}
