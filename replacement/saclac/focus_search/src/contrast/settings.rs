#![forbid(unsafe_code)]

//! The camera adjustments the contrast search reads, named by what the search does with
//! them. `stock::setting_key` gives each one's adjustment key.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Setting {
    /// 1: the level is ten times the first pair's second sum over its first, and each pair
    /// is summed.
    PairedEvaluation,
    /// The level ladder in thousands, hundreds, tens and units.
    LevelThousands,
    LevelHundreds,
    LevelTens,
    LevelUnits,
    /// 1: the direction search reads each pair's second sum.
    DirectionSecondChannel,
    /// 1: the other phases read each pair's second sum.
    SecondChannel,
    /// Stage report bits: 1 direction, 3 peak search, 2 return and approach, 4 a focus
    /// with a pending report, 0 the other focus phases.
    StageReport,
    ScanDriveDisabled,
    /// Either at 1 makes the bridge drive at speed 2.
    SlowBridge,
    SlowBridgeAlternate,
    /// 1: a hold drives to focus instead of holding in focus.
    HoldDrives,
    /// The largest step that brings the lens back inside its travel limits.
    ReturnStepLimit,
    /// The camera model letter.
    ModelLetter,
    /// 255 scales by the model table for any model, 0 only for models M and S.
    ModelScaling,
    /// Samples after a lens change that scale the direction step by `DirectionSettleScale`.
    DirectionSettle,
    DirectionSettleScale,
    /// Direction step scale in tenths for each level band (0 highest).
    DirectionScale { band: u8, model_scaled: bool },
    /// The travel range over this bounds a direction step.
    DirectionStepDivisor,
    /// A reported-position change beyond this restarts the contrast average.
    PositionJump,
    /// Bit 1: a flagged evaluation restarts the direction search.
    FlagRestart,
    SelectedFlagRestart,
    /// Direction samples taken before the direction search decides.
    DirectionSamples,
    /// Motion (in tens) beyond which a direction search returns when `MotionReturns` is 1.
    MotionReturn,
    MotionReturns,
    /// 1: the direction search decides from the trend counts.
    TrendDirection,
    /// 0: a direction search probes at once.
    ProbeAtOnce,
    ProbeSamples,
    /// Contrast (in tens) below which a direction search probes, and a scan step is the
    /// low-contrast step.
    ProbeContrast,
    /// Direction bits for a probe: 3 at once, 2 after the trend, 1 low contrast, 0 waits.
    ProbeDirection,
    TrendSamples,
    TrendLimit { selected: bool, basis: TrendBasis },
    /// The level at which a paired direction search stops counting probes.
    HandOffLevel,
    StillSamples,
    /// The change (in tens, a halfword) under which a sample counts as still.
    StillChange,
    TrendOverride,
    OverrideSamples,
    /// The margin (in tens) by which a low paired level's first sum exceeds its second.
    PairedMargin,
    /// Steps from a travel end within which a decided direction search returns.
    EndSteps,
    /// 1: a direction reversal adds `ReversalStep` (a halfword).
    ReversalSteps,
    ReversalStep,
    LowContrastStep,
    BaseStep { kind: StepKind, selected: bool },
    PeriodShift,
    MotionCheck,
    SelectedMotionCheck,
    MotionSamples,
    /// The level (in tens) the fastest motion raises to.
    MotionScale,
    /// Motion rates (in tens) from which and up to which the level rises.
    MotionFloor,
    MotionCeiling,
    /// 1: a motion level above the sample interval stops the direction search.
    MotionHolds,
}

/// Which trend-sample limit a direction decision uses.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrendBasis {
    /// After a lens change, with a settled evaluation.
    Settling,
    /// The intake's step class is 1.
    StepClass,
    Plain,
}

/// What a step is computed for (0xfaab3c's kind).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StepKind {
    Direction,
    /// The direction decision and the peak search.
    Scan,
    /// The return and the approach.
    Return,
}
