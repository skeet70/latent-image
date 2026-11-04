//! The first-ready search the observer drives and the checker replays.
use super::{
    view_word, ByteView, FirstReady, Instructions, ReadinessIndex, ReadinessKind, ReadinessSample, Sample, SampleIndex,
};
use crate::composed_catalog::{physical_address as physical, Core, Pc, Readiness};

/// Least instructions between periodic readiness samples.
pub const PERIOD: u64 = 250_000;
/// Task1's initialization chain.
pub(crate) const CHAIN: [u32; 5] = [0xc22f60, 0xbfb124, 0xc22f70, 0xc22bd0, 0xc22f74];
const QUEUE_WAIT: u8 = 0x12;
pub(crate) const IMAGE5_PUBLICATION: u32 = 0x6a9840;
const PLAIN_RETURN: u32 = 0x6a9934;
const INITIALIZER_CALL: u32 = 0x69d2e0;
const INITIALIZER_RETURN: u32 = 0x69d2e4;
const INITIALIZER_ENTRY: u32 = 0xd985a4;
const ED4ACC_RETURN: u32 = 0xd985c8;
const ED4AFC_RETURN: u32 = 0xd985d8;
pub(crate) const RECEIVE_CALL: u32 = 0xc2314c;
pub(crate) const RECEIVE_RETURN: u32 = 0xc23150;
pub(crate) const QUEUE_ENTRY: u32 = 0x725318;
pub(crate) const KERNEL_RETURN: u32 = 0x72537c;
pub(crate) const ADAPTER_RECEIVE_RETURN: u32 = 0xdb5174;

/// The publication and initialization conjuncts, from samples in order.
#[derive(Clone, Debug, Default)]
pub(crate) struct Facts {
    pub(crate) task1: Option<Core>,
    chain: usize,
    image5: bool,
    plain: bool,
    initializer_call: Option<(Core, u32)>,
    initializer_entered: bool,
    initializer_returned: bool,
    ed4acc: bool,
    ed4afc: bool,
}

impl Facts {
    pub(crate) fn observe(&mut self, stock: u32, sample: &Sample) {
        let registers = &sample.registers;
        match stock {
            IMAGE5_PUBLICATION => self.image5 = registers.r[0].0 == 0,
            PLAIN_RETURN => self.plain = registers.r[0].0 == 0,
            INITIALIZER_CALL if physical(registers.r[3].0) == INITIALIZER_ENTRY => {
                self.initializer_call = Some((sample.core, registers.sp.0));
            }
            INITIALIZER_ENTRY => self.initializer_entered = self.initializer_call.is_some(),
            INITIALIZER_RETURN if self.initializer_entered && self.initializer_call == Some((sample.core, registers.sp.0)) => {
                self.initializer_returned = true;
            }
            ED4ACC_RETURN => self.ed4acc = true,
            ED4AFC_RETURN => self.ed4afc = true,
            _ => {}
        }
        if stock == CHAIN[0] && self.task1.is_none() {
            self.task1 = Some(sample.core);
        }
        if self.task1 == Some(sample.core) && CHAIN.get(self.chain) == Some(&stock) {
            self.chain += 1;
        }
    }

    pub(crate) fn complete(&self) -> bool {
        self.image5 && self.plain && self.chain == CHAIN.len() && self.initializer_returned && self.ed4acc && self.ed4afc
    }
}

