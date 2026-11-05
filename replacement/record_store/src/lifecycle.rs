use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Handle(u8);
impl Handle { pub fn index(self) -> usize { self.0 as usize } pub fn from_index(index: usize) -> Option<Self> { if index < 5 { Some(Self(index as u8)) } else { None } } }
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Registration<C> { pub area: Area, pub callbacks: C }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegistrationError { Closed, Geometry(GeometryError), Count, SlotsFull }
// An active cell contains an initialized registration.
struct Cell<C> { active: bool, registration: core::mem::MaybeUninit<Registration<C>> }
impl<C> Cell<C> {
    fn close(&mut self) {
        if self.active {
            self.active = false;
            unsafe { self.registration.assume_init_drop(); }
        }
    }
}
impl<C> Drop for Cell<C> { fn drop(&mut self) { self.close(); } }
pub struct Service<C> { initialized: bool, slots: [Cell<C>; 5] }
impl<C> Default for Service<C> { fn default() -> Self { Self::new() } }
impl<C> Service<C> {
    pub const fn new() -> Self {
        Self { initialized: false, slots: [const { Cell { active: false, registration: core::mem::MaybeUninit::uninit() } }; 5] }
    }
    /// # Safety
    /// `pointer` must designate aligned, writable, uninitialized storage for one service.
    pub unsafe fn initialize_at(pointer: *mut Self) {
        core::ptr::addr_of_mut!((*pointer).initialized).write(false);
        let slots = core::ptr::addr_of_mut!((*pointer).slots).cast::<Cell<C>>();
        for index in 0..5 { core::ptr::addr_of_mut!((*slots.add(index)).active).write(false); }
    }
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
        let Some((index, slot)) = self.slots.iter_mut().enumerate().find(|(_, slot)| !slot.active) else { return Err(RegistrationError::SlotsFull); };
        slot.registration.write(Registration { area: Area::register(kind, geometry), callbacks });
        slot.active = true;
        Ok(Handle(index as u8))
    }
    pub fn get_mut(&mut self, handle: Handle) -> Option<&mut Registration<C>> {
        let slot = self.slots.get_mut(handle.index())?;
        if slot.active { Some(unsafe { slot.registration.assume_init_mut() }) } else { None }
    }
    pub fn close(&mut self, handle: Handle) { if let Some(slot) = self.slots.get_mut(handle.index()) { slot.close(); } }
}
