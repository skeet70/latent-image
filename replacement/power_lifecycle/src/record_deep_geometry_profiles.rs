use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_image_geometry_query::profiles as calibration;
pub use calibration::{ConfigurationKey, GeometryKind, ProfileId};

pub struct ProfileGeometry;
impl ProfileGeometry {
    pub const fn new()->Self{Self}
    pub fn configuration_byte<M:GuestAddressSpace,E>(&mut self,context:&mut RecordContext<M,E>,key:ConfigurationKey)->Result<u32,MemoryError>{crate::record_configuration::helper_69f8fc(context,key.0)}
    pub fn configuration_half<M:GuestAddressSpace,E>(&mut self,context:&mut RecordContext<M,E>,key:ConfigurationKey)->Result<u32,MemoryError>{crate::record_configuration::helper_69fec8(context,key.0)}
    pub fn profile_row<M:GuestAddressSpace,E>(&mut self,context:&mut RecordContext<M,E>,profile:ProfileId)->Result<u32,MemoryError>{context.read_u32(GuestAddress::new(0xed7e08u32.wrapping_add(profile.0.wrapping_mul(16))))}
    pub fn rectangle<M:GuestAddressSpace,E>(&mut self,context:&mut RecordContext<M,E>,profile:ProfileId,kind:GeometryKind)->Result<[u32;4],MemoryError>{calibration::helper_ef5d44(context,profile.0,kind.0)}
    pub fn horizontal_scale<M:GuestAddressSpace,E>(&mut self,context:&mut RecordContext<M,E>,profile:ProfileId,kind:GeometryKind)->Result<u32,MemoryError>{calibration::helper_ef5ab8(context,profile.0,kind.0)}
    pub fn vertical_scale<M:GuestAddressSpace,E>(&mut self,context:&mut RecordContext<M,E>,profile:ProfileId,kind:GeometryKind)->Result<u32,MemoryError>{calibration::helper_ef5cc4(context,profile.0,kind.0)}
    pub fn alignment<M:GuestAddressSpace,E>(&mut self,context:&mut RecordContext<M,E>,profile:ProfileId)->Result<[u32;2],MemoryError>{calibration::helper_ef5b5c(context,profile.0)}
    pub fn full_rectangle<M:GuestAddressSpace,E>(&mut self,context:&mut RecordContext<M,E>,profile:ProfileId)->Result<[u32;4],MemoryError>{calibration::helper_ef5138(context,profile.0)}
    pub fn active_rectangle<M:GuestAddressSpace,E>(&mut self,context:&mut RecordContext<M,E>,profile:ProfileId)->Result<[u32;4],MemoryError>{calibration::helper_ef5190(context,profile.0)}
    pub fn horizontal_divisor<M:GuestAddressSpace,E>(&mut self,context:&mut RecordContext<M,E>,profile:ProfileId)->Result<u32,MemoryError>{calibration::helper_ef5a8c(context,profile.0)}
}
