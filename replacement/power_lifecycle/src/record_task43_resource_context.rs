use crate::record_address_space::{ByteCount, CallbackKey, GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceCallback { FocusReady, ProfileReady, SnapshotReady, ScanReady, ScanStep, SelectionRoute }

impl ResourceCallback {
    pub const fn key(self) -> CallbackKey {
        let value = match self {
            Self::FocusReady => 0x00f24170,
            Self::ProfileReady => 0x00f241a8,
            Self::SnapshotReady => 0x00f241d4,
            Self::ScanReady => 0x00f25758,
            Self::ScanStep => 0x00f25728,
            Self::SelectionRoute => 0x00f28e94,
        };
        match CallbackKey::new(value) { Some(key) => key, None => unreachable!() }
    }
}

pub fn clear<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, address: GuestAddress, length: ByteCount) -> Result<(), MemoryError> {
    for offset in 0..length.0 { context.write_u8(address.checked_add(offset)?, 0)?; }
    Ok(())
}

#[macro_export]
macro_rules! record_resource_word {
    ($context:expr, $address:expr) => { $context.read_u32(crate::record_address_space::GuestAddress::new($address as u32))? };
}

#[macro_export]
macro_rules! record_resource_half {
    ($context:expr, $address:expr) => { $context.read_u16(crate::record_address_space::GuestAddress::new($address as u32))? };
}

#[macro_export]
macro_rules! record_resource_byte {
    ($context:expr, $address:expr) => { $context.read_u8(crate::record_address_space::GuestAddress::new($address as u32))? };
}

#[macro_export]
macro_rules! record_resource_put {
    ($context:expr, $address:expr, $value:expr) => {{
        let address = crate::record_address_space::GuestAddress::new($address as u32);
        let value = $value;
        $context.write_u32(address, value as u32)?;
    }};
}

#[macro_export]
macro_rules! record_resource_put_half {
    ($context:expr, $address:expr, $value:expr) => {{
        let address = crate::record_address_space::GuestAddress::new($address as u32);
        let value = $value;
        $context.write_u16(address, value as u16)?;
    }};
}

#[macro_export]
macro_rules! record_resource_put_byte {
    ($context:expr, $address:expr, $value:expr) => {{
        let address = crate::record_address_space::GuestAddress::new($address as u32);
        let value = $value;
        $context.write_u8(address, value as u8)?;
    }};
}

#[macro_export]
macro_rules! record_resource_call {
    ($context:expr, $function:path) => { $function($context)? };
    ($context:expr, $function:path, $a:expr) => {{ let a = $a; $function($context, a)? }};
    ($context:expr, $function:path, $a:expr, $b:expr) => {{ let (a, b) = ($a, $b); $function($context, a, b)? }};
    ($context:expr, $function:path, $a:expr, $b:expr, $c:expr) => {{ let (a, b, c) = ($a, $b, $c); $function($context, a, b, c)? }};
    ($context:expr, $function:path, $a:expr, $b:expr, $c:expr, $d:expr) => {{ let (a, b, c, d) = ($a, $b, $c, $d); $function($context, a, b, c, d)? }};
    ($context:expr, $function:path, $a:expr, $b:expr, $c:expr, $d:expr, $e:expr) => {{ let (a, b, c, d, e) = ($a, $b, $c, $d, $e); $function($context, a, b, c, d, e)? }};
    ($context:expr, $function:path, $a:expr, $b:expr, $c:expr, $d:expr, $e:expr, $f:expr) => {{ let (a, b, c, d, e, f) = ($a, $b, $c, $d, $e, $f); $function($context, a, b, c, d, e, f)? }};
    ($context:expr, $function:path, $a:expr, $b:expr, $c:expr, $d:expr, $e:expr, $f:expr, $g:expr) => {{ let (a, b, c, d, e, f, g) = ($a, $b, $c, $d, $e, $f, $g); $function($context, a, b, c, d, e, f, g)? }};
    ($context:expr, $function:path, $a:expr, $b:expr, $c:expr, $d:expr, $e:expr, $f:expr, $g:expr, $h:expr) => {{ let (a, b, c, d, e, f, g, h) = ($a, $b, $c, $d, $e, $f, $g, $h); $function($context, a, b, c, d, e, f, g, h)? }};
    ($context:expr, $function:path, $a:expr, $b:expr, $c:expr, $d:expr, $e:expr, $f:expr, $g:expr, $h:expr, $i:expr) => {{ let (a, b, c, d, e, f, g, h, i) = ($a, $b, $c, $d, $e, $f, $g, $h, $i); $function($context, a, b, c, d, e, f, g, h, i)? }};
}
