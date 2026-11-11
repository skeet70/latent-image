//! Sector-backed catalog shared by USB and wireless callers.
//! Source pin: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3.
use crate::filesystem_format::{Cluster, FatKind, FormatError, Layout, Sector};
use crate::filesystem_runtime::SectorIo;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogError { Io(FormatError), InvalidPath, InvalidName, InvalidDrive, WrongDrive, InvalidLayout, CorruptChain, CorruptEntry, NotFound, NotDirectory, ShortOutput, AlreadyExists, DirectoryFull, DirectoryNotEmpty, ReadOnly, File(crate::filesystem_file_cpu::FileError) }
impl From<FormatError> for CatalogError { fn from(value: FormatError) -> Self { Self::Io(value) } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Drive(u8);
impl Drive {
    pub fn parse(value: u8) -> Result<Self, CatalogError> { if value.is_ascii_uppercase() { Ok(Self(value)) } else { Err(CatalogError::InvalidDrive) } }
    pub const fn byte(self) -> u8 { self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EntryName { units: [u16; 255], length: u8 }
impl EntryName {
    pub fn parse(value: &str) -> Result<Self, CatalogError> {
        let mut name = Self { units: [0;255], length: 0 };
        for unit in value.encode_utf16() {
            if name.length == 255 || unit < 32 || matches!(unit, 34|42|47|58|60|62|63|92|124) { return Err(CatalogError::InvalidName); }
            name.units[usize::from(name.length)] = unit; name.length += 1;
        }
        if name.length == 0 || matches!(value, "."|"..") { return Err(CatalogError::InvalidName); } Ok(name)
    }
    pub fn units(&self) -> &[u16] { &self.units[..usize::from(self.length)] }
    pub fn ascii(&self, output: &mut [u8]) -> Result<usize, CatalogError> {
        let length = usize::from(self.length);
        if output.len() < length { return Err(CatalogError::ShortOutput); }
        for (destination, unit) in output.iter_mut().zip(self.units()) { if *unit > 127 { return Err(CatalogError::InvalidName); } *destination = *unit as u8; } Ok(length)
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MediaPath { units: [u16; 260], length: u16, drive: Drive }
impl MediaPath {
    pub fn parse(value: &str) -> Result<Self, CatalogError> {
        let bytes = value.as_bytes();
        if bytes.len() < 2 || bytes[1] != b':' { return Err(CatalogError::InvalidPath); }
        let mut path = Self { units: [0;260], length: 0, drive: Drive::parse(bytes[0])? };
        for component in value[2..].split(['/', '\\']).filter(|part| !part.is_empty()) { path = path.join(&EntryName::parse(component)?)?; } Ok(path)
    }
    pub fn root(drive: Drive) -> Self { Self { units: [0;260], length: 0, drive } }
    pub fn dcim(drive: Drive) -> Self { let mut path = Self::root(drive); path.units[..4].copy_from_slice(&[68,67,73,77]); path.length=4; path }
    pub fn drive(&self) -> Drive { self.drive }
    pub fn units(&self) -> &[u16] { &self.units[..usize::from(self.length)] }
    pub fn join(&self, name: &EntryName) -> Result<Self, CatalogError> {
        let mut path = *self; let mut start=usize::from(path.length);
        if start != 0 { if start == 260 { return Err(CatalogError::InvalidPath); } path.units[start]=47; start+=1; }
        let end=start+name.units().len(); if end>260 { return Err(CatalogError::InvalidPath); }
        path.units[start..end].copy_from_slice(name.units()); path.length=end as u16; Ok(path)
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Pattern { units: [u16;255], length: u8 }
impl Pattern {
    pub fn parse(value: &str) -> Result<Self, CatalogError> {
        let mut pattern=Self { units:[0;255],length:0 };
        for unit in value.encode_utf16() { if pattern.length==255 || unit<32 || matches!(unit,47|58|92) { return Err(CatalogError::InvalidName); } pattern.units[usize::from(pattern.length)]=unit; pattern.length+=1; }
        if pattern.length==0 { return Err(CatalogError::InvalidName); } Ok(pattern)
    }
    pub const fn all() -> Self { let mut units=[0;255]; units[0]=42; Self {units,length:1} }
    fn matches(&self, name: &EntryName) -> bool {
        let pattern=&self.units[..usize::from(self.length)];
        if pattern==[42,46,42] { return true; }
        let text=name.units(); let (mut p,mut t,mut star,mut retry)=(0,0,None,0);
        while t<text.len() {
            if p<pattern.len() && (pattern[p]==63 || fold(pattern[p])==fold(text[t])) { p+=1;t+=1; }
            else if p<pattern.len() && pattern[p]==42 { star=Some(p);p+=1;retry=t; }
            else if let Some(index)=star { retry+=1;t=retry;p=index+1; } else { return false; }
        }
        while p<pattern.len() && pattern[p]==42 { p+=1; } p==pattern.len()
    }
}
fn fold(unit:u16)->u16 { if (97..=122).contains(&unit) {unit-32} else {unit} }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Attributes(pub u16);
impl Attributes { pub fn readonly(self)->bool {self.0&1!=0} pub fn directory(self)->bool {self.0&16!=0} pub fn protected(self)->bool {self.readonly()} pub fn raw(self)->u16 {self.0} }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileSize(pub u64);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FatTimestamp(pub u32);
impl FatTimestamp {
    pub fn digits(self)->[u8;14] {
        let year=1980+(self.0>>25);
        let parts=[year/100,year%100,(self.0>>21)&15,(self.0>>16)&31,(self.0>>11)&31,(self.0>>5)&63,(self.0&31)*2];
        let mut output=[0;14];
        for (index,part) in parts.iter().enumerate() {output[index*2]=b'0'+(part/10) as u8;output[index*2+1]=b'0'+(part%10) as u8;}
        output
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EntryLocation { pub sector: Sector, pub offset: u16 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Entry { pub name: EntryName, pub attributes: Attributes, pub first_cluster: Cluster, pub size: FileSize, pub valid_size: FileSize, pub contiguous: bool, pub location: EntryLocation, pub created: FatTimestamp, pub modified: FatTimestamp, position: Cursor, delete_position: Cursor, long_count:u8 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryFilter { All, Files, Directories }
impl EntryFilter { fn accepts(self, attrs:Attributes)->bool { match self { Self::All=>true,Self::Files=>!attrs.directory(),Self::Directories=>attrs.directory() } } }

pub fn cluster_sector(layout:&Layout, cluster:Cluster)->Result<Sector,CatalogError> {
    if cluster.0<2 || cluster.0-2>=layout.clusters {return Err(CatalogError::CorruptChain);}
    (cluster.0-2).checked_mul(u32::from(layout.cluster_sectors)).and_then(|v|layout.data_start.0.checked_add(v)).map(Sector).ok_or(CatalogError::CorruptChain)
}
pub fn next_cluster(io:&mut impl SectorIo,layout:&Layout,cluster:Cluster)->Result<Option<Cluster>,CatalogError> {
    if !matches!(layout.sector_bytes,512|2048) || io.sector_bytes()!=layout.sector_bytes {return Err(CatalogError::InvalidLayout);}
    cluster_sector(layout,cluster)?;
    let (offset,width,mask,end)=match layout.kind {FatKind::Fat12=>(u64::from(cluster.0)*3/2,2,0xfff,0xff8),FatKind::Fat16=>(u64::from(cluster.0)*2,2,0xffff,0xfff8),FatKind::Fat32=>(u64::from(cluster.0)*4,4,0x0fff_ffff,0x0fff_fff8),FatKind::ExFat=>(u64::from(cluster.0)*4,4,u32::MAX,0xffff_fff8)};
    let bytes=u64::from(layout.sector_bytes); let mut buffer=[0;2048]; let mut packed=[0;4]; let mut loaded=None;
    for (index,output) in packed[..width].iter_mut().enumerate() {
        let at=offset+index as u64; let relative=at/bytes;
        if relative>=u64::from(layout.fat_sectors.0) {return Err(CatalogError::CorruptChain);}
        let sector=layout.fat_start.0.checked_add(relative as u32).ok_or(CatalogError::CorruptChain)?;
        if loaded!=Some(sector) {io.read(Sector(sector),&mut buffer[..usize::from(layout.sector_bytes)])?;loaded=Some(sector);}
        *output=buffer[(at%bytes) as usize];
    }
    let mut value=u32::from_le_bytes(packed); if layout.kind==FatKind::Fat12 && cluster.0&1!=0 {value>>=4;} value&=mask;
    if value>=end {Ok(None)} else {let next=Cluster(value);cluster_sector(layout,next)?;Ok(Some(next))}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Cursor { cluster:Option<Cluster>, fixed_sector:Sector, fixed_entries:u32, index:u32, sectors:u32, remaining:Option<u64>, contiguous:bool, ended:bool }
impl Cursor {
    fn root(layout:&Layout)->Self { let fixed=matches!(layout.kind,FatKind::Fat12|FatKind::Fat16); Self {cluster:if fixed {None}else{Some(layout.root)},fixed_sector:Sector(layout.fat_start.0+2*layout.fat_sectors.0),fixed_entries:u32::from(layout.root_entries),index:0,sectors:0,remaining:None,contiguous:false,ended:false} }
    fn directory(entry:&Entry)->Self { Self {cluster:Some(entry.first_cluster),fixed_sector:Sector(0),fixed_entries:0,index:0,sectors:0,remaining:if entry.contiguous {Some(entry.size.0/32)}else{None},contiguous:entry.contiguous,ended:false} }
    fn raw(&mut self,io:&mut impl SectorIo,layout:&Layout)->Result<Option<([u8;32],EntryLocation)>,CatalogError> {
        if self.ended || self.remaining==Some(0) {return Ok(None);}
        let entries=u32::from(layout.sector_bytes)/32;
        if self.cluster.is_some() && self.index>=entries*u32::from(layout.cluster_sectors) {
            self.sectors+=u32::from(layout.cluster_sectors); if self.sectors/u32::from(layout.cluster_sectors)>layout.clusters {return Err(CatalogError::CorruptChain);}
            let current=self.cluster.ok_or(CatalogError::CorruptChain)?;
            self.cluster=if self.contiguous {Some(Cluster(current.0.checked_add(1).ok_or(CatalogError::CorruptChain)?))} else {next_cluster(io,layout,current)?};
            self.index=0; if self.cluster.is_none() {self.ended=true;return Ok(None);}
        }
        if self.cluster.is_none() && self.index>=self.fixed_entries {self.ended=true;return Ok(None);}
        let first=match self.cluster {Some(cluster)=>cluster_sector(layout,cluster)?,None=>self.fixed_sector};
        let sector=Sector(first.0.checked_add(self.index/entries).ok_or(CatalogError::CorruptChain)?);let offset=(self.index%entries)*32;
        let mut buffer=[0;2048];io.read(sector,&mut buffer[..usize::from(layout.sector_bytes)])?;
        let mut raw=[0;32];raw.copy_from_slice(&buffer[offset as usize..offset as usize+32]);self.index+=1;
        if let Some(left)=&mut self.remaining {*left-=1;}
        Ok(Some((raw,EntryLocation {sector,offset:offset as u16})))
    }
}
fn le16(raw:&[u8],offset:usize)->u16 {u16::from_le_bytes([raw[offset],raw[offset+1]])}
fn le32(raw:&[u8],offset:usize)->u32 {u32::from_le_bytes([raw[offset],raw[offset+1],raw[offset+2],raw[offset+3]])}
fn le64(raw:&[u8],offset:usize)->u64 {u64::from(le32(raw,offset))|u64::from(le32(raw,offset+4))<<32}
fn short_name(raw:&[u8;32])->EntryName {
    let mut name=EntryName {units:[0;255],length:0};
    for (start,end,lower) in [(0,8,raw[12]&8!=0),(8,11,raw[12]&16!=0)] {
        let mut end=end;while end>start && raw[end-1]==32 {end-=1;}
        if start==8 && end>start {name.units[usize::from(name.length)]=46;name.length+=1;}
        for index in start..end {let mut byte=raw[index];if index==0 && byte==5 {byte=0xe5;}if lower {byte=byte.to_ascii_lowercase();}name.units[usize::from(name.length)]=u16::from(byte);name.length+=1;}
    } name
}
fn read_entry(io:&mut impl SectorIo,layout:&Layout,cursor:&mut Cursor)->Result<Option<Entry>,CatalogError> {
    let mut long=EntryName {units:[0xffff;255],length:0};let mut expected=0;let mut checksum=0;let mut delete_position=*cursor;let mut long_count=0;
    loop {
        let position=*cursor;
        let Some((raw,location))=cursor.raw(io,layout)? else {return Ok(None)};
        if raw[0]==0 {cursor.ended=true;return Ok(None);}
        if layout.kind==FatKind::ExFat {
            if raw[0]!=0x85 {continue;}
            let count=raw[1]; if count<2 {return Err(CatalogError::CorruptEntry);}
            let (stream,_)=cursor.raw(io,layout)?.ok_or(CatalogError::CorruptEntry)?;
            if stream[0]!=0xc0 {return Err(CatalogError::CorruptEntry);}
            let length=usize::from(stream[3]);if length==0 || usize::from(count)-1<length.div_ceil(15) {return Err(CatalogError::CorruptEntry);}
            let mut name=EntryName {units:[0;255],length:stream[3]};let mut written=0;
            let mut sum=0u16;for (index,byte) in raw.iter().enumerate() {if index!=2 && index!=3 {sum=sum.rotate_right(1).wrapping_add(u16::from(*byte));}}
            for byte in stream {sum=sum.rotate_right(1).wrapping_add(u16::from(byte));}
            for _ in 1..count {
                let (secondary,_)=cursor.raw(io,layout)?.ok_or(CatalogError::CorruptEntry)?;
                for byte in secondary {sum=sum.rotate_right(1).wrapping_add(u16::from(byte));}
                if secondary[0]==0xc1 {for index in 0..15 {if written<length {name.units[written]=le16(&secondary,2+2*index);written+=1;}}}
                else if secondary[0]&0xe0!=0xe0 {return Err(CatalogError::CorruptEntry);}
            }
            if written!=length || sum!=le16(&raw,2) {return Err(CatalogError::CorruptEntry);}
            if le64(&stream,8)>le64(&stream,24) {return Err(CatalogError::CorruptEntry);}
            return Ok(Some(Entry {name,attributes:Attributes(le16(&raw,4)),first_cluster:Cluster(le32(&stream,20)),size:FileSize(le64(&stream,24)),valid_size:FileSize(le64(&stream,8)),contiguous:stream[1]&2!=0,location,created:FatTimestamp(le32(&raw,8)),modified:FatTimestamp(le32(&raw,12)),position,delete_position:position,long_count:0}));
        }
        if raw[0]==0xe5 {expected=0;continue;}
        if raw[11]==15 {
            let ordinal=raw[0]&31;if ordinal==0 || ordinal>20 {expected=0;continue;}
            if raw[0]&64!=0 {long.units.fill(0xffff);long.length=0;expected=ordinal;checksum=raw[13];delete_position=position;long_count=ordinal;}
            if ordinal!=expected || raw[13]!=checksum {expected=0;long.units.fill(0xffff);continue;}
            for (index,offset) in [1,3,5,7,9,14,16,18,20,22,24,28,30].iter().enumerate() {let at=usize::from(ordinal-1)*13+index;if at<255 {long.units[at]=le16(&raw,*offset);}}
            expected-=1;continue;
        }
        if raw[11]&8!=0 || raw[0]==46 {expected=0;continue;}
        let sum=raw[..11].iter().fold(0u8,|sum,byte|sum.rotate_right(1).wrapping_add(*byte));
        let name=if expected==0 && long.units[0]!=0xffff && checksum==sum {long.length=long.units.iter().position(|unit|matches!(*unit,0|0xffff)).unwrap_or(255) as u8;long.units[usize::from(long.length)..].fill(0);long} else {short_name(&raw)};
        let valid_long=expected==0 && long.units[0]!=0xffff && checksum==sum;
        return Ok(Some(Entry {name,attributes:Attributes(u16::from(raw[11])),first_cluster:Cluster((if layout.kind==FatKind::Fat32 {u32::from(le16(&raw,20))<<16}else{0})|u32::from(le16(&raw,26))),size:FileSize(u64::from(le32(&raw,28))),valid_size:FileSize(u64::from(le32(&raw,28))),contiguous:false,location,created:FatTimestamp(le32(&raw,14)),modified:FatTimestamp(le32(&raw,22)),position,delete_position:if valid_long {delete_position}else{position},long_count:if valid_long {long_count}else{0}}));
    }
}

pub struct Catalog<'a,I:SectorIo> { io:&'a mut I,layout:Layout,drive:Drive }
impl<'a,I:SectorIo> Catalog<'a,I> {
    pub fn new(io:&'a mut I,layout:Layout,drive:Drive)->Result<Self,CatalogError> {
        if !matches!(layout.sector_bytes,512|2048) || io.sector_bytes()!=layout.sector_bytes || layout.cluster_sectors==0 {return Err(CatalogError::InvalidLayout);} Ok(Self {io,layout,drive})
    }
    pub fn current_drive(&self)->Drive {self.drive}
    fn directory(&mut self,path:&MediaPath)->Result<Cursor,CatalogError> {
        if path.drive!=self.drive {return Err(CatalogError::WrongDrive);}
        let mut cursor=Cursor::root(&self.layout);
        for component in path.units[..usize::from(path.length)].split(|unit|*unit==47) {
            if component.is_empty() {continue;}
            let mut found=None;
            while let Some(entry)=read_entry(self.io,&self.layout,&mut cursor)? {if entry.name.units().len()==component.len() && entry.name.units().iter().zip(component).all(|(a,b)|fold(*a)==fold(*b)) {found=Some(entry);break;}}
            let entry=found.ok_or(CatalogError::NotFound)?;if !entry.attributes.directory() {return Err(CatalogError::NotDirectory);}cursor=Cursor::directory(&entry);
        } Ok(cursor)
    }
    pub fn scan<'s>(&'s mut self,path:&MediaPath,pattern:Pattern,filter:EntryFilter)->Result<Scan<'s,'a,I>,CatalogError> {let cursor=self.directory(path)?;Ok(Scan {catalog:self,cursor,pattern,filter})}
    pub fn resolve(&mut self,path:&MediaPath)->Result<Entry,CatalogError> {
        let units=&path.units[..usize::from(path.length)];let split=units.iter().rposition(|unit|*unit==47);
        let start=split.map_or(0,|index|index+1);let mut parent=*path;parent.length=split.unwrap_or(0) as u16;
        let mut cursor=self.directory(&parent)?;
        while let Some(entry)=read_entry(self.io,&self.layout,&mut cursor)? {if entry.name.units().len()==units[start..].len() && entry.name.units().iter().zip(&units[start..]).all(|(a,b)|fold(*a)==fold(*b)) {return Ok(entry);}}
        Err(CatalogError::NotFound)
    }
}
pub struct Scan<'s,'a,I:SectorIo> {catalog:&'s mut Catalog<'a,I>,cursor:Cursor,pattern:Pattern,filter:EntryFilter}
impl<I:SectorIo> Scan<'_,'_,I> {
    pub fn next(&mut self)->Result<Option<Entry>,CatalogError> {while let Some(entry)=read_entry(self.catalog.io,&self.catalog.layout,&mut self.cursor)? {if self.filter.accepts(entry.attributes) && self.pattern.matches(&entry.name) {return Ok(Some(entry));}}Ok(None)}
    pub fn close(self) {}
    pub fn cancel(self) {}
}
pub fn storage_current_drive_wrapper<I:SectorIo>(catalog:&Catalog<'_,I>)->Drive {catalog.current_drive()}
pub fn storage_catalog_scan_wrapper<'s,'a,I:SectorIo>(catalog:&'s mut Catalog<'a,I>,path:&MediaPath,pattern:Pattern)->Result<Scan<'s,'a,I>,CatalogError> {catalog.scan(path,pattern,EntryFilter::Directories)}
pub fn storage_path_pair_option<'s,'a,I:SectorIo>(catalog:&'s mut Catalog<'a,I>,path:&MediaPath,pattern:Pattern)->Result<Scan<'s,'a,I>,CatalogError> {catalog.scan(path,pattern,EntryFilter::Files)}

fn write_record(io:&mut impl SectorIo,layout:&Layout,location:EntryLocation,record:&[u8;32])->Result<(),CatalogError> {
    let mut buffer=[0;2048];let bytes=usize::from(layout.sector_bytes);io.read(location.sector,&mut buffer[..bytes])?;
    let offset=usize::from(location.offset);buffer[offset..offset+32].copy_from_slice(record);io.write(location.sector,&buffer[..bytes])?;Ok(())
}
pub fn commit_file(io:&mut impl SectorIo,layout:&Layout,entry:&Entry)->Result<(),CatalogError> {
    let mut cursor=entry.position;
    let (mut primary,location)=cursor.raw(io,layout)?.ok_or(CatalogError::NotFound)?;
    if layout.kind!=FatKind::ExFat {
        let size=u32::try_from(entry.size.0).map_err(|_|CatalogError::CorruptEntry)?;
        primary[11]=entry.attributes.raw() as u8|32;
        primary[22..26].copy_from_slice(&entry.modified.0.to_le_bytes());
        primary[20..22].copy_from_slice(&((entry.first_cluster.0>>16) as u16).to_le_bytes());
        primary[26..28].copy_from_slice(&(entry.first_cluster.0 as u16).to_le_bytes());primary[28..32].copy_from_slice(&size.to_le_bytes());
        return write_record(io,layout,location,&primary);
    }
    if primary[0]!=0x85 {return Err(CatalogError::CorruptEntry);}
    let count=primary[1];
    let (mut stream,stream_location)=cursor.raw(io,layout)?.ok_or(CatalogError::CorruptEntry)?;
    if stream[0]!=0xc0 {return Err(CatalogError::CorruptEntry);}
    primary[4..6].copy_from_slice(&(entry.attributes.raw()|32).to_le_bytes());
    primary[12..16].copy_from_slice(&entry.modified.0.to_le_bytes());
    stream[1]=(stream[1]&!2)|if entry.contiguous {2}else{0};
    stream[8..16].copy_from_slice(&entry.valid_size.0.to_le_bytes());stream[20..24].copy_from_slice(&entry.first_cluster.0.to_le_bytes());stream[24..32].copy_from_slice(&entry.size.0.to_le_bytes());
    let mut sum=0u16;for (index,byte) in primary.iter().enumerate() {if index!=2 && index!=3 {sum=sum.rotate_right(1).wrapping_add(u16::from(*byte));}}
    for byte in stream {sum=sum.rotate_right(1).wrapping_add(u16::from(byte));}
    for _ in 1..count {let (raw,_)=cursor.raw(io,layout)?.ok_or(CatalogError::CorruptEntry)?;for byte in raw {sum=sum.rotate_right(1).wrapping_add(u16::from(byte));}}
    primary[2..4].copy_from_slice(&sum.to_le_bytes());
    write_record(io,layout,stream_location,&stream)?;write_record(io,layout,location,&primary)
}

impl<I:SectorIo> Catalog<'_,I> {
    pub fn allocation_bitmap(&mut self)->Result<Option<crate::filesystem_file_cpu::ExfatAllocation>,CatalogError> {
        if self.layout.kind!=FatKind::ExFat {return Ok(None);}
        let mut cursor=Cursor::root(&self.layout);
        while let Some((raw,_))=cursor.raw(self.io,&self.layout)? {
            if raw[0]==0 {break;}
            if raw[0]==0x81 && raw[1]&1==0 {return Ok(Some(crate::filesystem_file_cpu::ExfatAllocation {bitmap_first:Cluster(le32(&raw,20)),bitmap_bytes:le64(&raw,24)}));}
        } Err(CatalogError::CorruptEntry)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CreateKind { File, Directory }
impl<I:SectorIo> Catalog<'_,I> {
    pub fn io(&mut self)->&mut I {self.io}
    pub fn layout(&self)->Layout {self.layout}
    fn allocate(&mut self,after:Option<Cluster>)->Result<Cluster,CatalogError> {
        let bitmap=self.allocation_bitmap()?;
        crate::filesystem_file_cpu::allocate_cluster(self.io,&self.layout,bitmap,after).map_err(CatalogError::File)
    }
    fn zero_cluster(&mut self,cluster:Cluster)->Result<(),CatalogError> {
        let first=cluster_sector(&self.layout,cluster)?;let zero=[0;2048];
        for offset in 0..u32::from(self.layout.cluster_sectors) {self.io.write(Sector(first.0+offset),&zero[..usize::from(self.layout.sector_bytes)])?;}Ok(())
    }
    fn free_run(&mut self,mut cursor:Cursor,count:usize)->Result<Cursor,CatalogError> {
        let mut start=cursor;let mut available=0;
        loop {
            let before=cursor;
            let raw=match cursor.raw(self.io,&self.layout)? {
                Some((raw,_))=>raw,
                None=>{
                    let previous=before.cluster.ok_or(CatalogError::DirectoryFull)?;
                    if before.contiguous {return Err(CatalogError::DirectoryFull);}
                    let cluster=self.allocate(Some(previous))?;self.zero_cluster(cluster)?;
                    cursor=Cursor {cluster:Some(cluster),index:0,ended:false,..before};continue;
                }
            };
            let free=if self.layout.kind==FatKind::ExFat {raw[0]&128==0}else{matches!(raw[0],0|0xe5)};
            if free {if available==0 {start=before;}available+=1;if available==count {return Ok(start);}}else{available=0;}
        }
    }
    pub fn create(&mut self,parent:&MediaPath,name:EntryName,kind:CreateKind)->Result<Entry,CatalogError> {
        let path=parent.join(&name)?;
        match self.resolve(&path) {Ok(_)=>return Err(CatalogError::AlreadyExists),Err(CatalogError::NotFound)=>{},Err(error)=>return Err(error)}
        let directory=self.directory(parent)?;
        let exfat=self.layout.kind==FatKind::ExFat;
        let count=if exfat {2+name.units().len().div_ceil(15)}else{1+name.units().len().div_ceil(13)};
        let mut position=match self.free_run(directory,count) {
            Ok(position)=>position,
            Err(CatalogError::DirectoryFull) if directory.contiguous=>{
                let mut entry=self.resolve(parent)?;
                let cluster_bytes=u64::from(self.layout.sector_bytes)*u64::from(self.layout.cluster_sectors);
                let clusters=u32::try_from(entry.size.0.div_ceil(cluster_bytes)).map_err(|_|CatalogError::CorruptChain)?;
                for offset in 0..clusters {
                    let cluster=Cluster(entry.first_cluster.0.checked_add(offset).ok_or(CatalogError::CorruptChain)?);
                    let next=if offset+1==clusters {u32::MAX}else{cluster.0.checked_add(1).ok_or(CatalogError::CorruptChain)?};
                    crate::filesystem_file_cpu::set_fat_entry(self.io,&self.layout,cluster,next).map_err(CatalogError::File)?;
                }
                entry.contiguous=false;commit_file(self.io,&self.layout,&entry)?;
                let directory=self.directory(parent)?;self.free_run(directory,count)?
            }
            Err(error)=>return Err(error)
        };
        let first=if kind==CreateKind::Directory {let cluster=self.allocate(None)?;self.zero_cluster(cluster)?;cluster}else{Cluster(0)};
        let mut records=[[0u8;32];21];
        if exfat {
            records[0][0]=0x85;records[0][1]=(count-1) as u8;records[0][4]=if kind==CreateKind::Directory {16}else{32};
            records[1][0]=0xc0;records[1][1]=1;records[1][3]=name.length;
            records[1][20..24].copy_from_slice(&first.0.to_le_bytes());
            let size=if kind==CreateKind::Directory {u64::from(self.layout.sector_bytes)*u64::from(self.layout.cluster_sectors)}else{0};
            records[1][8..16].copy_from_slice(&size.to_le_bytes());records[1][24..32].copy_from_slice(&size.to_le_bytes());
            for (index,unit) in name.units().iter().enumerate() {let record=&mut records[2+index/15];record[0]=0xc1;let offset=2+(index%15)*2;record[offset..offset+2].copy_from_slice(&unit.to_le_bytes());}
            let mut hash=0u16;for unit in name.units() {for byte in fold(*unit).to_le_bytes() {hash=hash.rotate_right(1).wrapping_add(u16::from(byte));}}
            records[1][4..6].copy_from_slice(&hash.to_le_bytes());
            let mut checksum=0u16;for (index,record) in records[..count].iter().enumerate() {for (offset,byte) in record.iter().enumerate() {if index!=0 || !matches!(offset,2|3) {checksum=checksum.rotate_right(1).wrapping_add(u16::from(*byte));}}}
            records[0][2..4].copy_from_slice(&checksum.to_le_bytes());
        } else {
            let mut short=[32u8;11];
            let stem=name.units().iter().rposition(|unit|*unit==46).unwrap_or(name.units().len());
            let extension=if stem<name.units().len() {&name.units()[stem+1..]}else{&[]};
            let mut chosen=None;
            for sequence in 1..=999999u32 {
                short.fill(32);let mut digits=[0;6];let mut value=sequence;let mut used=0;loop {digits[5-used]=(value%10) as u8+48;used+=1;value/=10;if value==0 {break;}}
                let prefix=7-used;for (index,unit) in name.units()[..stem].iter().take(prefix).enumerate() {let unit=fold(*unit);short[index]=if unit<128 && (unit as u8).is_ascii_alphanumeric() {unit as u8}else{b'_'};}
                short[prefix]=b'~';short[prefix+1..8].copy_from_slice(&digits[6-used..]);
                for (index,unit) in extension.iter().take(3).enumerate() {let unit=fold(*unit);short[8+index]=if unit<128 && (unit as u8).is_ascii_alphanumeric() {unit as u8}else{b'_'};}
                let mut check=directory;let mut collision=false;
                while let Some((raw,_))=check.raw(self.io,&self.layout)? {if raw[0]==0 {break;}if raw[0]!=0xe5 && raw[..11]==short {collision=true;break;}}
                if !collision {chosen=Some(short);break;}
            }
            let short=chosen.ok_or(CatalogError::DirectoryFull)?;let checksum=short.iter().fold(0u8,|sum,byte|sum.rotate_right(1).wrapping_add(*byte));
            for index in 0..count-1 {
                let ordinal=count-1-index;let raw=&mut records[index];raw.fill(255);raw[0]=ordinal as u8|if index==0 {64}else{0};raw[11]=15;raw[12]=0;raw[13]=checksum;raw[26]=0;raw[27]=0;
                for (unit_index,offset) in [1,3,5,7,9,14,16,18,20,22,24,28,30].iter().enumerate() {let at=(ordinal-1)*13+unit_index;let unit=if at<name.units().len() {name.units()[at]}else if at==name.units().len() {0}else{0xffff};raw[*offset..*offset+2].copy_from_slice(&unit.to_le_bytes());}
            }
            let raw=&mut records[count-1];raw[..11].copy_from_slice(&short);raw[11]=if kind==CreateKind::Directory {16}else{32};
            raw[20..22].copy_from_slice(&((first.0>>16) as u16).to_le_bytes());raw[26..28].copy_from_slice(&(first.0 as u16).to_le_bytes());
            if kind==CreateKind::Directory {
                let first_sector=cluster_sector(&self.layout,first)?;let mut buffer=[0u8;2048];
                for (index,cluster) in [first,directory.cluster.unwrap_or(Cluster(0))].iter().enumerate() {let raw=&mut buffer[index*32..index*32+32];raw[..11].fill(32);raw[0]=46;if index==1 {raw[1]=46;}raw[11]=16;raw[20..22].copy_from_slice(&((cluster.0>>16) as u16).to_le_bytes());raw[26..28].copy_from_slice(&(cluster.0 as u16).to_le_bytes());}
                self.io.write(first_sector,&buffer[..usize::from(self.layout.sector_bytes)])?;
            }
        }
        for record in &records[..count] {let (_,location)=position.raw(self.io,&self.layout)?.ok_or(CatalogError::DirectoryFull)?;write_record(self.io,&self.layout,location,record)?;}
        if exfat && parent.length!=0 {
            let mut parent_entry=self.resolve(parent)?;
            if !parent_entry.contiguous {
                let mut cluster=Some(parent_entry.first_cluster);let mut count=0u64;
                while let Some(current)=cluster {count+=1;if count>u64::from(self.layout.clusters) {return Err(CatalogError::CorruptChain);}cluster=next_cluster(self.io,&self.layout,current)?;}
                let bytes=count*u64::from(self.layout.sector_bytes)*u64::from(self.layout.cluster_sectors);
                if parent_entry.size.0!=bytes {parent_entry.size=FileSize(bytes);parent_entry.valid_size=FileSize(bytes);commit_file(self.io,&self.layout,&parent_entry)?;}
            }
        }
        self.io.flush()?;self.resolve(&path)
    }
    pub fn delete(&mut self,path:&MediaPath)->Result<(),CatalogError> {
        let entry=self.resolve(path)?;if entry.attributes.readonly() {return Err(CatalogError::ReadOnly);}
        if entry.attributes.directory() {let mut cursor=Cursor::directory(&entry);if read_entry(self.io,&self.layout,&mut cursor)?.is_some() {return Err(CatalogError::DirectoryNotEmpty);}}
        let bitmap=self.allocation_bitmap()?;
        let mut cursor=entry.position;let (mut raw,location)=cursor.raw(self.io,&self.layout)?.ok_or(CatalogError::NotFound)?;
        if self.layout.kind==FatKind::ExFat {
            let count=raw[1];raw[0]&=127;write_record(self.io,&self.layout,location,&raw)?;
            for _ in 0..count {let (mut raw,location)=cursor.raw(self.io,&self.layout)?.ok_or(CatalogError::CorruptEntry)?;raw[0]&=127;write_record(self.io,&self.layout,location,&raw)?;}
        }else{
            raw[0]=0xe5;write_record(self.io,&self.layout,location,&raw)?;
            let mut long=entry.delete_position;
            for _ in 0..entry.long_count {let (mut raw,location)=long.raw(self.io,&self.layout)?.ok_or(CatalogError::CorruptEntry)?;raw[0]=0xe5;write_record(self.io,&self.layout,location,&raw)?;}
        }
        if entry.first_cluster.0>=2 {crate::filesystem_file_cpu::release_chain(self.io,&self.layout,bitmap,entry.first_cluster,entry.size.0,entry.contiguous).map_err(CatalogError::File)?;}
        self.io.flush()?;Ok(())
    }
}
