#![forbid(unsafe_code)]

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Lane { Acquisition, Search, Auxiliary, Image, Window, Source, Bookkeeping }

const ORDER: [Lane; 7] = [Lane::Acquisition, Lane::Search, Lane::Auxiliary, Lane::Image, Lane::Window, Lane::Source, Lane::Bookkeeping];

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Lanes(u8);

impl Lanes {
    pub const fn empty() -> Self { Self(0) }
    pub const fn all() -> Self { Self(127) }
    pub fn insert(&mut self, lane: Lane) { self.0 |= 1 << lane as u8; }
    pub fn remove(&mut self, lane: Lane) { self.0 &= !(1 << lane as u8); }
    pub const fn contains(self, lane: Lane) -> bool { self.0 & (1 << lane as u8) != 0 }
    pub const fn is_empty(self) -> bool { self.0 == 0 }
    fn difference(self, other: Self) -> Self { Self(self.0 & !other.0) }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Selection {
    pub acquisition: bool,
    pub search: bool,
    pub auxiliary: bool,
    pub image: bool,
    pub source: bool,
}

impl Selection {
    pub fn lanes(self, search_allowed: bool) -> Lanes {
        let mut lanes = Lanes::empty();
        if self.acquisition { lanes.insert(Lane::Acquisition); lanes.insert(Lane::Window); }
        if self.search && search_allowed { lanes.insert(Lane::Search); }
        if self.auxiliary { lanes.insert(Lane::Auxiliary); }
        if self.image && search_allowed { lanes.insert(Lane::Image); }
        if self.source { lanes.insert(Lane::Source); }
        lanes
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Control { Initialize, Enable, Stop }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome { Complete, Pending }

pub trait Ports {
    type Error;
    fn control(&mut self, lane: Lane, control: Control) -> Result<Outcome, Self::Error>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Failure<E> { Port(E), UnexpectedPending { lane: Lane, control: Control } }

pub struct Schedule {
    enabled: Lanes,
    enabling: Lanes,
    draining: Lanes,
    search_allowed: bool,
}

impl Default for Schedule { fn default() -> Self { Self::new() } }

impl Schedule {
    pub const fn new() -> Self {
        Self { enabled: Lanes::empty(), enabling: Lanes::empty(), draining: Lanes::empty(), search_allowed: true }
    }

    pub fn initialize<P: Ports>(&mut self, selected: Lanes, ports: &mut P) -> Result<Outcome, Failure<P::Error>> {
        self.enabled = selected;
        self.enabling = Lanes::empty();
        self.draining = Lanes::empty();
        for lane in ORDER {
            let control = if selected.contains(lane) { Control::Enable } else { Control::Initialize };
            Self::synchronous(ports, lane, control)?;
        }
        Ok(if selected.is_empty() { Outcome::Complete } else { Outcome::Pending })
    }

    pub fn changes(&mut self, requested: Selection, active: Selection, record_has_selection: bool, search_allowed: bool) -> Outcome {
        self.search_allowed = search_allowed;
        let enable = requested.lanes(search_allowed).difference(self.enabled);
        let mut disable = requested.lanes(true).difference(active.lanes(true));
        if !record_has_selection { disable.insert(Lane::Bookkeeping); }
        self.enabling = Lanes(self.enabling.0 | enable.0);
        self.draining = Lanes(self.draining.0 | disable.difference(enable).0);
        if enable.is_empty() && disable.is_empty() { Outcome::Complete } else { Outcome::Pending }
    }

    pub fn enable_pending<P: Ports>(&mut self, ports: &mut P) -> Result<Outcome, Failure<P::Error>> {
        for lane in ORDER {
            if !self.enabling.contains(lane) || (!self.search_allowed && matches!(lane, Lane::Search | Lane::Image)) { continue; }
            Self::synchronous(ports, lane, Control::Enable)?;
            self.enabling.remove(lane);
        }
        Ok(if self.enabling.is_empty() { Outcome::Complete } else { Outcome::Pending })
    }

    pub fn stop<P: Ports>(&mut self, ports: &mut P) -> Result<Outcome, P::Error> {
        self.draining = Lanes::all();
        self.drain(ports)
    }

    pub fn drain<P: Ports>(&mut self, ports: &mut P) -> Result<Outcome, P::Error> {
        for lane in ORDER {
            if self.draining.contains(lane) && ports.control(lane, Control::Stop)? == Outcome::Complete {
                self.draining.remove(lane);
            }
        }
        Ok(if self.draining.is_empty() { Outcome::Complete } else { Outcome::Pending })
    }

    pub fn completed(&mut self, lane: Lane, outcome: Outcome) -> Outcome {
        if outcome == Outcome::Complete { self.draining.remove(lane); }
        if self.draining.is_empty() { Outcome::Complete } else { Outcome::Pending }
    }

    pub fn enable_search(&mut self, allowed: bool) { self.search_allowed = allowed; }

    fn synchronous<P: Ports>(ports: &mut P, lane: Lane, control: Control) -> Result<(), Failure<P::Error>> {
        match ports.control(lane, control).map_err(Failure::Port)? {
            Outcome::Complete => Ok(()),
            Outcome::Pending => Err(Failure::UnexpectedPending { lane, control }),
        }
    }
}
