#![forbid(unsafe_code)]

use xm1_saclac_interface::{
    lens_effect, BlockingRequest, CalcOperation, CommandKind, Completed, Completion, CompletionStatus, ImgDcOperation, LensDcOperation, LensEffect,
    PeerOperation, RecordHandle, StatisticsOperation,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Branch { HalfPress, Live }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Signal { Completion(Completion), Command(CommandKind) }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FinishKind {
    /// Relative and absolute lens moves.
    HalfPressLens,
    /// No lens move; completes at once.
    HalfPressImmediate,
    /// Absolute lens move only.
    Live,
}

/// Mode word of the half-press command parameters.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HalfPressMode(pub u32);

impl HalfPressMode {
    pub const fn synchronizes_adjustment(self) -> bool { matches!(self.0, 0 | 5 | 7) }
    pub const fn synchronizes_lens(self) -> bool { self.0 == 0 }
    pub const fn two_pass_exposure(self) -> bool { self.0 == 5 }
    pub const fn skips_relative_lens_move(self) -> bool { self.0 == 1 }
}

/// Notification mask of the half-press command parameters.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HalfPressNotifications {
    pub frame_released: bool,
    pub relative_lens_done: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EvaluationIndex(pub u8);

impl EvaluationIndex {
    /// The calculation offers this index when it has no further evaluation.
    pub const FINAL: Self = Self(255);
    pub const fn is_final(self) -> bool { self.0 == Self::FINAL.0 }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CalculationPlan { Request, Skip }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AbortStatus(pub CompletionStatus);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SequenceEffect {
    BindRecord(RecordHandle),
    NoteLiveRecord(RecordHandle),
    PrepareRecords(Branch),
    SetRefinementPass(bool),
    RecordLensStatus,
    ReleaseLiveResources,
    /// The half press has replied its completion.
    Replied,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExposureEffect {
    Begin { branch: Branch, record: RecordHandle },
    /// Calc 0x3c00, completion 0x8e.
    RequestEvaluation(EvaluationIndex),
    /// Calc 0x3c01, completion 0x8f.
    RequestCorrection,
    /// Imgdc 0x3f1a, completions 0x4d then 0x4c.
    RequestStatistics,
    /// Imgdc 0x3f1b.
    RequestStatisticsRelease,
    AbsorbStatistics,
    AbsorbStatisticsResult,
    AbsorbCalculation,
    Conclude,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExposureQuery { CorrectionFirst, StatisticsAccepted, CorrectionNeeded, ReleaseStatusAtMostOne }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CalculationEffect {
    /// Calc 0x3c02 for the live branch, completion 0x90.
    RequestLive(RecordHandle),
    /// Calc 0x3c02, completion 0x90.
    Request,
    RecordSkipped,
    /// A requested calculation's result reaches the bound record; precedes `Absorb`.
    RecordResult,
    Absorb,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PipelinePhase { First, Second, Third }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PipelineRequest { Image1d, Calc03, Image1e, Image1f, Image21, Image20, Calc04, Statistics03, Calc05, Image22, Statistics07, Image23 }

impl PipelineRequest {
    pub const fn operation(self) -> PeerOperation {
        match self {
            Self::Image1d => PeerOperation::ImgDc(ImgDcOperation::Request1d),
            Self::Calc03 => PeerOperation::Calc(CalcOperation::Request03),
            Self::Image1e => PeerOperation::ImgDc(ImgDcOperation::Request1e),
            Self::Image1f => PeerOperation::ImgDc(ImgDcOperation::Request1f),
            Self::Image21 => PeerOperation::ImgDc(ImgDcOperation::Request21),
            Self::Image20 => PeerOperation::ImgDc(ImgDcOperation::Request20),
            Self::Calc04 => PeerOperation::Calc(CalcOperation::Request04),
            Self::Statistics03 => PeerOperation::Statistics(StatisticsOperation::Request03),
            Self::Calc05 => PeerOperation::Calc(CalcOperation::Request05),
            Self::Image22 => PeerOperation::ImgDc(ImgDcOperation::Request22),
            Self::Statistics07 => PeerOperation::Statistics(StatisticsOperation::Request07),
            Self::Image23 => PeerOperation::ImgDc(ImgDcOperation::Request23),
        }
    }

    /// The completion the request hands out; imgdc 0x3f1f hands out two (0x50, 0x51).
    pub const fn completion(self) -> Option<Completion> {
        match self {
            Self::Image1d => Some(Completion::Image4e),
            Self::Calc03 => Some(Completion::Calculation91),
            Self::Image1e => Some(Completion::Image4f),
            Self::Image1f => Some(Completion::Image50),
            Self::Calc04 => Some(Completion::Calculation92),
            Self::Statistics03 => Some(Completion::Statistics7a),
            Self::Calc05 => Some(Completion::Calculation93),
            Self::Image22 => Some(Completion::Image52),
            Self::Statistics07 => Some(Completion::Statistics6b),
            Self::Image23 => Some(Completion::Image53),
            Self::Image21 | Self::Image20 => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PipelineEffect {
    BindRecord(RecordHandle),
    RecordLensStatus,
    Prepare(RecordHandle),
    MarkRestart,
    Configure { record: RecordHandle, first: bool },
    ClearFrameReady,
    MarkFrameReady,
    ReleaseFrame,
    /// Follows the frame-released reply.
    FrameReleased,
    StartFrame,
    PrepareFrame,
    ArmFrame,
    SynchronizeFrame,
    ResumeFrame,
    ReleaseFrameLocks,
    PrepareFallback,
    ConcludeRaw,
    SetStatisticsPending(bool),
    SetUploadDone(bool),
    SetPhase(PipelinePhase),
    SetToneResult(bool),
    SetFinishDone,
    Reset,
    Request(PipelineRequest),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PipelineQuery {
    FinishOnly,
    CaptureIdle,
    SkipLensFinish,
    FrameReady,
    Restarted,
    NeedsRaw,
    RawCalculationNeeded,
    StatisticsReady,
    StatisticsPending,
    UploadDone,
    ToneRequested,
    ToneSkipped,
    ToneCalculationFinal,
    ToneResultSet,
    ToneRestart,
    GradationNeeded,
    FinishDone,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FinishEffect {
    Begin(RecordHandle),
    LoadAdjustment,
    PrepareRecords,
    PrepareLens,
    SelectTarget,
    PrepareLiveTarget,
    AbsorbRelative,
    MarkRelativeDone,
    MarkAbsoluteDone,
    Clear,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FinishQuery { RelativeFirst }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LensMove {
    /// Back to the position recorded when the finish began.
    FinishRelative,
    FinishAbsolute,
    CaptureRelative,
    CaptureAbsolute,
    ReturnRelative,
    ReturnAbsolute,
}

impl LensMove {
    pub const fn operation(self) -> LensDcOperation {
        match self {
            Self::FinishRelative | Self::CaptureRelative | Self::ReturnRelative => LensDcOperation::Request0b,
            Self::FinishAbsolute | Self::CaptureAbsolute | Self::ReturnAbsolute => LensDcOperation::Request13,
        }
    }

    pub const fn completion(self) -> Completion {
        match self {
            Self::FinishRelative | Self::CaptureRelative | Self::ReturnRelative => Completion::Lens35,
            Self::FinishAbsolute | Self::CaptureAbsolute | Self::ReturnAbsolute => Completion::Lens3a,
        }
    }

    pub const fn lens_effect(self) -> LensEffect { lens_effect(PeerOperation::LensDc(self.operation())) }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureEffect {
    BindRecord(RecordHandle),
    /// Statistics 0x4007, completion 0x83.
    RequestStatisticsSetup,
    PrepareSetting,
    /// Statistics 0x4005, completion 0x85.
    RequestSetting,
    CommitSetting,
    PrepareRecord,
    PrepareJob,
    ClearSecondary,
    Finalize,
    Release,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureQuery { LensReturn, SecondaryJob }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReturnEffect {
    Begin(RecordHandle),
    PrepareRecords,
    MarkActive(bool),
    PrepareCalculation,
    AbsorbCalculation,
    ConcludeExposure,
    /// Calc 0x3c02, completion 0x90.
    RequestCalculation,
    PrepareMoves,
    PrepareRelative,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReturnQuery { NeedsCalculation, NeedsAbsoluteMove }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecondaryEffect {
    Begin(RecordHandle),
    ResetCount,
    Configure,
    /// Imgdc 0x3f46, completions 0x5c then 0x5b.
    Request,
    PrepareFirst,
    PrepareNext,
    /// Calc 0x3c0d, completions 0x94 and 0x95.
    RequestCalculation,
    Absorb,
    AbsorbStatistics,
    AbsorbCalculation,
    AbsorbSecondCalculation,
    /// Imgdc 0x3f47, completion 0x5d.
    RequestTransfer,
    Release,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecondaryQuery {
    /// Counts the measurement as it answers.
    Accepted,
    FirstPass,
    Alternative,
    TransferPending,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrimaryEffect {
    Begin(RecordHandle),
    ResetFlags,
    PrepareRecords,
    PrepareTransfer,
    /// Imgdc 0x3f49, completions 0x5e and 0x62.
    Request,
    /// Imgdc 0x3f4a, completed through the client callback of command 0xc.
    RequestCommandC,
    /// Imgdc 0x3f4f, completed through the client callback of command 0x28.
    RequestCommand28,
    MarkCommandC,
    MarkCommand28,
    ClearCommandC,
    ClearCommand28,
    PrepareRelative,
    /// Statistics 0x4005, completion 0x7d.
    RequestStatistics,
    Output,
    Delivered,
    Block,
    Account,
    /// Command-7 reply with status 9, once per capture.
    PublishProgress,
    /// Command-7 reply with status 10.
    PublishFinish,
    RestoreLens,
    Abort,
    MarkStatisticsSeen,
    /// Imgdc 0x3f4e.
    RequestDisplay,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrimaryQuery {
    ImageFirst,
    ImageVariant,
    NeedsRelative,
    NeedsAbsolute,
    CommandCPending,
    Command28Pending,
    StatisticsWanted,
    BlockReady,
    DisplayEnabled,
    Delivered,
    TransferPending,
}

/// Calc 0x3c0f, 0x3c10, 0x3c0e and 0x3c11, offered in that order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CalculationsJob { First, Second, Third, Fourth }

impl CalculationsJob {
    pub const fn operation(self) -> CalcOperation {
        match self {
            Self::First => CalcOperation::Request0f,
            Self::Second => CalcOperation::Request10,
            Self::Third => CalcOperation::Request0e,
            Self::Fourth => CalcOperation::Request11,
        }
    }

    pub const fn completion(self) -> Completion {
        match self {
            Self::First => Completion::Calculation99,
            Self::Second => Completion::Calculation97,
            Self::Third => Completion::Calculation98,
            Self::Fourth => Completion::Calculation9a,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CalculationsEffect { Begin(RecordHandle), Configure, Absorb(CalculationsJob), Request(CalculationsJob) }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CalculationsQuery { Requested, Enabled(CalculationsJob) }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Effect {
    Sequence(SequenceEffect),
    Exposure(ExposureEffect),
    Calculation(CalculationEffect),
    Pipeline(PipelineEffect),
    Finish(FinishEffect),
    Lens(LensMove),
    Capture(CaptureEffect),
    Return(ReturnEffect),
    Secondary(SecondaryEffect),
    Primary(PrimaryEffect),
    Calculations(CalculationsEffect),
}

impl Effect {
    pub const fn lens_effect(self) -> LensEffect {
        match self {
            Self::Lens(lens) => lens.lens_effect(),
            _ => LensEffect::NoLensDrive,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Query {
    HasWork,
    TwoPassExposure,
    SkipLensSynchronization,
    LensStatusPending,
    Exposure(ExposureQuery),
    Pipeline(PipelineQuery),
    Finish(FinishQuery),
    Capture(CaptureQuery),
    Return(ReturnQuery),
    Secondary(SecondaryQuery),
    Primary(PrimaryQuery),
    Calculations(CalculationsQuery),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Reply { pub command: CommandKind, pub record: Option<RecordHandle>, pub status: CompletionStatus }

impl Reply {
    /// The half-press reply sends lensdc 0x4220 before it enters the client callback.
    pub const fn lens_effect(self) -> LensEffect {
        match self.command {
            CommandKind::HalfPress => lens_effect(PeerOperation::LensDc(LensDcOperation::Request20)),
            _ => LensEffect::NoLensDrive,
        }
    }
}

pub const fn blocking_lens_effect(request: BlockingRequest) -> LensEffect {
    match request {
        BlockingRequest::LensSynchronize => lens_effect(PeerOperation::LensDc(LensDcOperation::Request17)),
        BlockingRequest::AdjustmentSynchronize => LensEffect::NoLensDrive,
    }
}

pub trait Ports {
    fn effect(&mut self, effect: Effect);
    fn query(&mut self, query: Query) -> bool;
    fn half_press_mode(&mut self) -> HalfPressMode;
    fn half_press_notifications(&mut self) -> HalfPressNotifications;
    /// Prepares the half-press calculation and reports whether calc 0x3c02 is requested.
    fn calculation_plan(&mut self, record: RecordHandle) -> CalculationPlan;
    fn next_evaluation(&mut self) -> EvaluationIndex;
    fn capture_abort(&mut self) -> Option<AbortStatus>;
    /// Holds task 43 until the request's own callback completes it.
    fn blocking(&mut self, request: BlockingRequest);
    fn reply(&mut self, reply: Reply);
    /// Enters the completion's callback in task 43, which posts it to queue 43.
    fn self_complete(&mut self, completion: Completion);
    /// Takes the result a completion delivers before the step it targets handles it.
    fn receive(&mut self, completed: Completed<'_>);
    fn fault(&mut self);
}
