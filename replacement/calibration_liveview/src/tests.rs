use super::*;
use super::publication::*;
use std::{boxed::Box, vec, vec::Vec};

struct Store { primary_valid: bool, fixed_valid: bool, reads: Vec<(usize, usize)> }
impl CalibrationStorage for Store {
    fn read_primary(&mut self, bytes: &mut [u8]) {
        self.reads.push((0, bytes.len()));
        bytes.fill(0x35);
        if self.primary_valid { bytes[0x202..0x204].copy_from_slice(&[0x55,0xaa]); }
    }
    fn read_backup(&mut self, bytes: &mut [u8]) { self.reads.push((0x400000, bytes.len())); bytes.fill(0x79); }
    fn read_fixed(&mut self, offset: CalibrationOffset, bytes: &mut [u8]) {
        self.reads.push((offset.0, bytes.len()));
        bytes.fill(0x46);
        if self.fixed_valid && offset.0 == 0 && bytes.len() > 0x203 { bytes[0x202..0x204].copy_from_slice(&[0x55,0xaa]); }
    }
}
fn backing() -> Box<[u8; CACHE_SIZE]> { vec![0xa5; CACHE_SIZE].into_boxed_slice().try_into().ok().unwrap() }
#[test]
fn cold_primary_fallback_and_defaults_preserve_distinct_effects() {
    let defaults = Defaults::parse(&[0xf7,0,2,0,0xd8,0,0x17,0,0xff,0xff,0,0]).unwrap();
    for (primary_valid, fixed_valid, path) in [(true,false,LoadPath::Primary),(false,true,LoadPath::Fixed),(false,false,LoadPath::Defaults)] {
        let mut bytes = backing();
        let mut store = Store { primary_valid, fixed_valid, reads: Vec::new() };
        let mut cache = Calibration::borrow(&mut bytes);
        assert_eq!(cache.initialize(&mut store, &defaults), path);
        assert_eq!(&cache.bytes()[0x204..0x206], &[1,0x78]);
        assert_eq!(&store.reads[..2], &[(0,0x76800),(0x400000,0xfd00)]);
        match path {
            LoadPath::Primary => { assert_eq!(store.reads.len(), 2); assert_eq!(cache.bytes()[0x400000],0x79); assert_eq!(cache.bytes()[CACHE_SIZE-1],0xa5); }
            LoadPath::Fixed => { assert_eq!(store.reads[2],(0,FIXED_SIZE)); assert_eq!(cache.bytes()[0x400000],0); assert_eq!(cache.bytes()[0xd8],0x80); }
            LoadPath::Defaults => { assert_eq!(cache.personality().raw(),2); assert_eq!(cache.bytes()[0xd8],0x17); assert_eq!(&cache.bytes()[0x4069..0x406f], &[0;6]); }
        }
    }
}
#[test]
fn typed_reads_and_reload_use_byte_order_and_original_stamp_windows() {
    let mut bytes = backing();
    let mut cache = Calibration::borrow(&mut bytes);
    for (offset, byte) in [(0,0x80),(1,0xfe),(2,0xab),(3,0xcd)] { cache.set_byte(CalibrationOffset::parse(offset).unwrap(), CalibrationByte(byte)); }
    let offset = CalibrationOffset::parse(0).unwrap();
    assert_eq!(cache.read(offset,Width::Word),Some(CalibrationValue(0xcdabfe80)));
    assert_eq!(cache.read(offset,Width::ThreeBytes),Some(CalibrationValue(0xabfe80)));
    assert_eq!(cache.signed_byte(offset),SignedCalibrationValue(-128));
    assert_eq!(cache.signed_halfword(offset),Some(SignedCalibrationValue(-384)));
    assert_eq!(cache.byte_extended_halfword(offset),SignedCalibrationValue(128));
    assert_eq!(cache.read(CalibrationOffset::parse(CACHE_SIZE-1).unwrap(),Width::Word),None);
    let mut store = Store { primary_valid:false,fixed_valid:false,reads:Vec::new() };
    cache.reload(&mut store,ReloadWindow::parse(31));
    assert_eq!(store.reads,vec![(0x1f00,256)]);
    assert_eq!(cache.bytes()[0x1e8],0xa5);
    cache.reload(&mut store,ReloadWindow::parse(32));
    assert_eq!(store.reads.len(),1);
    cache.reload(&mut store,ReloadWindow::parse(255));
    assert_eq!(&cache.bytes()[0x1e8..0x1ea],&[1,0x78]);
}
#[derive(Debug,PartialEq,Eq)]
enum Effect { Store(usize,usize,u32), Dirty(usize,u8), Lock, Rebuild, Unlock, Copy, Convert(i16), WhiteBalance, Halfword(u16) }
enum InputSource { Separate, Shifted }
struct Publisher { input:[u8;200], current:[u8;200], dirty:[u8;16], descriptors:[bool;4], lanes:[bool;2], auxiliary:(u32,u32), effects:Vec<Effect>, source:InputSource }
impl Publisher { fn new() -> Self { Self { input:[0;200],current:[0;200],dirty:[0;16],descriptors:[false;4],lanes:[false;2],auxiliary:(0,0),effects:Vec::new(),source:InputSource::Separate } } }
fn read(bytes:&[u8], offset:usize, width:Width) -> ParameterBits { let mut result=0; for i in 0..width.bytes() { result |= u32::from(bytes[offset+i]) << (i*8); } ParameterBits(result) }
impl Publication for Publisher {
    fn input(&self,o:ParameterOffset,w:Width)->ParameterBits {
        match self.source {
            InputSource::Separate => read(&self.input,o.byte_offset(),w),
            InputSource::Shifted => {
                let mut result = 0;
                for index in 0..w.bytes() {
                    let offset = o.byte_offset()+index;
                    let byte = if offset < 4 { self.input[offset] } else { self.current[offset-4] };
                    result |= u32::from(byte) << (index*8);
                }
                ParameterBits(result)
            }
        }
    }
    fn current(&self,o:ParameterOffset,w:Width)->ParameterBits { read(&self.current,o.0,w) }
    fn store(&mut self,o:ParameterOffset,w:Width,v:ParameterBits) { self.effects.push(Effect::Store(o.0,w.bytes(),v.0)); self.current[o.0..o.0+w.bytes()].copy_from_slice(&v.0.to_le_bytes()[..w.bytes()]); }
    fn gate(&self)->bool { self.dirty[12]&128!=0 }
    fn dirty(&self,c:DirtyCell)->DirtyBits { DirtyBits(self.dirty[c.offset()]) }
    fn raise(&mut self,c:DirtyCell,b:DirtyBits) { self.dirty[c.offset()]|=b.0; self.effects.push(Effect::Dirty(c.offset(),b.0)); }
    fn descriptor(&self,d:Descriptor)->bool { self.descriptors[match d { Descriptor::First=>0,Descriptor::Second=>1,Descriptor::Third=>2,Descriptor::Fourth=>3 }] }
    fn lane(&self,l:ProcessingLane)->bool { self.lanes[match l { ProcessingLane::First=>0,ProcessingLane::Second=>1 }] }
    fn white_balance_mode(&self)->WhiteBalanceMode { WhiteBalanceMode(-2) }
    fn white_balance_copy(&mut self) { self.effects.push(Effect::Copy); }
    fn white_balance_convert(&mut self,m:WhiteBalanceMode) { self.effects.push(Effect::Convert(m.0)); }
    fn white_balance_publish(&mut self) { self.effects.push(Effect::WhiteBalance); }
    fn halfword_publish(&mut self,v:HalfwordParameter) { self.effects.push(Effect::Halfword(v.0)); }
    fn auxiliary_mode(&self)->AuxiliaryMode { AuxiliaryMode::parse(self.auxiliary.0) }
    fn auxiliary_value(&self)->ParameterBits { ParameterBits(self.auxiliary.1) }
    fn store_auxiliary_value(&mut self,v:ParameterBits) { self.auxiliary.1=v.0; }
    fn lock(&mut self) { self.effects.push(Effect::Lock); }
    fn rebuild(&mut self) { self.effects.push(Effect::Rebuild); }
    fn unlock(&mut self) { self.effects.push(Effect::Unlock); }
}
#[test]
fn film_is_published_before_dirty_and_rebuild_inside_lock() {
    let mut p=Publisher::new(); p.input[0x5c]=12; p.dirty[12]=128;
    publish_and_rebuild(&mut p);
    assert_eq!(p.effects,vec![Effect::Lock,Effect::Store(0x5c,4,12),Effect::Dirty(3,1),Effect::Store(0x8c,4,0),Effect::Store(0x98,4,0),Effect::Store(0x8c,4,0),Effect::Rebuild,Effect::Unlock]);
    assert_eq!(p.current[0x5c],12);
}
#[test]
fn field_widths_gate_and_equal_publication_are_observable() {
    let mut p=Publisher::new(); p.input[0xc4]=0x80; p.current[0xc6]=0xa9; p.input[0x70]=0xfe;
    publish(&mut p);
    assert_eq!(p.current[0xc4],0x80); assert_eq!(p.current[0xc6],0xa9); assert_eq!(p.current[0x70],0xfe); assert_eq!(p.dirty,[0;16]);
    p.effects.clear(); publish(&mut p);
    assert_eq!(p.effects,vec![Effect::Store(0x8c,4,0),Effect::Store(0x98,4,0),Effect::Store(0x8c,4,0)]);
}
#[test]
fn grouped_profile_change_and_white_balance_have_ordered_effects() {
    let mut p=Publisher::new(); p.input[0]=7; p.input[0x40]=0xff; p.dirty[12]=128; p.descriptors=[true,false,true,true]; p.lanes=[true,true];
    publish(&mut p);
    assert_eq!(&p.effects[..8], &[Effect::Store(0,4,7),Effect::Store(4,4,0),Effect::Store(8,4,0),Effect::Dirty(4,2),Effect::Dirty(4,4),Effect::Dirty(8,0x86),Effect::Dirty(9,1),Effect::Dirty(3,1)]);
    assert!(p.effects.windows(4).any(|w|w==[Effect::Copy,Effect::Convert(-2),Effect::Dirty(3,1),Effect::WhiteBalance]));
}
#[test]
fn auxiliary_one_shot_clears_request_and_raises_lane_specific_bits() {
    let mut p=Publisher::new(); p.input[0x8c]=1; p.input[0x90]=9; p.input[0x94]=4; p.dirty[13]=3; p.lanes=[true,true];
    publish(&mut p);
    assert_eq!(p.current[0x8c],0); assert_eq!(p.current[0x90],9); assert_eq!(p.current[0x94],4); assert_eq!(p.auxiliary.1,9); assert_eq!(p.dirty[4],24);
}

#[test]
fn overlapping_input_snapshots_each_group_but_not_later_groups() {
    let mut p = Publisher::new();
    p.source = InputSource::Shifted;
    p.input[..4].copy_from_slice(&1u32.to_le_bytes());
    p.current[..12].copy_from_slice(&[2u32.to_le_bytes(),3u32.to_le_bytes(),4u32.to_le_bytes()].concat());
    publish(&mut p);
    assert_eq!(&p.current[..12], &[1u32.to_le_bytes(),2u32.to_le_bytes(),3u32.to_le_bytes()].concat());
    assert_eq!(&p.current[12..16], &3u32.to_le_bytes());
}
