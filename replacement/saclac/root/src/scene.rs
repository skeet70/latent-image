#![forbid(unsafe_code)]

use xm1_saclac_calc::scene_recognition::{ports::ClassificationControl, SceneRecognition};
use xm1_saclac_interface::{calc::SharedAddress, calc_sub::Frame, stock::StockCallbackWord, CalcSub, CalcSubRequest};
use xm1_saclac_shared::stock::Machine;

pub struct ClassificationPorts<'a, M, P> {
    pub machine: &'a M,
    pub peers: &'a mut P,
}
impl<M: Machine, P> ClassificationControl for ClassificationPorts<'_, M, P> {
    fn enable_classification(&mut self) { self.machine.enter(0xfb_236c, &[0]); }
}
impl<M, P: CalcSub> CalcSub for ClassificationPorts<'_, M, P> {
    type Error = P::Error;
    fn request(&mut self, request: CalcSubRequest<'_>) -> Result<(), Self::Error> { self.peers.request(request) }
}

/// The retained 0xf4eae0 argument order. Its caller ignores the post result.
pub fn request_classification<M: Machine, P: CalcSub>(state: &mut SceneRecognition, ports: &mut ClassificationPorts<'_, M, P>, arguments: [u32; 11]) {
    let [samples, plane, aspect, orientation, levels, columns, rows, average_levels, motion_regions, region_list, completion] = arguments;
    let frame = Frame {
        samples: SharedAddress(samples), plane: SharedAddress(plane), levels: SharedAddress(levels),
        columns: SharedAddress(columns), rows: SharedAddress(rows), average_levels, motion_regions,
        region_list: SharedAddress(region_list),
    };
    let _ = state.request_classification(ports, frame, [(aspect >> 16) as u16, aspect as u16], orientation, StockCallbackWord(completion));
}
