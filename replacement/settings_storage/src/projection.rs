pub const BANK_STRIDE: usize = 0x1c2;
pub const BANK_WINDOW: usize = BANK_STRIDE * 3;
pub const PREVIOUS_WINDOW: usize = 0x1e0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsBank { Active, Alternate }
impl SettingsBank {
    pub const fn from_abi(value: u32) -> Option<Self> {
        match value { 0 => Some(Self::Active), 1 => Some(Self::Alternate), _ => None }
    }
    pub(crate) const fn offset(self) -> usize { match self { Self::Active => 0, Self::Alternate => BANK_STRIDE + if cfg!(storage_defect_wrong_bank_stride) { 2 } else { 0 } } }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileSelection { Current, Record0, Record2, Record3, WrappedPredecessor }
impl ProfileSelection {
    pub const fn from_abi(value: u32) -> Option<Self> {
        match value { 0 => Some(Self::Record0), 1 => Some(Self::Current), 2 => Some(Self::Record2), 3 => Some(Self::Record3), u32::MAX => Some(Self::WrappedPredecessor), _ => None }
    }
    pub(crate) const fn offset(self) -> usize {
        match self { Self::Record0 => 0x140, Self::Current => 0x168, Self::Record2 => 0x190, Self::Record3 => 0x1b8, Self::WrappedPredecessor => 0x118 }
    }
}

macro_rules! projection {
    ($name:ident, $size:expr) => {
        #[derive(Clone, Copy)]
        pub struct $name<'a>(pub(crate) &'a [u8; $size]);
        impl<'a> $name<'a> { pub fn bytes(self) -> &'a [u8; $size] { self.0 } }
    };
}
projection!(HeaderRecord, 16);
projection!(BodyRecord, 0x118);
projection!(AuxiliaryRecord, 24);
projection!(SlotRecord, 40);
projection!(RamTail, 40);
projection!(BankImage, BANK_STRIDE);
impl<'a> BankImage<'a> {
    pub fn header(self) -> HeaderRecord<'a> { HeaderRecord(self.0[..16].try_into().unwrap()) }
    pub fn body(self) -> BodyRecord<'a> { BodyRecord(self.0[16..0x128].try_into().unwrap()) }
    pub fn auxiliary(self) -> AuxiliaryRecord<'a> { AuxiliaryRecord(self.0[0x128..0x140].try_into().unwrap()) }
    pub fn record0(self) -> SlotRecord<'a> { SlotRecord(self.0[0x140..0x168].try_into().unwrap()) }
    pub fn tail(self) -> RamTail<'a> { RamTail(self.0[0x19a..].try_into().unwrap()) }
}
