#![forbid(unsafe_code)]

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct LensPosition(pub i32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SearchInput<Sample> {
    Initialize,
    Begin,
    Sample(Sample),
    Cancel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SearchStep {
    AcquireSample,
    MoveTo(LensPosition),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SearchTerminal {
    Focused(LensPosition),
    Failed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SearchDecision {
    Next(SearchStep),
    Terminal(SearchTerminal),
}

pub trait FocusSearch {
    type Context;
    type Sample;

    fn advance(
        &self,
        context: &mut Self::Context,
        input: SearchInput<Self::Sample>,
    ) -> SearchDecision;
}
