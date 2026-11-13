#![forbid(unsafe_code)]

//! Leaf work of the capture's calculations job: which of calc 0x3c0f, 0x3c10, 0x3c0e and 0x3c11 the
//! capture command enables, the processing configuration the readout selects, the binned
//! statistics planes the client reads, each job's input and what each result leaves in the record
//! and the client's memory.

use crate::ports::{CalculationsEffect, CalculationsJob, CalculationsQuery};
use xm1_saclac_interface::capture_results::{FirstResult, FourthResult, ReadoutPayload, SecondResult, ThirdResult};
use xm1_saclac_interface::stock::capture_results;
use xm1_saclac_interface::{Completed, Completion, RecordHandle};

/// Capture-command job bits: calc 0x3c0f, 0x3c10 (either bit), 0x3c0e and 0x3c11.
const FIRST: u32 = 1 << 4;
const SECOND: u32 = (1 << 1) | (1 << 3);
const THIRD: u32 = 1 << 2;
const FOURTH: u32 = 1 << 8;
/// Configuration bit that passes the capture command's detail word to the jobs.
const DETAIL_ENABLED: u32 = 1 << 7;
/// Readout mode whose variant byte the processing selection takes.
const VARIANT_MODE: u32 = 3;
/// Planes of the statistics buffer, each a 16 x 16 grid; the client reads them binned 2 x 2.
pub const PLANES: usize = 4;
pub const GRID: usize = 16;
pub const BINNED: usize = GRID / 2;

pub type StatisticsPlanes = [[u32; GRID * GRID]; PLANES];
pub type BinnedPlanes = [[u32; BINNED * BINNED]; PLANES];

pub const PROCESSING_BYTES: usize = 0x98;

/// The processing configuration the capture pipeline fills for the jobs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Processing(pub [u8; PROCESSING_BYTES]);

impl Default for Processing {
    fn default() -> Self { Self([0; PROCESSING_BYTES]) }
}

impl Processing {
    fn word(&self, offset: usize) -> u32 { u32::from_le_bytes(self.0[offset..offset + 4].try_into().unwrap_or([0; 4])) }

    /// The two halves the client takes with the binned planes.
    pub fn extent(&self) -> [u16; 2] { [self.word(4) as u16, self.word(0) as u16] }

    /// 1 asks the second job to add the configured offset to an unrefined base.
    pub fn offset_mode(&self) -> u32 { self.word(0x88) }
}

/// What the capture pipeline's configuration selects from the readout.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProcessingSelection {
    pub variant: u8,
    pub variant_mode: bool,
    pub mode: u32,
    pub value: u32,
}

/// The secondary job's measurement the second calc job takes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SecondaryMeasurement {
    pub first: i16,
    pub second: i16,
    pub base: u16,
    pub refined: bool,
    pub third: i16,
    pub fourth: i16,
    pub fifth: i16,
    pub sixth: i16,
    pub count: u8,
    pub block: [u8; 24],
    pub seventh: i16,
    pub marked: bool,
}

/// The second calc job's input.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SecondJobInput {
    pub record: Option<RecordHandle>,
    pub measurement: SecondaryMeasurement,
    /// The measurement's base with the configured offset when the processing asks for it.
    pub base: i16,
    pub slot: [u8; 12],
    pub detail: u32,
    pub paired: bool,
    pub profile_kind: u32,
}

/// The capture command's job words and the client memory the jobs publish into.
pub trait CaptureCommand {
    fn jobs(&mut self) -> u32;
    fn detail(&mut self) -> u32;
    /// The client word the processing configuration reads.
    fn processing_word(&mut self) -> u32;
    fn fourth_target(&mut self) -> bool;
    fn publish_readout(&mut self, mode: u32, value: u32);
    fn publish_planes(&mut self, planes: &BinnedPlanes, extent: [u16; 2]);
    fn publish(&mut self, job: CalculationsJob, record: RecordHandle);
}

/// The capture pipeline's processing configuration and statistics buffer.
pub trait CaptureProcessing {
    /// Fills `processing` for the readout's source; nonzero asks for the enabled jobs to be
    /// requested. With `statistics` the pipeline also fills the statistics buffer.
    fn configure(&mut self, source: u8, statistics: bool, processing: &mut Processing, client: u32, detail: u32, selection: &ProcessingSelection) -> u32;
    fn statistics(&mut self) -> StatisticsPlanes;
}

