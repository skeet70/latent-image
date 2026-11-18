#![forbid(unsafe_code)]

use crate::outcome::{fault, Outcome};
use crate::ports::{Effect, FinishEffect as E, FinishKind, FinishQuery, LensMove, Ports, Query, Reply, Signal};
use xm1_saclac_interface::{CommandKind, Completion, CompletionStatus, RecordHandle};

pub const RELATIVE_DONE_STATUS: CompletionStatus = CompletionStatus(6);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Moves { record: RecordHandle, relative_done: bool, absolute_done: bool }

pub struct Finish { moves: Option<Moves> }

impl Default for Finish { fn default() -> Self { Self::new() } }

impl Finish {
    pub const fn new() -> Self { Self { moves: None } }

    pub fn start(&mut self, kind: FinishKind, record: RecordHandle, ports: &mut impl Ports) -> Outcome {
        if self.moves.is_some() { return fault(ports); }
        ports.effect(Effect::Finish(E::Begin(record)));
        match kind {
            FinishKind::HalfPressImmediate => {
                ports.effect(Effect::Finish(E::LoadAdjustment));
                ports.effect(Effect::Finish(E::PrepareRecords));
                Outcome::Complete
            }
            FinishKind::HalfPressLens => {
                ports.effect(Effect::Finish(E::LoadAdjustment));
                ports.effect(Effect::Finish(E::PrepareRecords));
                ports.effect(Effect::Finish(E::PrepareLens));
                ports.effect(Effect::Finish(E::SelectTarget));
                if ports.query(Query::Finish(FinishQuery::RelativeFirst)) {
                    Self::relative(ports);
                    ports.effect(Effect::Lens(LensMove::FinishAbsolute));
                } else {
                    ports.effect(Effect::Lens(LensMove::FinishAbsolute));
                    Self::relative(ports);
                }
                self.moves = Some(Moves { record, relative_done: false, absolute_done: false });
                Outcome::Pending
            }
            FinishKind::Live => {
                ports.effect(Effect::Finish(E::PrepareLiveTarget));
                ports.effect(Effect::Lens(LensMove::FinishAbsolute));
                self.moves = Some(Moves { record, relative_done: true, absolute_done: false });
                Outcome::Pending
            }
        }
    }

    fn relative(ports: &mut impl Ports) {
        if ports.half_press_mode().skips_relative_lens_move() {
            ports.self_complete(LensMove::FinishRelative.completion());
        } else {
            ports.effect(Effect::Lens(LensMove::FinishRelative));
        }
    }

    pub fn completed(&mut self, signal: Signal, ports: &mut impl Ports) -> Outcome {
        let Signal::Completion(completion) = signal else { return Outcome::Unhandled };
        let Some(mut moves) = self.moves else {
            return match completion {
                Completion::Lens35 | Completion::Lens3a | Completion::Filea1 => fault(ports),
                _ => Outcome::Unhandled,
            };
        };
        let outstanding = match completion {
            Completion::Lens35 => {
                if ports.half_press_notifications().relative_lens_done {
                    ports.reply(Reply { command: CommandKind::HalfPress, record: Some(moves.record), status: RELATIVE_DONE_STATUS });
                }
                ports.effect(Effect::Finish(E::AbsorbRelative));
                ports.effect(Effect::Finish(E::MarkRelativeDone));
                moves.relative_done = true;
                Outcome::Progress
            }
            Completion::Lens3a => {
                ports.effect(Effect::Finish(E::MarkAbsoluteDone));
                moves.absolute_done = true;
                Outcome::Pending
            }
            Completion::Filea1 => return fault(ports),
            _ => return Outcome::Unhandled,
        };
        if moves.relative_done && moves.absolute_done {
            self.moves = None;
            ports.effect(Effect::Finish(E::Clear));
            Outcome::Complete
        } else {
            self.moves = Some(moves);
            outstanding
        }
    }
}
