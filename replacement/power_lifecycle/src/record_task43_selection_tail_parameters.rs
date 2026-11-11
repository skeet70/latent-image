//! X-M1 1.50 fixed parameter-storage returns, confirmed from image-5 literal loads at x70view 55436fc85c0560dd252b0410af6f2427fe198cd3.

use crate::record_address_space::{GuestAddress,GuestAddressSpace,MemoryError,RecordContext};
macro_rules! parameter {($name:ident,$address:expr)=>{
    pub fn $name<M:GuestAddressSpace,E>(_ctx:&mut RecordContext<M,E>)->Result<GuestAddress,MemoryError> {Ok(GuestAddress::new($address))}
};}
parameter!(helper_ef9b50,0x0169_d004);
parameter!(helper_ef9b6c,0x0169_d020);
parameter!(helper_ef9c54,0x0169_d110);
parameter!(helper_ef9d18,0x0169_d1ec);
parameter!(helper_ef9dc0,0x0169_d22c);
parameter!(helper_ef9ddc,0x0169_d254);
parameter!(helper_ef9e14,0x0169_d278);
parameter!(helper_ef9e4c,0x0169_d294);
parameter!(helper_ef9e68,0x0169_d29c);
parameter!(helper_ef9e84,0x0169_d2a8);
parameter!(helper_ef9ea0,0x0169_d2b0);
parameter!(helper_ef9ebc,0x0169_d2c4);
parameter!(helper_ef9ed8,0x0169_d2cc);
parameter!(helper_ef9ef4,0x0169_d2d4);
parameter!(helper_ef9f10,0x0169_d2dc);
parameter!(helper_ef9f64,0x0169_d308);
parameter!(helper_ef9fb8,0x0169_d31c);
parameter!(helper_efa1ac,0x0169_d324);
