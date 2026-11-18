#![forbid(unsafe_code)]

use xm1_saclac_imaging::{capture::pipeline::StockBlock, stock::statistics};
use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_shared::stock::Machine;
use xm1_shot_sequence::{calculations_work::{CaptureProcessing, Processing, ProcessingSelection, StatisticsPlanes}, stock::calculations};
use crate::shot_services::Libraries;

impl<M: Machine + Clone, R: Memory, E: StockEntries, P, I, T, A, Q> CaptureProcessing for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn configure(&mut self, source: u8, with_statistics: bool, processing: &mut Processing, client: u32, detail: u32, selection: &ProcessingSelection) -> u32 {
        let mut selection = calculations::selection_bytes(selection);
        // Completed captures and raw stores belong to the imaging tasks. The owned configuration
        // operates on those stores; this borrow serializes its synchronous overlapping scratch.
        let mut workspace = self.capture.borrow_mut();
        u32::from(statistics::configure_processing(
            &mut workspace.configuration, source, with_statistics.then_some(StockBlock(calculations::STATISTICS)),
            &mut processing.0, client, (detail != 0).then_some(StockBlock(detail)), &mut selection,
        ))
    }
    fn statistics(&mut self) -> StatisticsPlanes {
        let services = self.services.borrow();
        core::array::from_fn(|plane| core::array::from_fn(|cell| {
            services.machine.read_u32(calculations::STATISTICS + ((plane * 256 + cell) * 4) as u32)
        }))
    }
}
