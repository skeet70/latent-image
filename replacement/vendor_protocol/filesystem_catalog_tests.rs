use crate::filesystem_catalog::*;
use crate::filesystem_format::{Cluster, FatKind, FormatClass, Geometry, Layout, MetadataStatus, SectorCount, VolumeDescriptor, WriteState};
use crate::filesystem_runtime::HostVolume;
use core::num::NonZeroU16;

fn layout()->Layout {
    Layout::select(&VolumeDescriptor {sectors:SectorCount(32768),geometry:Geometry {cylinders:32,heads:NonZeroU16::new(16).unwrap(),sectors_per_track:NonZeroU16::new(32).unwrap()},format:FormatClass::Tables,write_state:WriteState(0),metadata_status:MetadataStatus(0),sector_bytes:512,layout:None,next_free:Cluster(2),free_count:0}).unwrap()
}
fn root_sector(layout:&Layout)->usize {if matches!(layout.kind,FatKind::Fat12|FatKind::Fat16) {(layout.fat_start.0+2*layout.fat_sectors.0) as usize}else{cluster_sector(layout,layout.root).unwrap().0 as usize}}
fn fat_record(name:&[u8;11],attributes:u8,size:u32)->[u8;32] {let mut record=[0;32];record[..11].copy_from_slice(name);record[11]=attributes;record[28..32].copy_from_slice(&size.to_le_bytes());record}

#[test]
fn scan_filters_real_directory_records_and_resolves_case_insensitive_paths() {
    let layout=layout();let mut image=vec![0u8;32768*512];let offset=root_sector(&layout)*512;
    image[offset..offset+32].copy_from_slice(&fat_record(b"IMG00001JPG",32,1234));
    image[offset+32..offset+64].copy_from_slice(&fat_record(b"IMG00002RAF",33,5678));
    image[offset+64..offset+96].copy_from_slice(&fat_record(b"IGNORED JPG",32,9));image[offset+64]=0xe5;
    image[offset+96..offset+128].copy_from_slice(&fat_record(b"DCIM       ",16,0));
    let mut volume=HostVolume::adopt(&mut image,512).unwrap();let drive=Drive::parse(b'A').unwrap();let mut catalog=Catalog::new(&mut volume,layout,drive).unwrap();
    let root=MediaPath::root(drive);let mut scan=storage_path_pair_option(&mut catalog,&root,Pattern::parse("*.jpg").unwrap()).unwrap();
    let entry=scan.next().unwrap().unwrap();assert_eq!(entry.size,FileSize(1234));assert_eq!(scan.next().unwrap(),None);scan.close();
    let entry=catalog.resolve(&MediaPath::parse("A:/img00002.raf").unwrap()).unwrap();assert!(entry.attributes.protected());assert_eq!(entry.size,FileSize(5678));
    let mut scan=storage_catalog_scan_wrapper(&mut catalog,&root,Pattern::all()).unwrap();assert!(scan.next().unwrap().unwrap().attributes.directory());assert_eq!(scan.next().unwrap(),None);
}

#[test]
fn cancelled_scan_releases_media_for_a_new_scan() {
    let layout=layout();let mut image=vec![0u8;32768*512];let offset=root_sector(&layout)*512;image[offset..offset+32].copy_from_slice(&fat_record(b"ONE     JPG",32,1));
    let mut volume=HostVolume::adopt(&mut image,512).unwrap();let drive=Drive::parse(b'A').unwrap();let mut catalog=Catalog::new(&mut volume,layout,drive).unwrap();let root=MediaPath::root(drive);
    catalog.scan(&root,Pattern::all(),EntryFilter::All).unwrap().cancel();
    let mut scan=catalog.scan(&root,Pattern::all(),EntryFilter::Files).unwrap();assert_eq!(scan.next().unwrap().unwrap().size,FileSize(1));
}

#[test]
fn long_name_creation_metadata_commit_and_deletion_round_trip() {
    let layout=layout();let mut image=vec![0u8;32768*512];let mut volume=HostVolume::adopt(&mut image,512).unwrap();let drive=Drive::parse(b'A').unwrap();let mut catalog=Catalog::new(&mut volume,layout,drive).unwrap();let root=MediaPath::root(drive);
    let name=EntryName::parse("A long synthetic image.jpg").unwrap();let path=root.join(&name).unwrap();let mut entry=catalog.create(&root,name,CreateKind::File).unwrap();assert_eq!(entry.name,name);
    entry.size=FileSize(27);entry.valid_size=FileSize(27);entry.modified=FatTimestamp(0x58210000);commit_file(catalog.io(),&layout,&entry).unwrap();
    let stored=catalog.resolve(&path).unwrap();assert_eq!(stored.size,FileSize(27));assert_eq!(stored.modified,entry.modified);
    catalog.delete(&path).unwrap();assert_eq!(catalog.resolve(&path),Err(CatalogError::NotFound));
    assert_eq!(catalog.scan(&root,Pattern::all(),EntryFilter::All).unwrap().next().unwrap(),None);
}

