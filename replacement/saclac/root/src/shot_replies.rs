#![forbid(unsafe_code)]

use xm1_saclac_interface::{Calc, Calc1, CommandKind, CompletionStatus, ImageProcessor, ImgDc, RecordHandle};
use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_shared::stock::Machine;
use xm1_shot_sequence::{boundary::Replies, ports::Reply, primary_boundary::{CaptureReplies, CaptureStatus}};
use crate::{shot_native::ProfileReplies, shot_services::Libraries};

impl<M: Machine, R, E, P> xm1_saclac_calc::record_policy::AdjustmentPosition for crate::services::Services<M, R, E, P> {
    fn apply(&mut self, selector: u32, position: i16, unmeasured: bool) {
        self.machine.enter(0x74_0c3c, &[selector as usize, position as i32 as usize, usize::from(unmeasured)]);
    }
}

impl<M: Machine + Clone + ProfileReplies, R: Memory, E: StockEntries, P: ImageProcessor + Calc + Calc1 + ImgDc + xm1_saclac_interface::Statistics,
    I: xm1_saclac_imaging::driver::Isp, T: xm1_saclac_profile_geometry::controller::Tables, A, Q>
    Replies for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn reply(&mut self, reply: Reply) {
        let machine = self.services.borrow().machine.clone();
        match reply.command {
            CommandKind::HalfPress => machine.half_press_reply(reply.record, reply.status, |record| {
                let mut services = self.services.borrow_mut();
                let mut geometry = self.geometry.borrow_mut();
                let mut ports = crate::round_ports::RoundPorts::new(&mut services, &mut geometry, self.recognition, record);
                self.recognition.borrow_mut().vote(&mut ports, record, xm1_saclac_calc::scene_recognition::Mode::Capture)
            }, |record| {
                xm1_saclac_calc::record_policy::apply_mode_position(&mut *self.services.borrow_mut(), record);
            }),
            CommandKind::Command07 => machine.capture_reply(reply.record, reply.status),
            CommandKind::Command08 => machine.setting_reply(reply.record, reply.status),
            CommandKind::Command0c | CommandKind::Command28 => crate::controller_replies::scalar_reply(machine, reply.command, reply.status),
            CommandKind::Autofocus => crate::controller_replies::autofocus_reply(machine, reply.status),
            _ => panic!("Shot reply command"),
        }
    }
}

impl<M: Machine + Clone + ProfileReplies, R: Memory, E: StockEntries, P, I, T, A, Q>
    CaptureReplies for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn reply(&mut self, record: RecordHandle, status: CaptureStatus) {
        let machine = self.services.borrow().machine.clone();
        machine.capture_reply(Some(record), CompletionStatus(xm1_shot_sequence::capture_stock::capture_status(status) as i32));
    }
}
