use crate::calculations_work::*;
use crate::jobs::Calculations;
use crate::ports::*;
use crate::tests::record;
use crate::Outcome;
use std::vec::Vec;
use xm1_saclac_interface::capture_results::{FirstResult, FourthResult, SecondResult, ThirdResult};
use xm1_saclac_interface::{stock, BlockingRequest, Completed, Completion, RecordHandle};

#[derive(Clone, Debug, PartialEq)]
enum Call {
    Readout(u32, u32),
    Configure { source: u8, statistics: bool, client: u32, detail: u32, selection: ProcessingSelection },
    Planes(u32, [u16; 2]),
    First,
    Second(SecondJobInput),
    Third(u32),
    Fourth(u32),
    StoreFirst(FirstResult),
    StoreSecond(SecondResult),
    StoreThird(ThirdResult),
    StoreFourth(FourthResult),
    Publish(CalculationsJob),
}

struct World {
    jobs: u32,
    configuration: u32,
    fourth_target: bool,
    requested: u32,
    statistics: StatisticsPlanes,
    measurement: SecondaryMeasurement,
    paired_mode: u32,
    processing: Processing,
    calls: Vec<Call>,
}

impl World {
    fn new(jobs: u32) -> Self {
        let mut processing = Processing::default();
        processing.0[0..4].copy_from_slice(&0x30u32.to_le_bytes());
        processing.0[4..8].copy_from_slice(&0x40u32.to_le_bytes());
        Self {
            jobs,
            configuration: 0x80,
            fourth_target: true,
            requested: 1,
            statistics: [[0; GRID * GRID]; PLANES],
            measurement: SecondaryMeasurement { base: 100, marked: true, ..SecondaryMeasurement::default() },
            paired_mode: 2,
            processing,
            calls: Vec::new(),
        }
    }
}

impl CaptureCommand for World {
    fn jobs(&mut self) -> u32 { self.jobs }
    fn detail(&mut self) -> u32 { 0xd0 }
    fn processing_word(&mut self) -> u32 { 0xc0 }
    fn fourth_target(&mut self) -> bool { self.fourth_target }
    fn publish_readout(&mut self, mode: u32, value: u32) { self.calls.push(Call::Readout(mode, value)); }
    fn publish_planes(&mut self, planes: &BinnedPlanes, extent: [u16; 2]) { self.calls.push(Call::Planes(planes[1][0], extent)); }
    fn publish(&mut self, job: CalculationsJob, _record: RecordHandle) { self.calls.push(Call::Publish(job)); }
}

impl CaptureProcessing for World {
    fn configure(&mut self, source: u8, statistics: bool, processing: &mut Processing, client: u32, detail: u32, selection: &ProcessingSelection) -> u32 {
        *processing = self.processing;
        self.calls.push(Call::Configure { source, statistics, client, detail, selection: *selection });
        self.requested
    }
    fn statistics(&mut self) -> StatisticsPlanes { self.statistics }
}

impl CalculationsLibrary for World {
    fn request_first(&mut self, _record: RecordHandle, _processing: &Processing) { self.calls.push(Call::First); }
    fn request_second(&mut self, input: &SecondJobInput, _processing: &Processing) { self.calls.push(Call::Second(*input)); }
    fn request_third(&mut self, _record: RecordHandle, _processing: &Processing, detail: u32) { self.calls.push(Call::Third(detail)); }
    fn request_fourth(&mut self, _record: RecordHandle, _processing: &Processing, detail: u32) { self.calls.push(Call::Fourth(detail)); }
}

impl CalculationsRecords for World {
    fn configuration(&mut self) -> u32 { self.configuration }
    fn offset_unit(&mut self) -> u16 { 3 }
    fn paired_mode(&mut self, _record: RecordHandle) -> u32 { self.paired_mode }
    fn profile_kind(&mut self, _record: RecordHandle) -> u32 { 5 }
    fn slot(&mut self, _record: RecordHandle) -> [u8; 12] { [7; 12] }
    fn store_first(&mut self, _record: RecordHandle, result: &FirstResult) { self.calls.push(Call::StoreFirst(*result)); }
    fn store_second(&mut self, _record: RecordHandle, result: &SecondResult) { self.calls.push(Call::StoreSecond(*result)); }
    fn store_third(&mut self, _record: RecordHandle, result: ThirdResult) { self.calls.push(Call::StoreThird(result)); }
    fn store_fourth(&mut self, _record: RecordHandle, result: FourthResult) { self.calls.push(Call::StoreFourth(result)); }
}