/// `RenderFacts::probe`'s `drained`, recomputed from the sample's views and graphics state, with task1 among the
/// waiting render actors.
pub fn render_ready(sample: &ReadinessSample) -> bool {
    let byte = |view: &ByteView| view.value();
    let render = &sample.render;
    let statuses = sample.tasks.statuses();
    byte(&render.initialized) == Some(1)
        && byte(&render.ui_busy) == Some(0)
        && render.idle.iter().all(|view| view_word(std::slice::from_ref(view), view.address.0) == Some(0))
        && render.keys.iter().all(|key| byte(key) == Some(0))
        && sample.tasks.entries.contains(&Pc(CHAIN[0]))
        && statuses.iter().all(|status| *status == Some(QUEUE_WAIT))
        && sample.graphics.graphics().idle
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Open {
    receive: SampleIndex,
    core: Core,
    call_sp: u32,
    joined: Option<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Candidate {
    readiness: ReadinessIndex,
    receive: SampleIndex,
    start: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Due {
    Nothing,
    Periodic,
    SettleEnd,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SearchFault {
    /// No joined receive is open.
    Unjoined,
    OtherReceive,
    /// A settle-end sample other than at the candidate plus the settle interval.
    EndMismatch,
    /// A periodic sample at or after the interval's end.
    LateEnd,
    /// A settle-end sample with no candidate.
    StrayEnd,
    /// Two periodic samples closer than `PERIOD`.
    Rate,
    /// Two samples on one joined receive further apart than two `PERIOD`s.
    Gap,
}

/// First-ready is the first candidate whose readiness samples through first-ready plus the settle interval all hold
/// while its receive stays open; after an unstable candidate only a later receive may give the next.
#[derive(Clone, Debug)]
pub(crate) struct Search {
    readiness: Readiness,
    pub(crate) facts: Facts,
    open: Option<Open>,
    candidate: Option<Candidate>,
    after: Option<SampleIndex>,
    last: Option<u64>,
    previous: Option<(SampleIndex, u64)>,
    first_ready: Option<FirstReady>,
}

impl Search {
    pub(crate) fn new(readiness: Readiness) -> Self {
        Self { readiness, facts: Facts::default(), open: None, candidate: None, after: None, last: None, previous: None, first_ready: None }
    }

    pub(crate) fn sample(&mut self, stock: u32, sample: &Sample) {
        self.facts.observe(stock, sample);
        if self.facts.task1 != Some(sample.core) {
            return;
        }
        let registers = &sample.registers;
        match stock {
            RECEIVE_CALL => {
                self.close();
                self.open = Some(Open { receive: sample.index, core: sample.core, call_sp: registers.sp.0, joined: None });
            }
            QUEUE_ENTRY => {
                if let Some(open) = self.open.as_mut().filter(|open| open.joined.is_none())
                    && registers.r[0].0 == 1
                    && physical(registers.lr.0) == ADAPTER_RECEIVE_RETURN
                    && physical(registers.sp.0) < physical(open.call_sp)
                {
                    open.joined = Some(registers.sp.0);
                }
            }
            KERNEL_RETURN => {
                if self.open.is_some_and(|open| open.joined.is_some_and(|sp| registers.r[11].0.checked_add(4) == Some(sp))) {
                    self.close();
                }
            }
            RECEIVE_RETURN => self.close(),
            _ => {}
        }
    }

    fn close(&mut self) {
        if let (Some(open), Some(candidate)) = (self.open, self.candidate)
            && candidate.receive == open.receive
        {
            self.after = Some(open.receive);
            self.candidate = None;
        }
        self.open = None;
    }

    pub(crate) fn joined(&self) -> Option<SampleIndex> {
        self.open.filter(|open| open.joined.is_some()).map(|open| open.receive)
    }

    /// The instruction count at which the open candidate's settle interval ends.
    pub(crate) fn settle_end(&self) -> Option<u64> { self.candidate.map(|candidate| candidate.start + self.readiness.settle.0) }

    pub(crate) fn due(&self, now: u64) -> Due {
        if self.first_ready.is_some() || self.joined().is_none() {
            return Due::Nothing;
        }
        if self.settle_end().is_some_and(|end| now >= end) {
            return Due::SettleEnd;
        }
        if now < self.readiness.end.0 && self.last.is_none_or(|last| now - last >= PERIOD) { Due::Periodic } else { Due::Nothing }
    }

    pub(crate) fn readiness(&mut self, sample: &ReadinessSample) -> Result<(), SearchFault> {
        let receive = self.joined().ok_or(SearchFault::Unjoined)?;
        if sample.receive != receive {
            return Err(SearchFault::OtherReceive);
        }
        let now = sample.instructions.0;
        if self.previous.is_some_and(|(previous, at)| previous == receive && now.saturating_sub(at) > 2 * PERIOD) {
            return Err(SearchFault::Gap);
        }
        self.previous = Some((receive, now));
        let ready = self.facts.complete() && render_ready(sample);
        match (self.candidate, sample.kind) {
            (Some(candidate), ReadinessKind::SettleEnd) => {
                if now != candidate.start + self.readiness.settle.0 {
                    return Err(SearchFault::EndMismatch);
                }
                if ready {
                    self.first_ready = Some(FirstReady {
                        readiness: candidate.readiness, receive: candidate.receive, instructions: Instructions(candidate.start),
                    });
                } else {
                    self.after = Some(candidate.receive);
                }
                self.candidate = None;
            }
            (None, ReadinessKind::SettleEnd) => return Err(SearchFault::StrayEnd),
            (candidate, ReadinessKind::Periodic) => {
                if self.last.is_some_and(|last| now < last + PERIOD) {
                    return Err(SearchFault::Rate);
                }
                self.last = Some(now);
                match candidate {
                    Some(candidate) if now >= candidate.start + self.readiness.settle.0 => return Err(SearchFault::LateEnd),
                    Some(candidate) => {
                        if !ready {
                            self.after = Some(candidate.receive);
                            self.candidate = None;
                        }
                    }
                    None if ready && self.first_ready.is_none() && self.after.is_none_or(|after| receive > after) => {
                        if now + self.readiness.settle.0 > self.readiness.end.0 {
                            self.after = Some(receive);
                        } else {
                            self.candidate = Some(Candidate { readiness: sample.index, receive, start: now });
                        }
                    }
                    None => {}
                }
            }
        }
        Ok(())
    }

    pub(crate) fn first_ready(&self) -> Option<FirstReady> { self.first_ready }
}
