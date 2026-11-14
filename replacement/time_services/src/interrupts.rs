#![forbid(unsafe_code)]

use super::HandleWord;
use core::num::NonZeroU32;

/// A live interrupt registration's handle, issued by the retained allocator for the line's kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InterruptHandle(NonZeroU32);
impl InterruptHandle {
    pub const fn new(word: u32) -> Option<Self> {
        match NonZeroU32::new(word) { Some(word) => Some(Self(word)), None => None }
    }
    pub const fn word(self) -> u32 { self.0.get() }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Source(u8);
impl Source {
    pub const fn new(source: u32) -> Option<Self> {
        if source <= 122 { Some(Self(source as u8)) } else { None }
    }
    pub const fn get(self) -> u8 { self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Line(u8);
impl Line {
    pub const fn new(line: u32) -> Option<Self> {
        if line <= 127 { Some(Self(line as u8)) } else { None }
    }
    pub const fn get(self) -> u8 { self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Priority(u8);
impl Priority {
    pub const fn new(priority: u32) -> Option<Self> {
        if priority <= 15 { Some(Self(priority as u8)) } else { None }
    }
    pub const fn get(self) -> u8 { self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Trigger { Mode0, Mode1, Mode2, Mode3 }
impl Trigger {
    pub const fn new(mode: u32) -> Option<Self> {
        match mode { 0 => Some(Self::Mode0), 1 => Some(Self::Mode1), 2 => Some(Self::Mode2), 3 => Some(Self::Mode3), _ => None }
    }
    pub const fn word(self) -> u32 {
        match self { Self::Mode0 => 0, Self::Mode1 => 1, Self::Mode2 => 2, Self::Mode3 => 3 }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Route { pub line: Line, pub priority: Priority, pub trigger: Trigger }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error { BadSource, InvalidHandle, Configure, Allocate(i32) }
impl Error {
    pub const fn status(self) -> i32 {
        match self { Self::BadSource => -30, Self::InvalidHandle => -20, Self::Configure => -255, Self::Allocate(word) => word }
    }
}

pub trait Ports<C> {
    fn route(&mut self, source: Source) -> Result<Route, Error>;
    fn allocate(&mut self, line: Line) -> Result<InterruptHandle, Error>;
    fn configure(&mut self, route: Route, handler: C) -> Result<(), Error>;
    fn publish_source(&mut self, line: Line, source: Option<Source>);
    fn resolve(&mut self, handle: HandleWord) -> Result<Line, Error>;
    fn detach(&mut self, line: Line) -> Result<(), Error>;
    fn release(&mut self, line: Line);
}

pub fn attach<C>(source: Source, handler: C, ports: &mut impl Ports<C>) -> Result<InterruptHandle, Error> {
    let route = ports.route(source)?;
    let handle = ports.allocate(route.line)?;
    let configured = ports.configure(route, handler);
    ports.publish_source(route.line, Some(source));
    configured.map(|()| handle)
}

pub fn release<C>(handle: HandleWord, ports: &mut impl Ports<C>) -> Result<(), Error> {
    let line = ports.resolve(handle).map_err(|_| Error::InvalidHandle)?;
    let detached = ports.detach(line);
    ports.publish_source(line, None);
    ports.release(line);
    detached
}