/// Ports that dispatch the calculations job's effects and queries into `CalculationsWork`.
struct Wired { work: CalculationsWork, world: World }

impl Ports for Wired {
    fn effect(&mut self, effect: Effect) {
        if let Effect::Calculations(effect) = effect { self.work.effect(effect, self.world.measurement, &mut self.world); }
    }
    fn query(&mut self, query: Query) -> bool {
        match query {
            Query::Calculations(query) => self.work.query(query, &mut self.world),
            _ => false,
        }
    }
    fn half_press_mode(&mut self) -> HalfPressMode { HalfPressMode(0) }
    fn half_press_notifications(&mut self) -> HalfPressNotifications { HalfPressNotifications { frame_released: false, relative_lens_done: false } }
    fn calculation_plan(&mut self, _record: RecordHandle) -> CalculationPlan { CalculationPlan::Skip }
    fn next_evaluation(&mut self) -> EvaluationIndex { EvaluationIndex::FINAL }
    fn capture_abort(&mut self) -> Option<AbortStatus> { None }
    fn blocking(&mut self, _request: BlockingRequest) {}
    fn reply(&mut self, _reply: Reply) {}
    fn self_complete(&mut self, _completion: Completion) {}
    fn fault(&mut self) {}
    fn receive(&mut self, completed: Completed<'_>) { self.work.receive(completed); }
}

fn deliver(calculations: &mut Calculations, ports: &mut Wired, completion: Completion, payload: &[u8]) -> Outcome {
    ports.receive(Completed { completion, parameters: stock::parameters(payload) });
    calculations.signal(Signal::Completion(completion), record(), ports)
}

fn readout(source: u8, variant: u8, mode: u32, value: u32) -> [u8; 0x24] {
    let mut payload = [0; 0x24];
    payload[4] = source;
    payload[5] = variant;
    payload[0x1c..0x20].copy_from_slice(&mode.to_le_bytes());
    payload[0x20..0x24].copy_from_slice(&value.to_le_bytes());
    payload
}

const ALL_JOBS: u32 = (1 << 4) | (1 << 1) | (1 << 2) | (1 << 8);

#[test]
fn requested_jobs_run_in_order_and_each_result_reaches_record_and_client() {
    let mut calculations = Calculations::new();
    let mut ports = Wired { work: CalculationsWork::new(), world: World::new(ALL_JOBS) };
    assert_eq!(deliver(&mut calculations, &mut ports, Completion::Image61, &readout(9, 4, 3, 0x77)), Outcome::Pending);
    assert_eq!(&ports.world.calls[..2], [
        Call::Readout(3, 0x77),
        Call::Configure { source: 9, statistics: true, client: 0xc0, detail: 0xd0, selection: ProcessingSelection { variant: 4, variant_mode: true, mode: 3, value: 0x77 } },
    ]);
    assert_eq!(ports.world.calls[2], Call::Planes(0, [0x40, 0x30]));
    assert_eq!(ports.world.calls[3], Call::First);
    ports.world.calls.clear();

    assert_eq!(deliver(&mut calculations, &mut ports, Completion::Calculation99, &[1; 12]), Outcome::Pending);
    assert_eq!(&ports.world.calls[..2], [Call::StoreFirst(FirstResult([1; 12])), Call::Publish(CalculationsJob::First)]);
    let Call::Second(input) = ports.world.calls[2] else { panic!("second job requested") };
    assert_eq!((input.base, input.detail, input.paired, input.profile_kind, input.slot), (100, 0xd0, true, 5, [7; 12]));
    ports.world.calls.clear();

    assert_eq!(deliver(&mut calculations, &mut ports, Completion::Calculation97, &[2; 48]), Outcome::Pending);
    assert_eq!(ports.world.calls, [Call::StoreSecond(SecondResult([2; 48])), Call::Publish(CalculationsJob::Second), Call::Third(0xd0)]);
    ports.world.calls.clear();

    assert_eq!(deliver(&mut calculations, &mut ports, Completion::Calculation98, &5u32.to_le_bytes()), Outcome::Pending);
    assert_eq!(ports.world.calls, [Call::StoreThird(ThirdResult(5)), Call::Publish(CalculationsJob::Third), Call::Fourth(0xd0)]);
    ports.world.calls.clear();

    assert_eq!(deliver(&mut calculations, &mut ports, Completion::Calculation9a, &[1, 0, 2, 0, 3, 0]), Outcome::Complete);
    assert_eq!(ports.world.calls, [Call::StoreFourth(FourthResult { first: 1, second: 2, third: 3 }), Call::Publish(CalculationsJob::Fourth)]);
}

#[test]
fn an_unrequested_set_is_tracked_without_requests() {
    let mut calculations = Calculations::new();
    let mut ports = Wired { work: CalculationsWork::new(), world: World { requested: 0, ..World::new(1 << 2) } };
    assert_eq!(deliver(&mut calculations, &mut ports, Completion::Image61, &readout(1, 0, 0, 0)), Outcome::Pending);
    assert!(!ports.world.calls.iter().any(|call| matches!(call, Call::Third(_))));
    assert_eq!(deliver(&mut calculations, &mut ports, Completion::Calculation98, &5u32.to_le_bytes()), Outcome::Complete);
}

#[test]
fn without_enabled_jobs_the_configuration_takes_no_statistics_and_the_job_completes() {
    let mut calculations = Calculations::new();
    let mut ports = Wired { work: CalculationsWork::new(), world: World::new(0) };
    assert_eq!(deliver(&mut calculations, &mut ports, Completion::Image61, &readout(2, 8, 1, 6)), Outcome::Complete);
    assert_eq!(ports.world.calls, [
        Call::Readout(1, 6),
        Call::Configure { source: 2, statistics: false, client: 0xc0, detail: 0, selection: ProcessingSelection { variant: 0, variant_mode: false, mode: 1, value: 6 } },
    ]);
}

#[test]
fn the_detail_word_needs_its_configuration_bit_and_the_fourth_job_its_client_target() {
    let mut work = CalculationsWork::new();
    let mut world = World { configuration: 0, fourth_target: false, ..World::new(1 << 8) };
    work.effect(CalculationsEffect::Begin(record()), world.measurement, &mut world);
    work.effect(CalculationsEffect::Configure, world.measurement, &mut world);
    assert!(!work.query(CalculationsQuery::Enabled(CalculationsJob::Fourth), &mut world));
    work.query(CalculationsQuery::Requested, &mut world);
    assert!(matches!(world.calls[1], Call::Configure { detail: 0, .. }));
}

#[test]
fn the_second_job_adds_the_offset_to_an_unrefined_base_when_the_processing_asks() {
    for (offset_mode, refined, base) in [(1, false, 130), (1, true, 100), (0, false, 100)] {
        let mut work = CalculationsWork::new();
        let mut world = World::new(1 << 1);
        world.processing.0[0x88..0x8c].copy_from_slice(&(offset_mode as u32).to_le_bytes());
        world.measurement.refined = refined;
        work.effect(CalculationsEffect::Begin(record()), world.measurement, &mut world);
        work.effect(CalculationsEffect::Configure, world.measurement, &mut world);
        work.query(CalculationsQuery::Requested, &mut world);
        work.effect(CalculationsEffect::Request(CalculationsJob::Second), world.measurement, &mut world);
        let Some(Call::Second(input)) = world.calls.last().cloned() else { panic!("second job requested") };
        assert_eq!(input.base, base, "offset mode {offset_mode} refined {refined}");
    }
}

#[test]
fn binned_planes_average_each_two_by_two_block() {
    let mut statistics = [[0u32; GRID * GRID]; PLANES];
    for (index, cell) in statistics[2].iter_mut().enumerate() { *cell = (index / GRID * 100 + index % GRID) as u32; }
    let planes = binned(&statistics);
    assert_eq!(planes[2][0], (0 + 1 + 100 + 101) / 4);
    assert_eq!(planes[2][BINNED + 1], (202 + 203 + 302 + 303) / 4);
    assert_eq!(planes[2][BINNED * BINNED - 1], (1414 + 1415 + 1514 + 1515) / 4);
    assert_eq!(planes[0], [0; BINNED * BINNED]);
}