/// Calc-library requests of the four jobs.
pub trait CalculationsLibrary {
    fn request_first(&mut self, record: RecordHandle, processing: &Processing);
    fn request_second(&mut self, input: &SecondJobInput, processing: &Processing);
    fn request_third(&mut self, record: RecordHandle, processing: &Processing, detail: u32);
    fn request_fourth(&mut self, record: RecordHandle, processing: &Processing, detail: u32);
}

/// Image-4 record values the jobs read and the result fields they store.
pub trait CalculationsRecords {
    fn configuration(&mut self) -> u32;
    fn offset_unit(&mut self) -> u16;
    fn paired_mode(&mut self, record: RecordHandle) -> u32;
    fn profile_kind(&mut self, record: RecordHandle) -> u32;
    fn slot(&mut self, record: RecordHandle) -> [u8; 12];
    fn store_first(&mut self, record: RecordHandle, result: &FirstResult);
    fn store_second(&mut self, record: RecordHandle, result: &SecondResult);
    fn store_third(&mut self, record: RecordHandle, result: ThirdResult);
    fn store_fourth(&mut self, record: RecordHandle, result: FourthResult);
}

/// The secondary job's state the second calc job reads.
pub trait SecondaryMeasurements {
    fn secondary_measurement(&mut self) -> SecondaryMeasurement;
}

pub trait CalculationsBoundary: CaptureCommand + CaptureProcessing + CalculationsLibrary + CalculationsRecords + SecondaryMeasurements {}
impl<T: CaptureCommand + CaptureProcessing + CalculationsLibrary + CalculationsRecords + SecondaryMeasurements> CalculationsBoundary for T {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Delivered { Readout(ReadoutPayload), First(FirstResult), Second(SecondResult), Third(ThirdResult), Fourth(FourthResult) }

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CalculationsWork {
    record: Option<RecordHandle>,
    jobs: u32,
    detail: u32,
    processing: Processing,
    delivered: Option<Delivered>,
}

/// The client's planes: each cell the mean of a 2 x 2 block of the 16 x 16 statistics grid.
pub fn binned(statistics: &StatisticsPlanes) -> BinnedPlanes {
    let mut planes = [[0; BINNED * BINNED]; PLANES];
    for (plane, cells) in planes.iter_mut().zip(statistics) {
        for (index, cell) in plane.iter_mut().enumerate() {
            let corner = (index / BINNED) * 2 * GRID + (index % BINNED) * 2;
            let sum = [corner, corner + 1, corner + GRID, corner + GRID + 1].iter().fold(0u32, |sum, &at| sum.wrapping_add(cells[at]));
            *cell = sum >> 2;
        }
    }
    planes
}

impl CalculationsWork {
    pub const fn new() -> Self { Self { record: None, jobs: 0, detail: 0, processing: Processing([0; PROCESSING_BYTES]), delivered: None } }

    /// Takes the readout or a job result a completion delivers.
    pub fn receive(&mut self, completed: Completed<'_>) {
        let payload = completed.parameters.bytes();
        let delivered = match completed.completion {
            Completion::Image61 => capture_results::readout(payload).map(Delivered::Readout),
            Completion::Calculation99 => capture_results::first_result(payload).map(Delivered::First),
            Completion::Calculation97 => capture_results::second_result(payload).map(Delivered::Second),
            Completion::Calculation98 => capture_results::third_result(payload).map(Delivered::Third),
            Completion::Calculation9a => capture_results::fourth_result(payload).map(Delivered::Fourth),
            _ => return,
        };
        self.delivered = delivered;
    }

    pub fn effect(&mut self, effect: CalculationsEffect, boundary: &mut impl CalculationsBoundary) {
        match effect {
            CalculationsEffect::Begin(record) => self.record = Some(record),
            CalculationsEffect::Configure => {
                self.jobs = boundary.jobs();
                self.detail = if boundary.configuration() & DETAIL_ENABLED != 0 { boundary.detail() } else { 0 };
            }
            CalculationsEffect::Request(job) => self.request(job, boundary),
            CalculationsEffect::Absorb(job) => self.absorb(job, boundary),
        }
    }

