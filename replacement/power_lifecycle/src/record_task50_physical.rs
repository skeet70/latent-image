use crate::record_address_space::GuestAddressSpace;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActuatorSnapshot { pub available: bool, pub position: u8, pub flags: u8 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PhysicalProgress<T = ()> { Waiting, Complete(T) }

pub trait Task50Physical<M: GuestAddressSpace> {
    type Error;
    fn synchronize_focus(&mut self, memory: &mut M, first: bool, position: i16, mode: u32) -> Result<PhysicalProgress, Self::Error>;
    fn synchronize_focus_current(&mut self, memory: &mut M) -> Result<PhysicalProgress, Self::Error>;
    fn synchronize_iris(&mut self, memory: &mut M) -> Result<PhysicalProgress, Self::Error>;
    fn actuator_snapshot(&mut self, memory: &mut M) -> Result<PhysicalProgress<ActuatorSnapshot>, Self::Error>;
}
