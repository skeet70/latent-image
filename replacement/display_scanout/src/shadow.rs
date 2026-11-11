use crate::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group { First, Second }
impl Group {
    pub const fn parse(value: u32) -> Option<Self> { match value { 0 => Some(Self::First), 1 => Some(Self::Second), _ => None } }
    pub const fn index(self) -> usize { match self { Self::First => 0, Self::Second => 1 } }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Item(u8);
impl Item {
    pub const fn parse(value: u32) -> Option<Self> { if value < 4 { Some(Self(value as u8)) } else { None } }
    pub const fn index(self) -> usize { self.0 as usize }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemSelection { One(Item), All }
impl ItemSelection {
    pub const fn parse(value: u32) -> Option<Self> { if value == 255 { Some(Self::All) } else { match Item::parse(value) { Some(item) => Some(Self::One(item)), None => None } } }
    fn mask(self) -> u32 { match self { Self::One(item) => 1 << (item.index()*4), Self::All => 0x1111 } }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MaskStorage { Saved, Alternate }
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ColorComponent { Red, Green, Blue }
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Field {
    Initialized,
    ItemConfiguration(Group,Item),
    RangeOrigin(Group,Item),
    FirstDimension(Group,Item),
    SecondDimension(Group,Item),
    Padding(Group,Item),
    Opacity(Group,Item),
    RangeMask(Group,MaskStorage),
    GroupControl(Group),
    GroupFormat(Group),
    PreparationFlags,
    GroupSelectors(Group),
    PrimaryFlags(MaskStorage),
    BackingMask(MaskStorage),
    BlankColor(ColorComponent),
    AlternateSelector,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldBits(pub u32);
pub trait Storage {
    fn read(&self,field:Field)->FieldBits;
    fn write(&mut self,field:Field,value:FieldBits);
}
fn replace(state:&mut impl Storage,field:Field,mask:u32,value:u32) {
    let old=state.read(field).0;
    state.write(field,FieldBits((old & !mask)|(value & mask)));
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ItemConfiguration(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RangePacket { pub first: u16, pub second: u16, pub first_dimension: u16, pub padding_limit: u16, pub second_dimension: u16, pub opacity: u8 }
impl RangePacket {
    pub fn parse(bytes: &[u8; 11]) -> Self {
        let half = |offset| u16::from_le_bytes([bytes[offset],bytes[offset+1]]);
        Self { first:half(0), second:half(2), first_dimension:half(4), padding_limit:half(6), second_dimension:half(8), opacity:bytes[10] }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GroupOptions { pub control:u32, pub format:u32, pub origin:u32, pub first_selector:u32, pub second_selector:u32 }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Blanking { pub selector:u8, pub color:[u8;3] }

pub fn item_configuration(state: &mut impl Storage, group: Group, item: Item, configuration: ItemConfiguration) {
    replace(state,Field::ItemConfiguration(group,item),0x3fff_ffff,configuration.0);
}
pub fn range_packet(state: &mut impl Storage, group: Group, item: Item, packet: RangePacket) {
    let field=Field::RangeOrigin(group,item);
    let upper=(state.read(field).0 & !0x3fff_0000)|((u32::from(packet.first)&0x3fff)<<16);
    state.write(field,FieldBits(upper));
    state.write(field,FieldBits((upper & !0x3fff)|(u32::from(packet.second)&0x3fff)));
    replace(state,Field::FirstDimension(group,item),0x1fff,u32::from(packet.first_dimension.wrapping_sub(1)));
    replace(state,Field::SecondDimension(group,item),0x1fff,u32::from(packet.second_dimension.wrapping_sub(1)));
    replace(state,Field::Padding(group,item),0x0fff,u32::from(packet.padding_limit.saturating_sub(packet.first_dimension)));
    state.write(Field::Opacity(group,item),FieldBits(u32::from(packet.opacity.min(128))));
}
pub fn range_enabled(state: &mut impl Storage, group: Group, item: ItemSelection, storage: MaskStorage, enabled: bool) -> Result<(),Error> {
    if state.read(Field::Initialized).0==0 { return Err(Error::Uninitialized); }
    let mask=item.mask();
    replace(state,Field::RangeMask(group,storage),mask,if enabled{mask}else{0});
    Ok(())
}
pub fn group_options(state:&mut impl Storage,group:Group,options:GroupOptions) {
    replace(state,Field::GroupControl(group),7,options.control);
    match group {
        Group::Second => {
            replace(state,Field::GroupFormat(group),15,options.format);
            replace(state,Field::PreparationFlags,0x30,options.origin<<4);
            replace(state,Field::GroupSelectors(group),0x0c,options.first_selector<<2);
            replace(state,Field::GroupSelectors(group),3,options.second_selector);
        }
        Group::First => {
            replace(state,Field::GroupFormat(group),0xf0,0);
            replace(state,Field::PreparationFlags,0x0c,options.origin<<2);
            replace(state,Field::GroupSelectors(group),0xc0,options.first_selector<<6);
            replace(state,Field::GroupSelectors(group),0x30,options.second_selector<<4);
        }
    }
}
pub fn blank(state:&mut impl Storage,packet:Blanking)->Result<(),Error> {
    if state.read(Field::Initialized).0==0 { return Err(Error::Uninitialized); }
    state.write(Field::AlternateSelector,FieldBits(u32::from(packet.selector)));
    state.write(Field::BlankColor(ColorComponent::Blue),FieldBits(u32::from(packet.color[2])));
    state.write(Field::BlankColor(ColorComponent::Green),FieldBits(u32::from(packet.color[1])));
    state.write(Field::BlankColor(ColorComponent::Red),FieldBits(u32::from(packet.color[0])));
    let (source,destination)=if packet.selector!=0 {(MaskStorage::Saved,MaskStorage::Alternate)}else{(MaskStorage::Alternate,MaskStorage::Saved)};
    let flags=state.read(Field::PrimaryFlags(source)).0;
    replace(state,Field::PrimaryFlags(destination),1,flags);
    let backing=state.read(Field::BackingMask(source));
    state.write(Field::BackingMask(destination),backing);
    state.write(Field::PrimaryFlags(source),FieldBits(flags & !1));
    state.write(Field::BackingMask(source),FieldBits(0));
    for group in [Group::First,Group::Second] {
        let mask=state.read(Field::RangeMask(group,source));
        state.write(Field::RangeMask(group,destination),mask);
        state.write(Field::RangeMask(group,source),FieldBits(0));
    }
    Ok(())
}
