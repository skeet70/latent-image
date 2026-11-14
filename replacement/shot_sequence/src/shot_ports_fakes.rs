use crate::boundary::{Frame, LensReference, Trace};
use crate::calculations_work::{Processing, SecondJobInput};
use crate::capture_boundary::{CalculationResult, CaptureOptions, CaptureParameters, MoveProfile, SlotSelection};
use crate::ports::{AbortStatus, CalculationsJob, Reply};
use crate::preparation::HalfPressParameters;
use crate::shot_ports::{ShotCommands, ShotServices};
use crate::state::{StateAddress, StateMemory};
use std::collections::BTreeMap;
use std::vec::Vec;
use xm1_saclac_interface::area_measurement::{MeasurementRequest};
use xm1_saclac_interface::calculation::ReferenceRequest;
use xm1_saclac_interface::capture_job::CaptureRequest;
use xm1_saclac_interface::capture_results::{FirstResult, SecondResult, ThirdResult, FourthResult};
use xm1_saclac_interface::exposure::StatisticsRequest;
use xm1_saclac_interface::pipeline::{FrameRequest, RawFrameRequest};
use xm1_saclac_interface::statistics::SettingOverrides;
use xm1_saclac_interface::{BlockingRequest, Completion};

mod forward;

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Trace(Trace),
    Blocking(BlockingRequest),
    Fault,
    Reply(Reply),
    SelfComplete(Completion),
    Correction,
    Evaluation(crate::ports::EvaluationIndex),
    Statistics(StatisticsRequest),
    Calculation(Completion),
    Reference(ReferenceRequest, CalculationResult, Completion),
    Absolute(u32, MoveProfile, Completion),
    Relative(i16, u32, MoveProfile, Completion),
    Setup(bool, Completion),
    Capture(CaptureRequest, crate::primary_boundary::CaptureCompletions),
    CommandC,
    Command28,
    Measure(MeasurementRequest, [Completion; 2]),
    Job(CalculationsJob),
    SecondJob(SecondJobInput),
    First(FirstResult),
    Second(SecondResult),
    Third(ThirdResult),
    Fourth(FourthResult),
    Published(CalculationsJob),
    RawFrame(RawFrameRequest),
    Frame(FrameRequest),
    Output,
    Commit,
    DumpHeader(Frame),
    Fill(Frame, u32, u32, u8),
    FrameWord(Frame, u32, u32),
    StatisticsCopy(Frame, u32, u32),
}

pub struct ExternalFake {
    pub events: Vec<Event>,
    pub half_press: HalfPressParameters,
    pub capture_parameters: CaptureParameters,
    pub abort: Option<AbortStatus>,
    pub basic: crate::leaf_fakes::Fake,
    pub capture: crate::capture_fakes::CaptureFake,
    pub grid: crate::grid_fakes::Layers,
    pub absorb: crate::calculation_fakes::World,
    pub calculation: crate::calculation_work_tests::World,
    pub pipeline: crate::pipeline_work_tests::World,
    pub finish: crate::finish_work_tests::World,
    pub primary: crate::primary_job_tests::World,
    pub secondary: crate::secondary_job_tests::World,
    pub calculations: crate::calculations_work_tests::World,
    pub dump: crate::capture_dump_tests::World,
    pub readout_processing: Option<Processing>,
    pub output_marker: i16,
    pub statistics_source: bool,
}

impl ExternalFake {
    pub fn new() -> Self {
        let mut capture = crate::capture_fakes::CaptureFake::new();
        capture.selection = SlotSelection { requested: 1, count: 1 };
        capture.add_slot(0x1000, 0, 0);
        let mut processing = Processing::default();
        processing.0[0x8c] = 1;
        Self {
            events: Vec::new(), half_press: HalfPressParameters::default(),
            capture_parameters: CaptureParameters {
                options: CaptureOptions(9), mode: 0,
                sources: crate::boundary::ClientRef(0x100), profile: crate::boundary::ClientRef(0x200),
                slots: crate::boundary::ClientRef(0x300), selection: crate::boundary::ClientRef(0x400),
                calibration: crate::boundary::ClientRef(0), window: crate::boundary::ClientRef(0),
                areas: crate::boundary::ClientRef(0), statistics_selector: crate::boundary::ClientRef(0),
            },
            abort: None, basic: crate::leaf_fakes::Fake::new(), capture,
            grid: crate::grid_fakes::Layers::default(), absorb: crate::calculation_fakes::World::new(),
            calculation: crate::calculation_work_tests::World::new(), pipeline: crate::pipeline_work_tests::World::new(),
            finish: crate::finish_work_tests::World::new(), primary: crate::primary_job_tests::World::new(),
            secondary: crate::secondary_job_tests::World::new(), calculations: crate::calculations_work_tests::World::new(0x116),
            dump: crate::capture_dump_tests::World::new(), readout_processing: Some(processing),
            output_marker: 0, statistics_source: true,
        }
    }
}

impl ShotCommands for ExternalFake {
    fn half_press_parameters(&mut self) -> HalfPressParameters { self.half_press }
    fn live_lens_reference(&mut self) -> LensReference { LensReference(77) }
    fn capture_parameters(&mut self) -> CaptureParameters { self.capture_parameters }
    fn setting_overrides(&mut self) -> SettingOverrides { SettingOverrides::default() }
    fn capture_abort(&mut self) -> Option<AbortStatus> { self.abort }
}

impl ShotServices for ExternalFake {
    fn request_calculation(&mut self, completion: Completion) { self.events.push(Event::Calculation(completion)); }
    fn blocking(&mut self, request: BlockingRequest) { self.events.push(Event::Blocking(request)); }
    fn fault(&mut self) { self.events.push(Event::Fault); }
}

impl crate::statistics_dump::DumpBoundary for ExternalFake {
    fn statistics_source_present(&mut self) -> bool { self.statistics_source }
    fn fill_dump_header(&mut self, frame: Frame) { self.events.push(Event::DumpHeader(frame)); }
    fn fill_frame(&mut self, frame: Frame, offset: u32, length: u32, byte: u8) { self.events.push(Event::Fill(frame, offset, length, byte)); }
    fn write_frame_word(&mut self, frame: Frame, offset: u32, value: u32) { self.events.push(Event::FrameWord(frame, offset, value)); }
    fn copy_statistics(&mut self, frame: Frame, offset: u32, length: u32) { self.events.push(Event::StatisticsCopy(frame, offset, length)); }
}

#[derive(Default)]
pub struct Memory { bytes: BTreeMap<u32, u8> }

impl Memory {
    pub fn word(&mut self, at: StateAddress) -> u32 {
        let mut bytes = [0; 4];
        self.read(at, &mut bytes);
        u32::from_le_bytes(bytes)
    }
}

impl StateMemory for Memory {
    fn read(&mut self, at: StateAddress, into: &mut [u8]) {
        for (index, byte) in into.iter_mut().enumerate() { *byte = self.bytes.get(&(at.get() + index as u32)).copied().unwrap_or(0); }
    }
    fn write(&mut self, at: StateAddress, bytes: &[u8]) {
        for (index, byte) in bytes.iter().enumerate() { self.bytes.insert(at.get() + index as u32, *byte); }
    }
    fn copy(&mut self, to: StateAddress, from: StateAddress, bytes: usize) {
        for index in 0..bytes as u32 {
            let byte = self.bytes.get(&(from.get() + index)).copied().unwrap_or(0);
            self.bytes.insert(to.get() + index, byte);
        }
    }
}
