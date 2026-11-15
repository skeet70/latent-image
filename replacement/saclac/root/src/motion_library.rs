#![forbid(unsafe_code)]

use xm1_saclac_calc::{adjust::{motion::{ports::{LensTables, MeasurementReadings}, plan::PlanKind, plan_result::FocusPublication, sampling::SpeedStep, state::MotionState}, rate_samples::{self, RecorderSetup}}, exposure::ports::Placement, stock::{measurement::{MeasurementRecords, RECORDER_RESULTS}, motion as native_motion}};
use xm1_saclac_imaging::driver::Isp;
use xm1_saclac_interface::{calc::{CalcPayload, Record, SharedAddress}, pipeline::{GradationRequest, ToneRequest, TONE_LEVELS}, Calc, Calc1, Completion, ImageProcessor, ImgDc, RecordHandle};
use xm1_saclac_profile_geometry::controller::Tables;
use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_shared::stock::Machine;
use xm1_shot_sequence::{boundary::{LensPoint, LensReference}, pipeline_work::{AdjustmentPass, FocusResultOut, FrameFlags, PhaseAttribute, PipelineLibrary, Plan, PlanBlock, PlanInput, ResultKind, SampleSnapshot}, subjects::Candidates};
use crate::{calculation_services::Calculation, motion_plan as blocks, motion_projection, round_ports::RoundPorts, shot_services::Libraries};

const ADJUSTMENT: [u32; 7] = [0x16d7870, 0x16d7908, 0x16d79cc, 0x16d7a38, 0x16d7a50, 0x16d7a54, 0x16d7ae4];
const RAW_SECOND: u32 = 0x16d84dc;

