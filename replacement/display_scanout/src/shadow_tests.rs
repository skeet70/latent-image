use crate::Error;
use crate::shadow::*;
use std::collections::BTreeMap;
use std::vec::Vec;

struct Model { fields:BTreeMap<Field,FieldBits>, effects:Vec<(Field,FieldBits)> }
impl Model {
    fn new()->Self {let mut model=Self{fields:BTreeMap::new(),effects:Vec::new()};model.fields.insert(Field::Initialized,FieldBits(1));model}
    fn seed(&mut self,field:Field,value:u32){self.fields.insert(field,FieldBits(value));}
    fn value(&self,field:Field)->u32{self.read(field).0}
}
impl Storage for Model {
    fn read(&self,field:Field)->FieldBits{self.fields.get(&field).copied().unwrap_or(FieldBits(match field{Field::ItemConfiguration(..)|Field::RangeOrigin(..)=>0xa5a5a5a5,Field::FirstDimension(..)|Field::SecondDimension(..)|Field::Padding(..)|Field::RangeMask(..)|Field::BackingMask(..)=>0xa5a5,_=>0xa5}))}
    fn write(&mut self,field:Field,value:FieldBits){self.fields.insert(field,value);self.effects.push((field,value));}
}
fn item(value:u32)->Item{Item::parse(value).unwrap()}
#[test]
fn configuration_touches_only_the_requested_item() {
    for group in [Group::First,Group::Second] {for index in 0..4 {
        let mut state=Model::new();let field=Field::ItemConfiguration(group,item(index));
        item_configuration(&mut state,group,item(index),ItemConfiguration(0x12345678));
        assert_eq!(state.effects,[(field,FieldBits(0x92345678))]);
    }}
    assert_eq!(Item::parse(4),None);assert_eq!(Group::parse(2),None);
}
#[test]
fn range_packet_preserves_order_reserved_bits_and_zero_dimension_wrap() {
    let mut state=Model::new();let group=Group::Second;let item=item(2);
    state.seed(Field::RangeOrigin(group,item),0x5a5a5a5a);
    range_packet(&mut state,group,item,RangePacket::parse(&[0xff,0xff,0x12,0x34,0,0,0x23,0x41,0,0,0xff]));
    assert_eq!(state.effects,[(Field::RangeOrigin(group,item),FieldBits(0x7fff5a5a)),(Field::RangeOrigin(group,item),FieldBits(0x7fff7412)),(Field::FirstDimension(group,item),FieldBits(0xbfff)),(Field::SecondDimension(group,item),FieldBits(0xbfff)),(Field::Padding(group,item),FieldBits(0xa123)),(Field::Opacity(group,item),FieldBits(128))]);
}
#[test]
fn enabling_masks_only_requested_nibbles_in_selected_storage() {
    let mut state=Model::new();
    state.seed(Field::RangeMask(Group::First,MaskStorage::Saved),0xa5a5);
    range_enabled(&mut state,Group::First,ItemSelection::All,MaskStorage::Saved,true).unwrap();
    assert_eq!(state.value(Field::RangeMask(Group::First,MaskStorage::Saved)),0xb5b5);
    state.seed(Field::RangeMask(Group::Second,MaskStorage::Alternate),0x5a5a);
    range_enabled(&mut state,Group::Second,ItemSelection::One(item(3)),MaskStorage::Alternate,false).unwrap();
    assert_eq!(state.value(Field::RangeMask(Group::Second,MaskStorage::Alternate)),0x4a5a);
    state.seed(Field::Initialized,0);state.effects.clear();
    assert_eq!(range_enabled(&mut state,Group::First,ItemSelection::All,MaskStorage::Saved,false),Err(Error::Uninitialized));
    assert!(state.effects.is_empty());assert_eq!(ItemSelection::parse(4),None);
}
#[test]
fn options_keep_unselected_bits_and_order() {
    let mut state=Model::new();
    for field in [Field::GroupControl(Group::First),Field::GroupFormat(Group::First),Field::GroupSelectors(Group::First),Field::GroupControl(Group::Second),Field::GroupFormat(Group::Second),Field::GroupSelectors(Group::Second)]{state.seed(field,0xa5);}
    state.seed(Field::PreparationFlags,0x5a);
    group_options(&mut state,Group::First,GroupOptions{control:2,format:15,origin:1,first_selector:3,second_selector:2});
    assert_eq!(state.value(Field::GroupControl(Group::First)),0xa2);assert_eq!(state.value(Field::GroupFormat(Group::First)),5);assert_eq!(state.value(Field::PreparationFlags),0x56);assert_eq!(state.value(Field::GroupSelectors(Group::First)),0xe5);
    group_options(&mut state,Group::Second,GroupOptions{control:4,format:3,origin:2,first_selector:0,second_selector:2});
    assert_eq!(state.value(Field::GroupControl(Group::Second)),0xa4);assert_eq!(state.value(Field::GroupFormat(Group::Second)),0xa3);assert_eq!(state.value(Field::PreparationFlags),0x66);assert_eq!(state.value(Field::GroupSelectors(Group::Second)),0xa2);
}
#[test]
fn blanking_is_ordered_and_repeated_blank_loses_saved_values() {
    let mut state=Model::new();
    state.seed(Field::PrimaryFlags(MaskStorage::Saved),0xa5);state.seed(Field::PrimaryFlags(MaskStorage::Alternate),0x5a);
    for storage in [MaskStorage::Saved,MaskStorage::Alternate]{state.seed(Field::BackingMask(storage),0x7319);for group in [Group::First,Group::Second]{state.seed(Field::RangeMask(group,storage),0x9241);}}
    blank(&mut state,Blanking{selector:7,color:[0x12,0x34,0x56]}).unwrap();
    assert_eq!(&state.effects[..4],&[(Field::AlternateSelector,FieldBits(7)),(Field::BlankColor(ColorComponent::Blue),FieldBits(0x56)),(Field::BlankColor(ColorComponent::Green),FieldBits(0x34)),(Field::BlankColor(ColorComponent::Red),FieldBits(0x12))]);
    assert_eq!(state.value(Field::PrimaryFlags(MaskStorage::Saved)),0xa4);assert_eq!(state.value(Field::PrimaryFlags(MaskStorage::Alternate)),0x5b);assert_eq!(state.value(Field::BackingMask(MaskStorage::Saved)),0);
    blank(&mut state,Blanking{selector:0,color:[1,2,3]}).unwrap();assert_eq!(state.value(Field::BackingMask(MaskStorage::Saved)),0x7319);
    for _ in 0..2{blank(&mut state,Blanking{selector:1,color:[0;3]}).unwrap();}
    blank(&mut state,Blanking{selector:0,color:[0;3]}).unwrap();assert_eq!(state.value(Field::BackingMask(MaskStorage::Saved)),0);assert_eq!(state.value(Field::PrimaryFlags(MaskStorage::Saved))&1,0);
}
#[test]
fn uninitialized_blank_has_no_effects() {
    let mut state=Model::new();state.seed(Field::Initialized,0);
    assert_eq!(blank(&mut state,Blanking{selector:1,color:[1,2,3]}),Err(Error::Uninitialized));assert!(state.effects.is_empty());
}
