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
    /// Motion (in tens) beyond which a direction search returns when `MotionReturns` is 1; also
    /// the contrast (in tens) below which a focus or return cannot be near its peak.
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
    /// Nonzero: the lens table corrects the focus value.
    FocusCorrection,
    /// Selected samples below which a selected record's level is the hold level.
    SelectedLevelSamples,
    /// The drive amount a pinned record takes.
    PinnedAmount,
    /// 1: the debug display shows each evaluation.
    DebugDisplay,
    /// Bit 1: each evaluation is written to the focus log.
    FocusLog,
    /// 0 (or `TrendOverride` 0): the peak phase ends in focus instead of returning. Also the
    /// peaks a return passes before it decides.
    PeakReturns,
    /// 0: a drive to focus turns back from its direction when bit 0 of `FocusDriveTurn` is
    /// clear, otherwise when it is set.
    FocusDriveKeeps,
    /// Also the peaks an approach passes before it decides.
    FocusDriveTurn,
    /// Bit 1: the recent measurements' levels can end a peak search.
    RecentPeak,
    RecentPeakSamples,
    /// A halfword.
    RecentPeakLevel,
    /// 1: a peak search reads both channel sums.
    PeakBothChannels,
    /// 1: a change of the intake's focus words beyond `LevelJumpTens` restarts the search.
    LevelJump,
    LevelJumpTens,
    /// The step a peak search takes back inside its travel limits.
    EndStep,
    /// The margin (in tens) a peak must clear its channel's lowest contrast by to hold there.
    HoldMargin,
    /// The level drop (percent) that marks a rise.
    LevelDrop,
    /// The rise threshold in percent at and below the low logarithm, at and above the high
    /// one, and the two logarithms in hundreds.
    RiseHigh,
    RiseLow,
    RiseFrom,
    RiseTo,
    FallHigh,
    FallLow,
    FallFrom,
    FallTo,
    /// The combined contrast (in tens) below which the reference series can end a peak search.
    FallLevel,
    /// Consecutive focus-value moves that end a scanning peak search after a rise.
    ScanTurns,
    /// Consecutive focus-value moves that end it without one.
    ScanStalls,
    /// Nonzero: three samples around a new best are fitted.
    FitPeak,
    /// 0 logs track marks 3 and 4, else 5 and 6.
    TrackMarks,
    /// A word the focus log carries.
    FocusDisplay,
    /// Evaluations between debug display refreshes.
    DebugRate,
    /// Rows the debug display shows, one bit each.
    DebugRowsFirst,
    DebugRowsSecond,
    DebugRowsThird,
    /// The change of a focus contrast from the peak (percent of the peak plus tens) that
    /// the focus log classifies.
    FocusChangePercent,
    FocusChangeTens,
    FocusLowTens,
    FocusLevelTens,
    /// The selected samples a focus allows before it approaches; the scaled one for models
    /// the model table scales.
    SelectedSamplesScaled,
    SelectedSamples,
    /// 1: a focus takes its first contrast as the peak.
    PeakAtFocus,
    /// The focus drift allowance (tens) and strike limits (below and from
    /// `FocusLimitContrast`) per mode set and selection.
    FocusDriftMovingSelected,
    StrikesMovingSelectedLow,
    StrikesMovingSelected,
    FocusDriftMoving,
    StrikesMovingLow,
    StrikesMoving,
    FocusDriftSelected,
    StrikesSelectedLow,
    StrikesSelected,
    StrikesLow,
    Strikes,
    /// A signed halfword.
    FocusLimitContrast,
    FocusSamples,
    /// 1: a focus that ends near its peak approaches instead of searching again.
    FocusApproaches,
    FocusSecondLowTens,
    StrikeThousands,
    StrikeHundreds,
    StrikeTens,
    FilterHandover,
    FilterFeedback,
    FilterDamping,
    FilterInput,
    FocusFallFrom,
    FocusFallTo,
    FocusRiseFrom,
    FocusRiseTo,
    FocusStepFrom,
    FocusStepTo,
    FocusFallScaled,
    FocusRiseScaled,
    FocusFallHigh,
    FocusRiseHigh,
    FocusRiseLow,
    FocusStepLow,
    FocusStepHigh,
    FocusReturnTens,
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
    /// Samples a return or approach takes before it decides.
    ReturnSamples,
    /// Samples a return or approach takes before its direction follows the contrast.
    ReturnTrendSamples,
    /// Nonzero: while the record's scan flag stands, a return follows the ring's focus values.
    FocusValueTracking,
    /// 1 or above 2: a selected record's return below `SelectedReturnContrast` (hundreds, a
    /// halfword) turns on a fall of `SelectedReturnFall` percent.
    SelectedReturnMode,
    SelectedReturnContrast,
    SelectedReturnFall,
    /// The fall (percent of the previous contrast) that turns a return of an unpaired
    /// evaluation.
    ReturnFall,
    /// Signed halfwords: the focus value change that turns a tracking return, and the rise
    /// that counts as steep.
    FocusValueTurn,
    FocusValueSteep,
    /// Times a tracking return may pass the extent mark before it turns.
    ExtentCrossings,
    /// Turns a tracking return takes before it drives to a mark.
    TrackedTurns,
    /// Steep rises above which a tracking return drives to the scan mark.
    SteepRises,
    /// 0: a tracking return with no fitted extent peak drives to the extent mark, else to the
    /// scan mark.
    ScanMarkFallback,
    /// Level runs that raise and that clear the record's scan flag.
    ScanOnSamples,
    ScanOffSamples,
    /// Signed bytes: the raw focus value band (in tens) within which the levels count.
    FocusRawLow,
    FocusRawHigh,
    /// Rises that send a return to peak search.
    ReturnRises,
    /// Samples a paired return of an unselected record holds at the tens level before it
    /// searches for the peak.
    LevelHolds,
    /// Strikes that end a return.
    ReturnStrikes,
    /// A halfword: the fall (in tens) that drives a return to its best position.
    ReturnFallTens,
    /// The bias (in tens) below which a monitor follows the lens with a return, for a lens
    /// moving up and down.
    MonitorRiseTens,
    MonitorFallTens,
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