#[test]
fn exfat_entry_sets_publish_64bit_sizes_and_contiguous_state() {
    let mut layout=layout();layout.kind=FatKind::ExFat;layout.root=Cluster(2);
    let mut image=vec![0u8;32768*512];let offset=cluster_sector(&layout,layout.root).unwrap().0 as usize*512;
    let mut records=[[0u8;32];3];records[0][0]=0x85;records[0][1]=2;records[0][4]=33;
    records[1][0]=0xc0;records[1][1]=3;records[1][3]=7;records[1][8..16].copy_from_slice(&4u64.to_le_bytes());records[1][20..24].copy_from_slice(&3u32.to_le_bytes());records[1][24..32].copy_from_slice(&0x100000004u64.to_le_bytes());records[2][0]=0xc1;
    for (index,unit) in "one.jpg".encode_utf16().enumerate() {records[2][2+index*2..4+index*2].copy_from_slice(&unit.to_le_bytes());}
    let mut sum=0u16;for (index,record) in records.iter().enumerate() {for (at,byte) in record.iter().enumerate() {if index!=0 || !matches!(at,2|3) {sum=sum.rotate_right(1).wrapping_add(u16::from(*byte));}}}records[0][2..4].copy_from_slice(&sum.to_le_bytes());
    for (index,record) in records.iter().enumerate() {image[offset+index*32..offset+(index+1)*32].copy_from_slice(record);}
    let mut volume=HostVolume::adopt(&mut image,512).unwrap();let drive=Drive::parse(b'A').unwrap();let mut catalog=Catalog::new(&mut volume,layout,drive).unwrap();let entry=catalog.resolve(&MediaPath::parse("A:/ONE.JPG").unwrap()).unwrap();assert_eq!(entry.size,FileSize(0x100000004));assert_eq!(entry.valid_size,FileSize(4));assert!(entry.contiguous);assert!(entry.attributes.readonly());
}

#[test]
fn wrong_drive_and_sector_failures_remain_errors() {
    let layout=layout();let mut image=[0u8;512];let mut volume=HostVolume::adopt(&mut image,512).unwrap();let mut catalog=Catalog::new(&mut volume,layout,Drive::parse(b'A').unwrap()).unwrap();
    assert_eq!(catalog.resolve(&MediaPath::parse("B:/ONE.JPG").unwrap()),Err(CatalogError::WrongDrive));assert!(matches!(catalog.resolve(&MediaPath::parse("A:/ONE.JPG").unwrap()),Err(CatalogError::Io(_))));
}

#[test]
fn directory_creation_nested_lookup_and_nonempty_delete() {
    let layout=layout();let mut image=vec![0u8;32768*512];let mut volume=HostVolume::adopt(&mut image,512).unwrap();let drive=Drive::parse(b'A').unwrap();let mut catalog=Catalog::new(&mut volume,layout,drive).unwrap();let root=MediaPath::root(drive);
    let directory_name=EntryName::parse("DCIM").unwrap();let directory=root.join(&directory_name).unwrap();let entry=catalog.create(&root,directory_name,CreateKind::Directory).unwrap();assert!(entry.attributes.directory());
    let file_name=EntryName::parse("IMG_0001.JPG").unwrap();let file=directory.join(&file_name).unwrap();catalog.create(&directory,file_name,CreateKind::File).unwrap();
    assert_eq!(catalog.delete(&directory),Err(CatalogError::DirectoryNotEmpty));assert_eq!(catalog.resolve(&file).unwrap().name,file_name);
    catalog.delete(&file).unwrap();catalog.delete(&directory).unwrap();assert_eq!(catalog.resolve(&directory),Err(CatalogError::NotFound));
}

#[test]
fn modified_timestamp_digits_follow_scan_record_written_timestamp() {
    let timestamp=FatTimestamp(((2026-1980)<<25)|(11<<21)|(10<<16)|(23<<11)|(7<<5)|29);
    assert_eq!(timestamp.digits(),*b"20261110230758");
    assert_eq!(FatTimestamp(0).digits(),*b"19800000000000");
}
