use std::vec::Vec;

use xm1_saclac_interface::calc::SharedAddress;
use xm1_saclac_profile_records::Bytes;

use super::ports::{Overlay, Steadiness, SubjectQuery, SubjectRequest};
use crate::exposure::fakes::{Headers, World};
use crate::exposure::ports::{ProfileAccess, Properties, Property, StockMemory};
use crate::profile::Profile;

/// The exposure world plus the subject query, the lens's steadiness and the overlay lines drawn.
#[derive(Default)]
pub struct Scenery {
    pub world: World,
    pub subjects: Option<u32>,
    pub requests: Vec<SubjectRequest>,
    pub steady: bool,
    pub lines: Vec<(Vec<u8>, [u8; 4])>,
}

impl Properties for Scenery {
    fn property(&mut self, property: Property) -> u32 { Properties::property(&mut self.world, property) }
}

impl StockMemory for Scenery {
    fn read_u8(&mut self, at: SharedAddress) -> u8 { self.world.read_u8(at) }
    fn read_u16(&mut self, at: SharedAddress) -> u16 { self.world.read_u16(at) }
    fn read_u32(&mut self, at: SharedAddress) -> u32 { self.world.read_u32(at) }
    fn write_u8(&mut self, at: SharedAddress, value: u8) { self.world.write_u8(at, value) }
    fn write_u16(&mut self, at: SharedAddress, value: u16) { self.world.write_u16(at, value) }
    fn write_u32(&mut self, at: SharedAddress, value: u32) { self.world.write_u32(at, value) }
    fn fill(&mut self, at: SharedAddress, bytes: usize, value: u8) { self.world.fill(at, bytes, value) }
}

impl ProfileAccess for Scenery {
    type Records = Bytes;
    type Entries = Headers;
    fn profile(&mut self) -> Profile<'_, Bytes, Headers> { self.world.profile() }
}

impl SubjectQuery for Scenery {
    fn subjects(&mut self, request: &SubjectRequest) -> Option<u32> {
        self.requests.push(*request);
        self.subjects
    }
}

impl Steadiness for Scenery {
    fn steady(&mut self) -> bool { self.steady }
}

impl Overlay for Scenery {
    fn text(&mut self, text: &[u8], column: u8, row: u8, foreground: u8, background: u8) { self.lines.push((text.to_vec(), [column, row, foreground, background])); }
}
