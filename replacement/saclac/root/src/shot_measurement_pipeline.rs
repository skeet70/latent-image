#![forbid(unsafe_code)]

use xm1_saclac_imaging::{capture::pipeline::StockBlock, stock::statistics};
use xm1_saclac_interface::{area_measurement::{MeasuredAreas, MeasurementRequest, TransferRequest}, stock::{self, area_measurement as wire}, Completion, ImgDc, ImgDcOperation, ImgDcRequest};
use xm1_saclac_shared::stock::Machine;
use xm1_shot_sequence::{calculations_work::Processing, capture_stock, secondary_boundary::{MeasurementPipeline, MeasurementWord, StatisticsStore}};
use crate::shot_services::Libraries;

impl<M: Machine + Clone, R, E, P, I, T, A, Q: ImgDc> MeasurementPipeline for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn prepare_statistics(&mut self, source: u8, store: StatisticsStore, processing: &mut Processing) -> bool {
        // Both native stores cover0x1140 bytes and overlap. Earlier asynchronous readers
        // must finish before reuse; a synchronous workspace borrow alone does not order them.
        statistics::prepare_statistics(source, StockBlock(capture_stock::statistics_store(store)), &mut processing.0)
    }
    fn measurement_word(&mut self, word: MeasurementWord) -> u32 {
        let offset = match word {
            MeasurementWord::FirstList => 0x94, MeasurementWord::SecondList => 0x98,
            MeasurementWord::DumpFirst => 0x9c, MeasurementWord::DumpSecond => 0xa0, MeasurementWord::DumpThird => 0xa4,
        };
        self.services.borrow().machine.read_u32(0x016f_0f5c + offset)
    }
    fn release_measurement(&mut self) {
        // feabf0 -> fedb18 -> the imaging task's actual step-timer cancellation, index0.
        self.services.borrow().machine.enter(0xfe_c984, &[0]);
    }
    fn measure_areas(&mut self, request: &MeasurementRequest, areas: &MeasuredAreas, completions: [Completion; 2]) {
        assert_eq!(completions, [Completion::Image5b, Completion::Image5c]);
        let at = self.capture.borrow_mut().stage_areas(areas).expect("persistent measured areas must fit the guest address space");
        let mut payload = [0; 116];
        payload[..4].copy_from_slice(&self.callbacks.measurement.finished.0.to_le_bytes());
        payload[4..8].copy_from_slice(&self.callbacks.measurement.ready.0.to_le_bytes());
        payload[8..].copy_from_slice(&wire::measurement_request(request, at));
        assert!(self.peers.request(ImgDcRequest { operation: ImgDcOperation::Request46, parameters: stock::parameters(&payload), completions: &completions }).is_ok(), "area measurement enqueue");
    }
    fn transfer(&mut self, request: &TransferRequest, completion: Completion) {
        assert_eq!(completion, Completion::Image5d);
        let mut payload = [0; 8];
        payload[..4].copy_from_slice(&self.callbacks.measurement.transferred.0.to_le_bytes());
        payload[4..].copy_from_slice(&wire::transfer_request(request));
        assert!(self.peers.request(ImgDcRequest { operation: ImgDcOperation::Request47, parameters: stock::parameters(&payload), completions: &[completion] }).is_ok(), "area transfer enqueue");
    }
}
