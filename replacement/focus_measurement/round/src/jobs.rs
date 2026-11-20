#![forbid(unsafe_code)]

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Calculation { Auxiliary, Window, Source }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Control { Initialize, Enable, Stop, Run }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome { Complete, Pending, NoWork, Updated(Calculation), SelectionUpdated, Ignored }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Effect { Initialize, Enable, Release, Submit, Collect }

pub trait CalculationPorts {
    fn effect(&mut self, calculation: Calculation, effect: Effect);
    /// Whether this tick runs the source calculation.
    fn source_due(&mut self) -> bool;
    fn result_changed(&mut self, calculation: Calculation) -> bool;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum State { Disabled, Ready, Running, Stopping }

pub struct CalculationJob {
    calculation: Calculation,
    state: State,
}

impl CalculationJob {
    pub const fn new(calculation: Calculation) -> Self { Self { calculation, state: State::Disabled } }

    pub fn control(&mut self, control: Control, ports: &mut impl CalculationPorts) -> Result<Outcome, InvalidTransition> {
        let effect = match (self.state, control) {
            (State::Disabled, Control::Initialize) => Some(Effect::Initialize),
            (State::Disabled, Control::Enable) => {
                ports.effect(self.calculation, Effect::Initialize);
                self.state = State::Ready;
                Some(Effect::Enable)
            }
            (State::Disabled, Control::Stop) => return Ok(Outcome::Complete),
            (State::Disabled, Control::Run) => return Ok(Outcome::NoWork),
            (State::Ready, Control::Stop) => {
                self.state = State::Disabled;
                Some(Effect::Release)
            }
            (State::Ready, Control::Run) => {
                if self.calculation == Calculation::Source && !ports.source_due() {
                    return Ok(Outcome::NoWork);
                }
                self.state = State::Running;
                ports.effect(self.calculation, Effect::Submit);
                return Ok(Outcome::Pending);
            }
            (State::Running, Control::Stop) => {
                self.state = State::Stopping;
                return Ok(Outcome::Pending);
            }
            _ => return Err(InvalidTransition),
        };
        if let Some(effect) = effect { ports.effect(self.calculation, effect); }
        Ok(Outcome::Complete)
    }

    pub fn completed(&mut self, ports: &mut impl CalculationPorts) -> Outcome {
        match self.state {
            State::Disabled | State::Ready => Outcome::Ignored,
            State::Running => {
                ports.effect(self.calculation, Effect::Collect);
                self.state = State::Ready;
                if self.calculation != Calculation::Window && ports.result_changed(self.calculation) {
                    Outcome::Updated(self.calculation)
                } else {
                    Outcome::NoWork
                }
            }
            State::Stopping => {
                ports.effect(self.calculation, Effect::Collect);
                ports.effect(self.calculation, Effect::Release);
                self.state = State::Disabled;
                Outcome::Complete
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidTransition;

#[cfg(saclac_qualification)]
pub mod qualification;
