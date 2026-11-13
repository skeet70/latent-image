#![forbid(unsafe_code)]

//! The capture pipeline's state, which the imaging task and the capture tasks write. saclac reads
//! and clears it only through these entries, bound at its composition root to the stock ones.

/// Where the pipeline writes captures, with each buffer's length in bytes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RecordBuffers {
    pub primary: u32,
    pub secondary: u32,
    pub primary_bytes: u32,
    pub secondary_bytes: u32,
}

/// A block of stock memory a caller reads through its own stock boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StockBlock(pub u32);

pub const SNAPSHOT_BYTES: usize = 0x5c4;
pub const SAVED_SNAPSHOT_BYTES: usize = 0xa4;
pub const CAPTURE_RECORD_BYTES: usize = 0x60;
pub const STATISTICS_PARAMETER_BYTES: usize = 0x98;
pub const STATISTICS_BLOCK_BYTES: usize = 0xd8;

/// A pipeline snapshot in the imaging task's layout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PipelineSnapshot(pub [u8; SNAPSHOT_BYTES]);

impl Default for PipelineSnapshot {
    fn default() -> Self { Self([0; SNAPSHOT_BYTES]) }
}

/// The sample state's saved snapshot in the imaging task's layout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SavedSnapshot(pub [u8; SAVED_SNAPSHOT_BYTES]);

/// A capture record in the imaging task's layout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptureRecord(pub [u8; CAPTURE_RECORD_BYTES]);

/// The statistics a completed capture left: their source and the request's two blocks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompletedStatistics {
    pub source: u32,
    pub parameters: [u8; STATISTICS_PARAMETER_BYTES],
    pub block: [u8; STATISTICS_BLOCK_BYTES],
}

/// The processing words the capture callbacks keep.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessingWord {
    FirstList,
    SecondList,
    DumpFirst,
    DumpSecond,
    DumpThird,
}

/// The sample state's measurement index and the measurements it indexes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SampleMeasurements {
    pub index: u16,
    pub measurements: StockBlock,
}

pub trait CapturePipeline {
    /// Lays out the record buffers and marks the buffer state started (0xfe90a0).
    fn start_buffers(&mut self, buffers: RecordBuffers);
    /// 0xfe90c0.
    fn clear_request_state(&mut self);
    /// 0xff9bb8.
    fn output_pending(&mut self) -> bool;
    /// 0xff6f94.
    fn snapshot(&mut self, index: u32, into: &mut PipelineSnapshot);
    /// 0xff6fc0.
    fn snapshot_counter(&mut self) -> u32;
    /// 0xfef8e8.
    fn sample_measurements(&mut self) -> SampleMeasurements;
    /// 0xfef910.
    fn sample_idle(&mut self) -> bool;
    /// 0xfef938.
    fn saved_snapshot(&mut self, into: &mut SavedSnapshot);
    /// 0xff2c5c.
    fn capture_record(&mut self, index: u32, into: &mut CaptureRecord);
    /// 0xfec984.
    fn cancel_step_timer(&mut self, index: u32);
    /// 0xfeda78, 0xfeda98, 0xfedab8, 0xfedad8, 0xfedaf8.
    fn processing_word(&mut self, word: ProcessingWord) -> u32;
    /// The statistics of the last completed capture of `kind`, if it was that kind (0xffcfd0).
    fn completed_statistics(&mut self, kind: u8) -> Option<CompletedStatistics>;
    /// The status word the buffer configuration step leaves (0xffde8c).
    fn configuration_status(&mut self) -> u32;
    /// Applies the completion request's descriptor to the profile layer (0x10029bc).
    fn apply_completion_descriptor(&mut self);
    /// 0x1005044.
    fn control_work_storage(&mut self) -> StockBlock;
}

/// The capture result and operation-history state, which the result task and the dispatch task
/// write.
pub trait CaptureResults {
    /// 0x1028adc.
    fn clear_result_completion(&mut self);
    /// 0x1054388 and 0x10543ac.
    fn set_dispatch_enabled(&mut self, enabled: bool);
    /// 0x10538a4.
    fn history_status(&mut self) -> u32;
    /// 0x105386c.
    fn clear_history_status(&mut self);
}