    pub fn query(&mut self, query: CalculationsQuery, boundary: &mut impl CalculationsBoundary) -> bool {
        match query {
            CalculationsQuery::Requested => self.configure_processing(boundary),
            CalculationsQuery::Enabled(job) => match job {
                CalculationsJob::First => self.jobs & FIRST != 0,
                CalculationsJob::Second => self.jobs & SECOND != 0,
                CalculationsJob::Third => self.jobs & THIRD != 0,
                CalculationsJob::Fourth => self.jobs & FOURTH != 0 && boundary.fourth_target(),
            },
        }
    }

    fn any_job(&self) -> bool { self.jobs & (FIRST | SECOND | THIRD | FOURTH) != 0 }

    /// Configures the capture pipeline's processing for the readout and, with jobs enabled, gives
    /// the client the binned statistics planes. Answers whether the jobs are requested.
    fn configure_processing(&mut self, boundary: &mut impl CalculationsBoundary) -> bool {
        let readout = match self.delivered.take() {
            Some(Delivered::Readout(readout)) => readout,
            _ => ReadoutPayload::default(),
        };
        boundary.publish_readout(readout.mode, readout.value);
        let variant_mode = readout.mode == VARIANT_MODE;
        let selection = ProcessingSelection { variant: if variant_mode { readout.variant } else { 0 }, variant_mode, mode: readout.mode, value: readout.value };
        let client = boundary.processing_word();
        let jobs = self.any_job();
        let detail = if jobs { self.detail } else { 0 };
        let requested = boundary.configure(readout.source, jobs, &mut self.processing, client, detail, &selection);
        if jobs {
            let planes = binned(&boundary.statistics());
            boundary.publish_planes(&planes, self.processing.extent());
        }
        requested != 0
    }

    fn request(&mut self, job: CalculationsJob, boundary: &mut impl CalculationsBoundary) {
        let Some(record) = self.record else { return };
        match job {
            CalculationsJob::First => boundary.request_first(record, &self.processing),
            CalculationsJob::Second => {
                let input = self.second_input(record, boundary);
                boundary.request_second(&input, &self.processing);
            }
            CalculationsJob::Third => boundary.request_third(record, &self.processing, self.detail),
            CalculationsJob::Fourth => boundary.request_fourth(record, &self.processing, self.detail),
        }
    }

    fn second_input(&self, record: RecordHandle, boundary: &mut impl CalculationsBoundary) -> SecondJobInput {
        let measurement = boundary.secondary_measurement();
        let base = if self.processing.offset_mode() == 1 && !measurement.refined {
            measurement.base.wrapping_add(boundary.offset_unit().wrapping_mul(10)) as i16
        } else {
            measurement.base as i16
        };
        let paired = boundary.paired_mode(record) == 2 && measurement.marked;
        SecondJobInput {
            record: Some(record),
            measurement,
            base,
            slot: boundary.slot(record),
            detail: self.detail,
            paired,
            profile_kind: boundary.profile_kind(record),
        }
    }

    /// The job's result goes into the record and the client's memory, and the job is done.
    fn absorb(&mut self, job: CalculationsJob, boundary: &mut impl CalculationsBoundary) {
        let Some(record) = self.record else { return };
        let delivered = self.delivered.take();
        match (job, delivered) {
            (CalculationsJob::First, Some(Delivered::First(result))) => boundary.store_first(record, &result),
            (CalculationsJob::Second, Some(Delivered::Second(result))) => boundary.store_second(record, &result),
            (CalculationsJob::Third, Some(Delivered::Third(result))) => boundary.store_third(record, result),
            (CalculationsJob::Fourth, Some(Delivered::Fourth(result))) => boundary.store_fourth(record, result),
            _ => {}
        }
        boundary.publish(job, record);
        self.jobs &= !match job {
            CalculationsJob::First => FIRST,
            CalculationsJob::Second => SECOND,
            CalculationsJob::Third => THIRD,
            CalculationsJob::Fourth => FOURTH,
        };
    }
}
