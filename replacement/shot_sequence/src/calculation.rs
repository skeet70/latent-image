#![forbid(unsafe_code)]

use crate::outcome::{fault, Outcome};
use crate::ports::{Branch, CalculationEffect as E, CalculationPlan, Effect, Ports};
use xm1_saclac_interface::{Completion, RecordHandle};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Pending { None, Requested, Skipped }

/// Calc 0x3c02, completed by 0x90 whether requested or skipped.
pub struct Calculation { pending: Pending }

impl Default for Calculation { fn default() -> Self { Self::new() } }

impl Calculation {
    pub const fn new() -> Self { Self { pending: Pending::None } }

    pub fn start(&mut self, branch: Branch, record: RecordHandle, ports: &mut impl Ports) -> Outcome {
        if self.pending != Pending::None { return fault(ports); }
        self.pending = Pending::Requested;
        match branch {
            Branch::Live => ports.effect(Effect::Calculation(E::RequestLive(record))),
            Branch::HalfPress => match ports.calculation_plan(record) {
                CalculationPlan::Request => ports.effect(Effect::Calculation(E::Request)),
                CalculationPlan::Skip => {
                    self.pending = Pending::Skipped;
                    ports.effect(Effect::Calculation(E::RecordSkipped));
                    ports.self_complete(Completion::Calculation90);
                }
            },
        }
        Outcome::Pending
    }

    pub fn completed(&mut self, completion: Completion, ports: &mut impl Ports) -> Outcome {
        match (self.pending, completion) {
            (Pending::Requested | Pending::Skipped, Completion::Calculation90) => {
                if self.pending == Pending::Requested { ports.effect(Effect::Calculation(E::RecordResult)); }
                self.pending = Pending::None;
                ports.effect(Effect::Calculation(E::Absorb));
                Outcome::Complete
            }
            (Pending::None, Completion::Calculation90) => fault(ports),
            _ => Outcome::Unhandled,
        }
    }
}
