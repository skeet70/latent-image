#![forbid(unsafe_code)]

//! The contrast-search slot (#109). Stock X-M1 1.50's search is its first strategy; the
//! module that owns the search issues the lens and evaluation requests a decision asks for.

/// A focus position in the lens's raw signed coordinate (lensdc's position halfword).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct LensPosition(pub i16);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    Increasing,
    Decreasing,
}

/// A lens move relative to where the lens is when the request is made (0xf3e9cc).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Travel {
    pub direction: Direction,
    pub steps: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SearchInput<Sample> {
    /// Forget everything learnt from earlier searches.
    Initialize,
    /// Start searching from where the lens is.
    Begin,
    /// One evaluated measurement, with the lens report that preceded it.
    Sample(Sample),
    /// Keep the lens where it is and watch the scene.
    Suspend,
    /// Search again after a suspension.
    Resume,
    /// Prepare to resume after outstanding requests drain.
    ResumePending,
    Cancel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SearchStep<Drive> {
    /// Evaluate the next frame with the lens where it is.
    Measure,
    /// Move the lens, then evaluate the next frame. `Drive` is what the strategy tells the
    /// lens bridge about the move beyond its travel.
    Travel { travel: Travel, drive: Drive },
    /// Stop a lens move in progress and evaluate the next frame.
    Interrupt,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SearchTerminal {
    Focused(LensPosition),
    Failed,
    Cancelled,
}

/// Stock's live search never returns `Terminal`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SearchDecision<Drive = ()> {
    Next(SearchStep<Drive>),
    Terminal(SearchTerminal),
}

pub trait FocusSearch {
    type Context;
    type Sample;
    type Drive;

    fn advance(
        &mut self,
        context: &mut Self::Context,
        input: SearchInput<Self::Sample>,
    ) -> SearchDecision<Self::Drive>;
}