fn speed_step(snapshot: &SampleSnapshot, index: usize) -> SpeedStep {
    let at = 8 * index;
    SpeedStep { frame: snapshot.0[at], speed: u32::from_le_bytes(snapshot.0[at + 4..at + 8].try_into().unwrap()) }
}
fn records(addresses: &[PlanBlock; 7]) -> MeasurementRecords {
    assert_eq!(addresses.map(|at| at.0), ADJUSTMENT);
    let [map, checks, selection, peak, threshold, weights, search] = addresses.map(|at| SharedAddress(at.0));
    MeasurementRecords { map, checks, selection, peak, threshold, weights, search }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P: ImageProcessor, I: Isp, T: Tables, A: Placement, Q>
    Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn with_motion<U>(&mut self, body: impl FnOnce(&mut MotionState, &mut Calculation<'_, M, R, E, P, I, T, A>) -> U) -> U {
        let mut state = self.motion.borrow_mut();
        body(state.as_mut().expect("motion operation before preparation"), &mut Calculation {
            services: &mut self.services.borrow_mut(), geometry: &mut self.geometry.borrow_mut(), placement: &mut *self.placement.borrow_mut(),
        })
    }
}
impl<M: Machine + Clone, R, E, P, I, T, A, Q: Calc + Calc1> Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn post_pipeline_calculation(&mut self, payload: &CalcPayload) {
        let mut machine = self.services.borrow().machine.clone();
        let mut peers = crate::acquisition::CalculationPeers { machine: &mut machine, peers: &mut self.peers };
        assert!(xm1_saclac_calc::requests::Peers::calc(&mut peers, payload).is_ok(), "pipeline calculation enqueue failed");
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P: ImageProcessor + Calc + Calc1 + ImgDc, I: Isp, T: Tables, A: Placement, Q: Calc + Calc1>
    PipelineLibrary for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn plan(&mut self, input: &PlanInput, candidates: &Candidates, selection_reference: u32) -> Option<Plan> {
        let prepared = self.prepare_motion(input, candidates, selection_reference);
        Some(Plan {
            frame_flags: FrameFlags(2 | u8::from(prepared.searches_in_full)),
            subject: PlanBlock(blocks::SUBJECT), capture: PlanBlock(blocks::CAPTURE), lens: PlanBlock(blocks::LENS), scan: PlanBlock(blocks::SCAN),
            image_buffer: u32::from(prepared.frame_delay),
            sample_targets: [1, 0, 2, 3, 4].map(|i| PlanBlock(native_motion::IMAGE_BUFFERS[i].0)),
            adjustment: ADJUSTMENT.map(PlanBlock), raw_selection: PlanBlock(blocks::RAW_SELECTION), raw_subject: PlanBlock(blocks::RAW_SUBJECT),
            raw_first: PlanBlock(blocks::RAW_FIRST), raw_second: PlanBlock(RAW_SECOND), raw_result: PlanBlock(blocks::RAW_RESULT),
        })
    }
    fn refresh_channels(&mut self) { self.lay_out_motion(); }
    fn channel_item_ready(&mut self, record: RecordHandle) -> bool {
        self.with_motion(|_, calculation| xm1_saclac_calc::adjust::motion::queries::second_channel_available(calculation, record))
    }
    fn apply_adjustment(&mut self, adjustment: &[PlanBlock; 7], pass: AdjustmentPass) {
        let records = records(adjustment);
        self.with_motion(|state, calculation| match pass {
            AdjustmentPass::Primary => { state.set_up_measurement(records, calculation); }
            AdjustmentPass::Secondary => {
                let setup = RecorderSetup { buffer: Some(state.section()), results: Some(RECORDER_RESULTS), tables: records.addresses().map(Some) };
                let _ = rate_samples::set_up(calculation, &setup);
            }
        });
    }
    fn adjustment_gated(&mut self) -> bool { self.with_motion(|state, calculation| state.peak_reached(calculation)) }
    fn build_scan_plan(&mut self, scan: PlanBlock) -> u32 {
        assert_eq!(scan.0, blocks::SCAN);
        u32::from(self.next_motion_plan().0)
    }
    fn focus_result(&mut self, scan: PlanBlock, kind: ResultKind) -> FocusResultOut {
        assert_eq!(scan.0, blocks::SCAN);
        self.with_motion(|state, calculation| {
            let mut publication = FocusPublication::default();
            match kind {
                ResultKind::Released => state.publish_existing(calculation, &mut publication),
                ResultKind::Policy => state.publish_fallback(calculation, &mut publication),
            }
            let [position, reference, state, kind] = publication.snapshot;
            FocusResultOut { status: publication.status, point: LensPoint { position: position as i16, detail: (position >> 16) as u16, reference: LensReference(reference), state, kind }, pending: publication.pending }
        })
    }
    fn selected_value(&mut self) -> u32 { self.with_motion(|state, calculation| u32::from(state.pipeline_flag(calculation))) }
    fn needs_raw(&mut self) -> bool { self.with_motion(|state, _| state.channel_status()) }
    fn conclude_raw(&mut self) {
        let machine = self.services.borrow().machine.clone();
        let mut result = [0; 20];
        for (i, bytes) in result.chunks_exact_mut(4).enumerate() { bytes.copy_from_slice(&machine.read_u32(blocks::RAW_RESULT + 4 * i as u32).to_le_bytes()); }
        let measured = native_motion::measured_levels(&result);
        self.with_motion(|state, calculation| state.check_measured_levels(calculation, &measured));
    }
    fn store_converted_input(&mut self) { self.with_motion(|state, calculation| state.trace_elapsed(calculation)); }
    fn begin_scan(&mut self) { self.with_motion(|_, calculation| rate_samples::mark_recording(calculation)); }
    fn active_channels(&mut self) -> u8 { self.with_motion(|_, calculation| rate_samples::recorded(calculation)) }
    fn position_correction(&mut self) -> u32 { self.with_motion(|state, _| state.search_mode().code()) }
    fn scaled_value(&mut self, scan: PlanBlock, index: u8) -> u16 {
        assert_eq!(scan.0, blocks::SCAN);
        self.with_motion(|state, _| state.position_at_frame(u32::from(index)) as u16)
    }
    fn scaled_levels(&mut self, scan: PlanBlock, value: u16, index: u8) -> [i16; TONE_LEVELS] {
        assert_eq!(scan.0, blocks::SCAN);
        self.with_motion(|state, calculation| state.targets(calculation, value as i16, u32::from(index)).words())
    }
    fn store_channel_samples(&mut self, scan: PlanBlock, snapshot: &SampleSnapshot) {
        assert_eq!(scan.0, blocks::SCAN);
        let index = usize::from(snapshot.0[0xa0]);
        assert!(index < 20, "capture schedule index");
        let positions = crate::capture_samples::Samples::new(self.services.borrow().machine.clone());
        self.with_motion(|state, calculation| state.replan_from_run(calculation, (0..index).map(|i| speed_step(snapshot, i)), speed_step(snapshot, index), &positions));
    }
    fn store_channel_sample(&mut self, scan: PlanBlock, sample: i16, index: u8) {
        assert_eq!(scan.0, blocks::SCAN);
        self.with_motion(|state, calculation| {
            let targets = state.targets(calculation, sample, u32::from(index));
            let _ = rate_samples::record_column(calculation, &targets.words(), index);
        });
    }
    fn select_channel(&mut self, channel: u8) { self.with_motion(|_, calculation| { let _ = rate_samples::scan_extremes(calculation, channel); }); }
    fn indexed_selection(&mut self, record: RecordHandle) -> u32 { self.with_motion(|state, calculation| calculation.selection_value(record, state.readings().lens)) }
    fn adjust_direction(&mut self, point: &LensPoint) -> i16 {
        let mut services = self.services.borrow_mut(); let mut geometry = self.geometry.borrow_mut();
        let mut view = RoundPorts::new(&mut services, &mut geometry, self.recognition, self.record);
        let target = xm1_saclac_calc::scene_recognition::frame::target(&mut view, point.reference.0, point.state);
        view.side_adjusted(target, 1, point.kind)
    }
    fn phase_attribute(&mut self, phase: u32, attribute: PhaseAttribute) -> bool {
        let Ok(phase) = u8::try_from(phase) else { return false };
        match attribute { PhaseAttribute::ToneFinal => xm1_saclac_calc::stock::plan::kind_row(PlanKind(phase))[5] == 1 }
    }
    fn request_raw(&mut self, plan: &Plan, completion: Completion) {
        assert_eq!(completion, Completion::Calculation91);
        assert_eq!((plan.raw_first.0, plan.raw_second.0), (blocks::RAW_FIRST, RAW_SECOND));
        let (first, second) = self.with_motion(|state, _| motion_projection::raw_statistics(state));
        self.post_pipeline_calculation(&CalcPayload::PipelineFirst { first, second, callback: self.callbacks.pipeline.raw_calculated });
    }
    fn request_tone(&mut self, request: &ToneRequest, completion: Completion) {
        assert_eq!(completion, Completion::Calculation92);
        self.post_pipeline_calculation(&CalcPayload::PipelineSecond { block: Record(xm1_saclac_interface::stock::pipeline::tone_request(request)), callback: self.callbacks.pipeline.tone_calculated });
    }
    fn request_gradation(&mut self, request: &GradationRequest, completion: Completion) {
        assert_eq!(completion, Completion::Calculation93);
        self.post_pipeline_calculation(&CalcPayload::PipelineThird { block: Record(xm1_saclac_interface::stock::pipeline::gradation_request(request)), callback: self.callbacks.pipeline.gradation_calculated });
    }
}
