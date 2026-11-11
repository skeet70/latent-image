use fujimod_emulator::session::Session;
use fujimod_emulator_devices::checkpoint::Checkpoint;
use serde::{Serialize, Serializer, ser::{self, Impossible, SerializeStruct}};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fmt, io::{self, Cursor, Write}};

const MAX_BYTES: usize = 2 * 1024 * 1024 * 1024;
const MAX_HEADERS: usize = 16 * 1024 * 1024;
const MAX_NODES: usize = 8192;
const MAX_DEPTH: usize = 64;
const MAX_METADATA: usize = 16 * 1024 * 1024;
const CHUNK_BYTES: usize = 1024 * 1024;
const EXCLUDED: [&str;3] = ["board.entries", "board.hooks", "board.observations"];

#[derive(Debug)]
struct CaptureError(String);
impl fmt::Display for CaptureError {fn fmt(&self,f:&mut fmt::Formatter<'_>)->fmt::Result {f.write_str(&self.0)}}
impl std::error::Error for CaptureError {}
impl ser::Error for CaptureError {fn custom<T:fmt::Display>(message:T)->Self {Self(message.to_string())}}
struct Header {kind:String,bytes:Vec<u8>}
enum Captured {Text(String),Bytes(Vec<u8>),Header(Header)}
struct Capture;
struct Fields {kind:Option<String>,bytes:Option<Vec<u8>>}
macro_rules! unsupported {
    ($($name:ident($($arg:ident:$ty:ty),*);)*) => {$(
        fn $name(self,$(_: $ty),*)->Result<Self::Ok,Self::Error> {Err(CaptureError("unexpected checkpoint serialization".into()))}
    )*};
}
impl Serializer for Capture {
    type Ok=Captured; type Error=CaptureError;
    type SerializeSeq=Impossible<Captured,CaptureError>;
    type SerializeTuple=Impossible<Captured,CaptureError>;
    type SerializeTupleStruct=Impossible<Captured,CaptureError>;
    type SerializeTupleVariant=Impossible<Captured,CaptureError>;
    type SerializeMap=Impossible<Captured,CaptureError>;
    type SerializeStruct=Fields;
    type SerializeStructVariant=Impossible<Captured,CaptureError>;
    fn serialize_str(self,value:&str)->Result<Captured,CaptureError> {Ok(Captured::Text(value.into()))}
    fn serialize_bytes(self,value:&[u8])->Result<Captured,CaptureError> {
        if value.len()>MAX_HEADERS {return Err(CaptureError("checkpoint header capacity exhausted".into()));}
        Ok(Captured::Bytes(value.to_vec()))
    }
    fn serialize_struct(self,name:&'static str,len:usize)->Result<Fields,CaptureError> {
        if name!="Checkpoint" || len!=4 {return Err(CaptureError("checkpoint serialization geometry differs".into()));}
        Ok(Fields{kind:None,bytes:None})
    }
    unsupported! {serialize_bool(v:bool);serialize_i8(v:i8);serialize_i16(v:i16);serialize_i32(v:i32);serialize_i64(v:i64);
        serialize_u8(v:u8);serialize_u16(v:u16);serialize_u32(v:u32);serialize_u64(v:u64);serialize_f32(v:f32);serialize_f64(v:f64);
        serialize_char(v:char);serialize_none();serialize_unit();serialize_unit_struct(name:&'static str);
        serialize_unit_variant(name:&'static str,index:u32,variant:&'static str);}
    fn serialize_some<T:?Sized+Serialize>(self,_:&T)->Result<Captured,CaptureError> {Err(CaptureError("unexpected checkpoint option".into()))}
    fn serialize_newtype_struct<T:?Sized+Serialize>(self,_:&'static str,_:&T)->Result<Captured,CaptureError> {Err(CaptureError("unexpected checkpoint newtype".into()))}
    fn serialize_newtype_variant<T:?Sized+Serialize>(self,_:&'static str,_:u32,_:&'static str,_:&T)->Result<Captured,CaptureError> {Err(CaptureError("unexpected checkpoint variant".into()))}
    fn serialize_seq(self,_:Option<usize>)->Result<Self::SerializeSeq,CaptureError> {Err(CaptureError("unexpected checkpoint sequence".into()))}
    fn serialize_tuple(self,_:usize)->Result<Self::SerializeTuple,CaptureError> {Err(CaptureError("unexpected checkpoint tuple".into()))}
    fn serialize_tuple_struct(self,_:&'static str,_:usize)->Result<Self::SerializeTupleStruct,CaptureError> {Err(CaptureError("unexpected checkpoint tuple".into()))}
    fn serialize_tuple_variant(self,_:&'static str,_:u32,_:&'static str,_:usize)->Result<Self::SerializeTupleVariant,CaptureError> {Err(CaptureError("unexpected checkpoint tuple".into()))}
    fn serialize_map(self,_:Option<usize>)->Result<Self::SerializeMap,CaptureError> {Err(CaptureError("unexpected checkpoint map".into()))}
    fn serialize_struct_variant(self,_:&'static str,_:u32,_:&'static str,_:usize)->Result<Self::SerializeStructVariant,CaptureError> {Err(CaptureError("unexpected checkpoint struct".into()))}
}
impl SerializeStruct for Fields {
    type Ok=Captured;type Error=CaptureError;
    fn serialize_field<T:?Sized+Serialize>(&mut self,key:&'static str,value:&T)->Result<(),CaptureError> {
        match key {
            "blobs"|"children"=>Ok(()),
            "kind"=>match value.serialize(Capture)? {Captured::Text(value) if self.kind.is_none()=>{self.kind=Some(value);Ok(())},_=>Err(CaptureError("checkpoint kind differs".into()))},
            "bytes"=>match value.serialize(Capture)? {Captured::Bytes(value) if self.bytes.is_none()=>{self.bytes=Some(value);Ok(())},_=>Err(CaptureError("checkpoint header differs".into()))},
            _=>Err(CaptureError("unknown checkpoint field".into())),
        }
    }
    fn end(self)->Result<Captured,CaptureError> {Ok(Captured::Header(Header{kind:self.kind.ok_or_else(||CaptureError("checkpoint kind absent".into()))?,bytes:self.bytes.ok_or_else(||CaptureError("checkpoint bytes absent".into()))?}))}
}
fn header(point:&Checkpoint)->Result<Header,String> {
    match point.serialize(Capture).map_err(|error|error.to_string())? {Captured::Header(value)=>Ok(value),_=>Err("checkpoint header capture differs".into())}
}
fn normalize_board(bytes:&[u8])->Result<Vec<u8>,String> {
    let mut cursor=Cursor::new(bytes);
    let count=match ciborium_ll::Decoder::from(&mut cursor).pull().map_err(|error|format!("board header: {error:?}"))? {
        ciborium_ll::Header::Map(Some(count)) if (3..=128).contains(&count)=>count,
        _=>return Err("board header is not a bounded definite map".into()),
    };
    let mut spans=Vec::with_capacity(count-3);
    let mut removed=[false;3];
    for _ in 0..count {
        let start=cursor.position() as usize;
        let name:String=ciborium::from_reader(&mut cursor).map_err(|error|format!("board header key: {error}"))?;
        let _:serde::de::IgnoredAny=ciborium::from_reader(&mut cursor).map_err(|error|format!("board header field: {error}"))?;
        match ["entries","hooks","observations"].iter().position(|key|*key==name) {
            Some(index) if !removed[index]=>removed[index]=true,
            Some(_)=>return Err("duplicate board observer field".into()),
            None=>spans.push((start,cursor.position() as usize)),
        }
    }
    if removed!=[true;3] || cursor.position() as usize!=bytes.len() {return Err("board observer normalization coverage differs".into());}
    let mut result=Vec::with_capacity(bytes.len());
    ciborium_ll::Encoder::from(&mut result).push(ciborium_ll::Header::Map(Some(count-3))).map_err(|error|format!("board header encode: {error}"))?;
    for (start,end) in spans {result.extend_from_slice(&bytes[start..end]);}
    Ok(result)
}
struct HashWriter {digest:Sha256,bytes:usize}
impl Write for HashWriter {
    fn write(&mut self,bytes:&[u8])->io::Result<usize> {
        let count=self.bytes.checked_add(bytes.len()).filter(|count|*count<=MAX_BYTES).ok_or_else(||io::Error::other("machine fingerprint capacity exhausted"))?;
        self.digest.update(bytes);self.bytes=count;Ok(bytes.len())
    }
    fn flush(&mut self)->io::Result<()> {Ok(())}
}
impl HashWriter {
    fn number(&mut self,value:usize)->Result<(),String> {self.write_all(&(value as u64).to_le_bytes()).map_err(|error|error.to_string())}
    fn field(&mut self,bytes:&[u8])->Result<(),String> {self.number(bytes.len())?;self.write_all(bytes).map_err(|error|error.to_string())}
}
struct Geometry {nodes:usize,headers:usize}

#[derive(Clone, Copy)]
enum ComponentKind {Checkpoint, Header, Blob, Chunk}
impl ComponentKind {
    fn name(self)->&'static str {
        match self {Self::Checkpoint=>"checkpoint",Self::Header=>"header",Self::Blob=>"blob",Self::Chunk=>"chunk"}
    }
}
struct Components {entries:Vec<Value>,retained:usize}
impl Components {
    fn reserve(&mut self,path:&str,kind:&str)->Result<usize,String> {
        // The charge includes JSON map/value storage, digest text and allocation overhead.
        let charge=2048usize.checked_add(path.len().checked_mul(8).ok_or("machine fingerprint metadata capacity exhausted")?)
            .and_then(|n|kind.len().checked_mul(8).and_then(|k|n.checked_add(k)))
            .ok_or("machine fingerprint metadata capacity exhausted")?;
        self.retained=self.retained.checked_add(charge).filter(|n|*n<=MAX_METADATA)
            .ok_or("machine fingerprint metadata capacity exhausted")?;
        let index=self.entries.len();
        self.entries.push(Value::Null);
        Ok(index)
    }
    fn finish(&mut self,index:usize,path:&str,kind:ComponentKind,checkpoint_kind:&str,bytes:usize,digest:&[u8;32],offset:Option<usize>) {
        let mut entry=json!({"path":path,"kind":kind.name(),"checkpoint_kind":checkpoint_kind,
            "logical_bytes":bytes,"sha256":hex(digest)});
        if let Some(offset)=offset {entry["offset"]=json!(offset);}
        self.entries[index]=entry;
    }
}
fn hex(bytes:&[u8])->String {
    use fmt::Write as _;
    let mut result=String::with_capacity(bytes.len()*2);
    for byte in bytes {write!(&mut result,"{byte:02x}").expect("string write");}
    result
}
fn digest_number(digest:&mut Sha256,value:usize) {digest.update((value as u64).to_le_bytes());}
fn digest_field(digest:&mut Sha256,bytes:&[u8]) {digest_number(digest,bytes.len());digest.update(bytes);}
fn component_digest(domain:&[u8])->Sha256 {
    let mut digest=Sha256::new();
    digest_field(&mut digest,b"af-machine-components-v1");
    digest_field(&mut digest,domain);
    digest
}
fn hash_tree(point:&Checkpoint,depth:usize,is_board:bool,path:&str,writer:&mut HashWriter,geometry:&mut Geometry,components:&mut Components)->Result<[u8;32],String> {
    geometry.nodes+=1;
    if depth>MAX_DEPTH || geometry.nodes>MAX_NODES {return Err("machine fingerprint tree capacity exhausted".into());}
    let mut header=header(point)?;
    if depth==0 && (header.kind!="emulator-session-v2" || point.children().len()!=2 || !point.blobs().is_empty()) {
        return Err("machine fingerprint session geometry differs".into());
    }
    geometry.headers=geometry.headers.checked_add(header.bytes.len()).filter(|bytes|*bytes<=MAX_HEADERS).ok_or("machine fingerprint header capacity exhausted")?;
    if is_board {
        if header.kind!="board-keyframe-v1" || point.children().len()!=2 || point.blobs().len()!=3 {return Err("machine fingerprint requires complete two-core board state".into());}
        header.bytes=normalize_board(&header.bytes)?;
    }
    let start=writer.bytes;
    let node_index=components.reserve(path,&header.kind)?;
    let header_path=format!("{path}/header");
    let header_index=components.reserve(&header_path,&header.kind)?;
    let header_digest:[u8;32]=Sha256::digest(&header.bytes).into();
    components.finish(header_index,&header_path,ComponentKind::Header,&header.kind,header.bytes.len(),&header_digest,None);
    let mut digest=component_digest(b"checkpoint");
    digest_field(&mut digest,header.kind.as_bytes());
    digest_number(&mut digest,header.bytes.len());
    digest.update(header_digest);
    writer.field(header.kind.as_bytes())?;writer.field(&header.bytes)?;
    writer.number(point.blobs().len())?;
    digest_number(&mut digest,point.blobs().len());
    for (index,blob) in point.blobs().iter().enumerate() {
        let blob_path=format!("{path}/blobs/{index}");
        let blob_index=components.reserve(&blob_path,&header.kind)?;
        let mut blob_digest=component_digest(b"blob");
        digest_number(&mut blob_digest,blob.len());
        digest_number(&mut blob_digest,blob.len().div_ceil(CHUNK_BYTES));
        writer.number(blob.len())?;
        for (chunk_index,chunk) in blob.chunks(CHUNK_BYTES).enumerate() {
            writer.write_all(chunk).map_err(|error|error.to_string())?;
            let chunk_path=format!("{blob_path}/chunks/{chunk_index}");
            let entry=components.reserve(&chunk_path,&header.kind)?;
            let chunk_digest:[u8;32]=Sha256::digest(chunk).into();
            digest_number(&mut blob_digest,chunk.len());
            blob_digest.update(chunk_digest);
            components.finish(entry,&chunk_path,ComponentKind::Chunk,&header.kind,chunk.len(),&chunk_digest,Some(chunk_index*CHUNK_BYTES));
        }
        let blob_digest:[u8;32]=blob_digest.finalize().into();
        components.finish(blob_index,&blob_path,ComponentKind::Blob,&header.kind,blob.len(),&blob_digest,None);
        digest_number(&mut digest,blob.len());
        digest.update(blob_digest);
    }
    writer.number(point.children().len())?;
    digest_number(&mut digest,point.children().len());
    for (index,child) in point.children().iter().enumerate() {
        let before=writer.bytes;
        let child_digest=hash_tree(child,depth+1,depth==0 && index==0,&format!("{path}/children/{index}"),writer,geometry,components)?;
        digest_number(&mut digest,writer.bytes-before);
        digest.update(child_digest);
    }
    let digest=digest.finalize().into();
    components.finish(node_index,path,ComponentKind::Checkpoint,&header.kind,writer.bytes-start,&digest,None);
    Ok(digest)
}
struct MetadataSize(usize);
impl Write for MetadataSize {
    fn write(&mut self,bytes:&[u8])->io::Result<usize> {
        self.0=self.0.checked_add(bytes.len()).filter(|n|*n<=MAX_METADATA)
            .ok_or_else(||io::Error::other("machine fingerprint metadata capacity exhausted"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self)->io::Result<()> {Ok(())}
}
pub fn fingerprint(session:&Session)->Result<Value,String> {
    let counts=session.board.instruction_counts();let now=session.board.now();
    if counts.len()!=2 {return Err("machine fingerprint requires both cores".into());}
    let stopped=session.checkpoint(None)?;
    if stopped.instruction_counts()!=counts.as_slice() || stopped.now()!=now {return Err("machine fingerprint capture advanced execution".into());}
    let encoded=stopped.export_checkpoint()?;
    let logical_bytes=encoded.bytes();
    if logical_bytes>MAX_BYTES {return Err("machine fingerprint state capacity exhausted".into());}
    let mut writer=HashWriter{digest:Sha256::new(),bytes:0};
    writer.field(b"af-machine-fingerprint-v1")?;
    let mut components=Components{entries:Vec::new(),retained:4096};
    let root=hash_tree(&encoded,0,false,"$",&mut writer,&mut Geometry{nodes:0,headers:0},&mut components)?;
    let logical_bytes=writer.bytes;
    if session.board.instruction_counts()!=counts || session.board.now()!=now {return Err("machine fingerprint changed execution state".into());}
    let mut metadata=json!({"schema":1,"algorithm":"sha256-hierarchical-v1","root_sha256":hex(&root),
        "chunk_bytes":CHUNK_BYTES,"entries":null});
    metadata["entries"]=Value::Array(components.entries);
    serde_json::to_writer(MetadataSize(0),&metadata).map_err(|error|error.to_string())?;
    let mut result=json!({"schema":1,"status":"complete","machine_sha256":format!("{:x}",writer.digest.finalize()),
        "counts":counts,"now":now,"logical_bytes":logical_bytes,
        "included_state":"session-v2/board-v1-complete-machine","excluded_host_fields":EXCLUDED});
    result["components"]=metadata;
    let exact = crate::exact_cpu_archive::retain(&encoded, counts.as_slice().try_into().map_err(|_| "exact archive core count differs")?, now, &session.definition)?;
    let retained_bytes = exact.cpu.iter().chain(std::iter::once(&exact.exclusive))
        .try_fold(components.retained, |n, blob| n.checked_add(blob.length.checked_mul(16).ok_or("exact archive capacity exhausted")?).ok_or("exact archive capacity exhausted"))?;
    if retained_bytes > MAX_METADATA { return Err("machine fingerprint aggregate metadata capacity exhausted".into()); }
    result["exact_state"] = serde_json::to_value(exact).map_err(|error| error.to_string())?;
    serde_json::to_writer(MetadataSize(0), &result).map_err(|error| error.to_string())?;
    crate::exact_cpu_archive::authenticate(&result)?;
    Ok(result)
}
pub fn snapshot_digest(path:&std::path::Path)->Result<String,String> {
    if std::fs::metadata(path).map_err(|error|error.to_string())?.len()>512*1024*1024 {
        return Err("reset snapshot retained-byte budget exhausted".into());
    }
    let mut file=std::fs::File::open(path).map_err(|error|format!("reset snapshot open: {error}"))?;
    let mut writer=HashWriter{digest:Sha256::new(),bytes:0};
    io::copy(&mut file,&mut writer).map_err(|error|format!("reset snapshot hash: {error}"))?;
    Ok(format!("{:x}",writer.digest.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalization_preserves_original_float_tuple_key_and_binary_encodings() {
        let mut input=vec![0xa5];
        for name in ["entries","hooks","observations"] {
            ciborium::into_writer(name,&mut input).unwrap();
            input.push(0x80);
        }
        let retained_start=input.len();
        ciborium::into_writer("float",&mut input).unwrap();
        input.push(0xfb);
        input.extend_from_slice(&0x7ff8_0000_0000_0123u64.to_be_bytes());
        ciborium::into_writer("tuple",&mut input).unwrap();
        input.extend_from_slice(&[0xa1,0x82,0x01,0x02,0x44,0x00,0xff,0x80,0x01]);
        let mut expected=vec![0xa2];
        expected.extend_from_slice(&input[retained_start..]);
        assert_eq!(normalize_board(&input).unwrap(),expected);
    }
}
