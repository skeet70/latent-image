use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Handle(u8);
impl Handle { pub fn index(self) -> usize { self.0 as usize } pub fn from_index(index: usize) -> Option<Self> { if index < 5 { Some(Self(index as u8)) } else { None } } }
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Registration<C> { pub area: Area, pub callbacks: C }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegistrationError { Closed, Geometry(GeometryError), Count, SlotsFull }
pub struct Service<C> { initialized: bool, slots: [Option<Registration<C>>; 5] }
impl<C> Default for Service<C> { fn default() -> Self { Self::new() } }
impl<C> Service<C> {
    pub const fn new() -> Self { Self { initialized: false, slots: [const { None }; 5] } }
    pub fn open(&mut self, staging: &mut impl Staging) -> Result<(), RegistrationError> {
        if self.initialized { return Err(RegistrationError::Closed); }
        open(staging);
        self.initialized = true;
        Ok(())
    }
    pub fn register(&mut self, kind: RecordKind, bytes: RecordBytes, total: RecordCount, q: SequenceOffset, callbacks: C) -> Result<Handle, RegistrationError> {
        if !self.initialized { return Err(RegistrationError::Closed); }
        let geometry = Geometry::new(bytes, RecordCount(total.0 / 2), q).map_err(RegistrationError::Geometry)?;
        if total.0 > 0x7ffe { return Err(RegistrationError::Count); }
        let Some((index, slot)) = self.slots.iter_mut().enumerate().find(|(_, slot)| slot.is_none()) else { return Err(RegistrationError::SlotsFull); };
        *slot = Some(Registration { area: Area::register(kind, geometry), callbacks });
        Ok(Handle(index as u8))
    }
    pub fn get_mut(&mut self, handle: Handle) -> Option<&mut Registration<C>> { self.slots.get_mut(handle.index())?.as_mut() }
    pub fn close(&mut self, handle: Handle) { if let Some(slot) = self.slots.get_mut(handle.index()) { *slot = None; } }
}
