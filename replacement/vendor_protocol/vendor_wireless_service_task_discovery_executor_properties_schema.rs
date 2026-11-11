//! X-M1 1.50 object property schema at 0x013f9c4c and 0x013f9fb8.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectPropertyId { Storage, Format, Protection, Size, Name, Modified, Parent, PersistentId, DisplayName, Dc87, Dc88, Dc89, DcD3, De93, De94, De97, De9a, De9b, De9c }
impl ObjectPropertyId {
    pub const fn wire(self) -> u16 {
        match self { Self::Storage => 0xdc01, Self::Format => 0xdc02, Self::Protection => 0xdc03,
            Self::Size => 0xdc04, Self::Name => 0xdc07, Self::Modified => 0xdc08,
            Self::Parent => 0xdc0b, Self::PersistentId => 0xdc41, Self::DisplayName => 0xdc44,
            Self::Dc87 => 0xdc87, Self::Dc88 => 0xdc88, Self::Dc89 => 0xdc89,
            Self::DcD3 => 0xdcd3, Self::De93 => 0xde93, Self::De94 => 0xde94,
            Self::De97 => 0xde97, Self::De9a => 0xde9a, Self::De9b => 0xde9b, Self::De9c => 0xde9c }
    }
    pub fn parse(wire: u16) -> Option<Self> { ALL.into_iter().find(|property| property.wire() == wire) }
}
const ALL: [ObjectPropertyId; 19] = [ObjectPropertyId::Storage, ObjectPropertyId::Format, ObjectPropertyId::Protection, ObjectPropertyId::Size, ObjectPropertyId::Name, ObjectPropertyId::Modified, ObjectPropertyId::Parent, ObjectPropertyId::PersistentId, ObjectPropertyId::DisplayName, ObjectPropertyId::Dc87, ObjectPropertyId::Dc88, ObjectPropertyId::Dc89, ObjectPropertyId::DcD3, ObjectPropertyId::De93, ObjectPropertyId::De94, ObjectPropertyId::De97, ObjectPropertyId::De9a, ObjectPropertyId::De9b, ObjectPropertyId::De9c];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertyDataType { U16, U32, U64, U128, String }
impl PropertyDataType {
    pub const fn wire(self) -> u16 { match self { Self::U16 => 4, Self::U32 => 6, Self::U64 => 8, Self::U128 => 10, Self::String => 0xffff } }
    pub const fn default_length(self) -> usize { match self { Self::U16 => 2, Self::U32 => 4, Self::U64 => 8, Self::U128 => 16, Self::String => 1 } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectFormatId { Association, Script, Audio, Wave, Video, Avi, Raf, UndefinedImage, Jpeg, ExifJpeg, Firmware, Undefined }
impl ObjectFormatId {
    pub const fn parse(wire: u16) -> Option<Self> {
        Some(match wire { 0x3001 => Self::Association, 0x3002 => Self::Script,
            0x3006 => Self::Audio, 0x3008 => Self::Wave, 0x300a => Self::Video,
            0x300d => Self::Avi, 0xb103 => Self::Raf, 0x3800 => Self::UndefinedImage,
            0x3801 => Self::Jpeg, 0x3808 => Self::ExifJpeg, 0xb802 => Self::Firmware,
            0xb003 => Self::Undefined, _ => return None })
    }
    pub const fn properties(self) -> &'static [u16] {
        match self {
            Self::Association => &[0xdc01,0xdc02,0xdc03,0xdc04,0xdc07,0xdc0b,0xdc41,0xdc44],
            Self::Script | Self::Audio | Self::Firmware | Self::Undefined => &[0xdc01,0xdc02,0xdc03,0xdc04,0xdc07,0xdc08,0xdc0b,0xdc41,0xdc44],
            Self::Wave => &[0xdc01,0xdc02,0xdc03,0xdc04,0xdc07,0xdc08,0xdc0b,0xdc41,0xdc44,0xdc89,0xde93,0xde94,0xde9a],
            Self::Video | Self::Avi => &[0xdc01,0xdc02,0xdc03,0xdc04,0xdc07,0xdc08,0xdc0b,0xdc41,0xdc44,0xdc87,0xdc88,0xdc89,0xde93,0xde94,0xde97,0xde9a,0xde9b,0xde9c],
            _ => &[0xdc01,0xdc02,0xdc03,0xdc04,0xdc07,0xdc08,0xdc0b,0xdc41,0xdc44,0xdc87,0xdc88,0xdcd3,0xdc0a],
        }
    }
}

pub struct ObjectPropertyDescriptor { pub id: ObjectPropertyId, pub data_type: PropertyDataType, pub writable: bool, pub group: u32, pub form: DescriptorForm }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescriptorForm { None, Range32, Boolean16, ThreeValues16, VideoCodec32, DateTime }
impl ObjectPropertyDescriptor {
    pub const fn for_property(id: ObjectPropertyId) -> Self {
        use ObjectPropertyId::*;
        let data_type = match id { Format | Protection | De94 | De97 => PropertyDataType::U16,
            Size => PropertyDataType::U64, PersistentId => PropertyDataType::U128,
            Name | Modified | DisplayName => PropertyDataType::String, _ => PropertyDataType::U32 };
        let group = match id { Dc87 | Dc88 | Dc89 | De93 | De94 | De9a => 2, DcD3 => 3, De97 | De9b | De9c => 4, _ => 1 };
        let writable = matches!(id, Name | DisplayName | De94 | De97 | De9b);
        let form = match id { Protection | De97 => DescriptorForm::Boolean16, De94 => DescriptorForm::ThreeValues16,
            De9b => DescriptorForm::VideoCodec32, Dc87 | Dc88 | Dc89 | DcD3 | De93 | De9a | De9c => DescriptorForm::Range32,
            Modified => DescriptorForm::DateTime, _ => DescriptorForm::None };
        Self { id, data_type, writable, group, form }
    }
    pub fn encode(&self, output: &mut [u8]) -> Option<usize> {
        let (flag, extra): (u8, &[u8]) = match self.form {
            DescriptorForm::None => (0, &[]), DescriptorForm::DateTime => (3, &[]),
            DescriptorForm::Range32 => (1, &[0,0,0,0,255,255,255,255,1,0,0,0]),
            DescriptorForm::Boolean16 => (2, &[2,0,0,0,1,0]),
            DescriptorForm::ThreeValues16 => (2, &[3,0,0,0,1,0,2,0]),
            DescriptorForm::VideoCodec32 => (2, &[2,0,0,0,0,0,109,106,112,103]),
        };
        let default_length = self.data_type.default_length();
        let length = 10 + default_length + extra.len();
        let bytes = output.get_mut(..length)?;
        bytes.fill(0);
        bytes[..2].copy_from_slice(&self.id.wire().to_le_bytes());
        bytes[2..4].copy_from_slice(&self.data_type.wire().to_le_bytes());
        bytes[4] = u8::from(self.writable);
        bytes[5 + default_length..9 + default_length].copy_from_slice(&self.group.to_le_bytes());
        bytes[9 + default_length] = flag;
        bytes[10 + default_length..].copy_from_slice(extra);
        Some(length)
    }
}
