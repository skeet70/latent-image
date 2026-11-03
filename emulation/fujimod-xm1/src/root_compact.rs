//! Streaming lossless transcoder from a root `root-task.json` to its compact wire.
//! Each `bytes` array of a sample's `views` elements and of its `interrupt_stack` object moves to
//! `root-task.bytes` and is replaced by `{"offset":O,"length":L}`; references are contiguous in document order.
use serde::de::{self, DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::{fmt, fs::{self, File}, io::{self, BufReader, BufWriter, Read, Write}, path::Path, process::ExitCode};

const USAGE: &str = "fujimod-xm1-root-compact --receipt ROOT_TASK_JSON --output NEW_DIR";
const BLOB: &str = "root-task.bytes";

struct Hashing<R> { inner: R, hasher: Sha256, bytes: u64 }
impl<R: Read> Read for Hashing<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self.inner.read(buffer)?;
        self.hasher.update(&buffer[..count]);
        self.bytes += count as u64;
        Ok(count)
    }
}

struct Blob { writer: BufWriter<File>, hasher: Sha256, end: u64 }
impl Blob {
    fn append(&mut self, bytes: &[u8]) -> Result<Value, String> {
        self.writer.write_all(bytes).map_err(|error| error.to_string())?;
        self.hasher.update(bytes);
        let offset = self.end;
        self.end += bytes.len() as u64;
        Ok(json!({ "offset": offset, "length": bytes.len() }))
    }
}

/// JSON value that refuses duplicate object keys and non-integer numbers.
struct Strict;
impl<'de> DeserializeSeed<'de> for Strict {
    type Value = Value;
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> { deserializer.deserialize_any(StrictVisitor) }
}
struct StrictVisitor;
impl<'de> Visitor<'de> for StrictVisitor {
    type Value = Value;
    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result { formatter.write_str("integer-only JSON without duplicate keys") }
    fn visit_bool<E>(self, value: bool) -> Result<Value, E> { Ok(Value::Bool(value)) }
    fn visit_i64<E>(self, value: i64) -> Result<Value, E> { Ok(Value::Number(value.into())) }
    fn visit_u64<E>(self, value: u64) -> Result<Value, E> { Ok(Value::Number(value.into())) }
    fn visit_f64<E: de::Error>(self, _: f64) -> Result<Value, E> { Err(E::custom("non-integer JSON number")) }
    fn visit_str<E>(self, value: &str) -> Result<Value, E> { Ok(Value::String(value.to_owned())) }
    fn visit_string<E>(self, value: String) -> Result<Value, E> { Ok(Value::String(value)) }
    fn visit_unit<E>(self) -> Result<Value, E> { Ok(Value::Null) }
    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Value, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = sequence.next_element_seed(Strict)? { items.push(item); }
        Ok(Value::Array(items))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut object = Map::new();
        while let Some(key) = map.next_key::<String>()? {
            if object.contains_key(&key) { return Err(de::Error::custom(format!("duplicate JSON key {key}"))); }
            let value = map.next_value_seed(Strict)?;
            object.insert(key, value);
        }
        Ok(Value::Object(object))
    }
}

fn reference(holder: &mut Value, blob: &mut Blob) -> Result<(), String> {
    let Value::Object(holder) = holder else { return Err("byte holder is not an object".into()) };
    let Some(Value::Array(values)) = holder.get("bytes") else { return Err("byte holder lacks a bytes array".into()) };
    let bytes = values.iter().map(|value| value.as_u64().and_then(|byte| u8::try_from(byte).ok()))
        .collect::<Option<Vec<u8>>>().ok_or("bytes element is not an integer 0..255")?;
    let reference = blob.append(&bytes)?;
    holder.insert("bytes".into(), reference);
    Ok(())
}
fn compact_sample(sample: &mut Value, blob: &mut Blob) -> Result<(), String> {
    let Value::Object(object) = sample else { return Err("sample is not an object".into()) };
    let Some(Value::Array(views)) = object.get_mut("views") else { return Err("sample lacks a views array".into()) };
    for view in views { reference(view, blob)?; }
    match object.get_mut("interrupt_stack") {
        None | Some(Value::Null) => Ok(()),
        Some(stack @ Value::Object(_)) => reference(stack, blob),
        Some(_) => Err("interrupt_stack is neither null nor an object".into()),
    }
}

struct Samples<'a> { output: &'a mut BufWriter<File>, blob: &'a mut Blob }
impl<'de> DeserializeSeed<'de> for Samples<'_> {
    type Value = u64;
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<u64, D::Error> { deserializer.deserialize_seq(self) }
}
impl<'de> Visitor<'de> for Samples<'_> {
    type Value = u64;
    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result { formatter.write_str("root sample array") }
    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<u64, A::Error> {
        let mut count = 0;
        while let Some(mut sample) = sequence.next_element_seed(Strict)? {
            compact_sample(&mut sample, self.blob).map_err(|error| de::Error::custom(format!("sample {count}: {error}")))?;
            if count != 0 { self.output.write_all(b",").map_err(de::Error::custom)?; }
            serde_json::to_writer(&mut *self.output, &sample).map_err(de::Error::custom)?;
            count += 1;
        }
        Ok(count)
    }
}

struct Root<'a> { samples: Samples<'a> }
impl<'de> Visitor<'de> for Root<'_> {
    type Value = (Vec<(String, Value)>, u64);
    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result { formatter.write_str("root receipt object") }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let Root { samples } = self;
        let mut samples = Some(samples);
        let mut members: Vec<(String, Value)> = Vec::new();
        let mut count = None;
        while let Some(key) = map.next_key::<String>()? {
            if key == "compact" || members.iter().any(|(seen, _)| *seen == key) || (key == "samples" && count.is_some()) {
                return Err(de::Error::custom(format!("duplicate or reserved receipt key {key}")));
            }
            if key == "samples" {
                count = Some(map.next_value_seed(samples.take().ok_or_else(|| de::Error::custom("samples repeated"))?)?);
            } else {
                members.push((key, map.next_value_seed(Strict)?));
            }
        }
        Ok((members, count.ok_or_else(|| de::Error::custom("receipt lacks samples"))?))
    }
}

pub struct Summary { pub samples: u64, pub blob_bytes: u64, pub source_bytes: u64 }

/// Writes `root-task.json` and `root-task.bytes` into the new directory `output`.
pub fn transcode<R: Read>(input: R, output: &Path) -> Result<Summary, String> {
    fs::create_dir(output).map_err(|error| format!("output directory: {error}"))?;
    let part_path = output.join("samples.part");
    let mut part = BufWriter::new(File::create(&part_path).map_err(|error| error.to_string())?);
    let mut blob = Blob { writer: BufWriter::new(File::create(output.join(BLOB)).map_err(|error| error.to_string())?), hasher: Sha256::new(), end: 0 };
    let mut source = BufReader::with_capacity(1 << 20, Hashing { inner: input, hasher: Sha256::new(), bytes: 0 });
    let (members, count) = {
        let mut deserializer = serde_json::Deserializer::from_reader(&mut source);
        let parsed = deserializer.deserialize_map(Root { samples: Samples { output: &mut part, blob: &mut blob } }).map_err(|error| error.to_string())?;
        deserializer.end().map_err(|error| error.to_string())?;
        parsed
    };
    io::copy(&mut source, &mut io::sink()).map_err(|error| error.to_string())?;
    let Hashing { hasher: source_hasher, bytes: source_bytes, .. } = source.into_inner();
    part.flush().map_err(|error| error.to_string())?;
    drop(part);
    blob.writer.flush().map_err(|error| error.to_string())?;
    let metadata = json!({
        "schema": 1, "blob": BLOB, "blob_bytes": blob.end, "blob_sha256": hex(&blob.hasher.finalize()),
        "source_bytes": source_bytes, "source_sha256": hex(&source_hasher.finalize()),
    });
    let mut receipt = BufWriter::new(File::create(output.join("root-task.json")).map_err(|error| error.to_string())?);
    let write = |receipt: &mut BufWriter<File>, bytes: &[u8]| receipt.write_all(bytes).map_err(|error| error.to_string());
    write(&mut receipt, b"{")?;
    for (key, value) in &members {
        serde_json::to_writer(&mut receipt, key).map_err(|error| error.to_string())?;
        write(&mut receipt, b":")?;
        serde_json::to_writer(&mut receipt, value).map_err(|error| error.to_string())?;
        write(&mut receipt, b",")?;
    }
    write(&mut receipt, b"\"compact\":")?;
    serde_json::to_writer(&mut receipt, &metadata).map_err(|error| error.to_string())?;
    write(&mut receipt, b",\"samples\":[")?;
    io::copy(&mut File::open(&part_path).map_err(|error| error.to_string())?, &mut receipt).map_err(|error| error.to_string())?;
    write(&mut receipt, b"]}")?;
    receipt.flush().map_err(|error| error.to_string())?;
    fs::remove_file(&part_path).map_err(|error| error.to_string())?;
    Ok(Summary { samples: count, blob_bytes: blob.end, source_bytes })
}
fn hex(bytes: &[u8]) -> String { bytes.iter().map(|byte| format!("{byte:02x}")).collect() }

fn drive() -> Result<(), String> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let [flag_receipt, receipt, flag_output, output] = arguments.as_slice() else { return Err(USAGE.into()) };
    if flag_receipt != "--receipt" || flag_output != "--output" { return Err(USAGE.into()); }
    let input = File::open(receipt).map_err(|error| format!("receipt: {error}"))?;
    let summary = transcode(input, Path::new(output))?;
    println!("{}", json!({ "samples": summary.samples, "blob_bytes": summary.blob_bytes, "source_bytes": summary.source_bytes }));
    Ok(())
}
fn main() -> ExitCode { match drive() { Ok(()) => ExitCode::SUCCESS, Err(error) => { eprintln!("{error}"); ExitCode::FAILURE } } }

#[cfg(test)]
mod tests {
    use super::*;

    fn directory(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("root-compact-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        path
    }
    const ORIGINAL: &str = r#"{
  "schema": 1, "hits": [{"site": "start", "registers": {"r0": "0x1"}}],
  "samples": [
    {"hit": 0, "views": [{"object": "pool", "physical": 16, "bytes": [1, 2, 255]}, {"object": "code", "physical": 4, "bytes": [7]}],
     "kernels": [{"id": 104}], "interrupt_stack": null},
    {"hit": 1, "views": [{"object": "code", "physical": 8, "bytes": [0, 9]}], "interrupt_stack": {"low": 1879056316, "bytes": [196, 31]}}
  ],
  "final_menu": {"Ok": [1, 5, 2]}
}"#;

    #[test]
    fn receipt_values_survive_with_contiguous_references() {
        let output = directory("ok");
        let summary = transcode(ORIGINAL.as_bytes(), &output).unwrap();
        assert_eq!((summary.samples, summary.blob_bytes, summary.source_bytes), (2, 8, ORIGINAL.len() as u64));
        let blob = fs::read(output.join(BLOB)).unwrap();
        assert_eq!(blob, [1, 2, 255, 7, 0, 9, 196, 31]);
        let mut compact: Value = serde_json::from_slice(&fs::read(output.join("root-task.json")).unwrap()).unwrap();
        let metadata = compact.as_object_mut().unwrap().remove("compact").unwrap();
        assert_eq!(metadata["blob_bytes"], 8);
        for sample in compact["samples"].as_array_mut().unwrap() {
            let mut holders: Vec<&mut Value> = sample["views"].as_array_mut().unwrap().iter_mut().collect();
            for holder in holders.drain(..) {
                let (offset, length) = (holder["bytes"]["offset"].as_u64().unwrap() as usize, holder["bytes"]["length"].as_u64().unwrap() as usize);
                holder["bytes"] = json!(blob[offset..offset + length]);
            }
            if sample["interrupt_stack"].is_object() {
                let stack = &mut sample["interrupt_stack"];
                let (offset, length) = (stack["bytes"]["offset"].as_u64().unwrap() as usize, stack["bytes"]["length"].as_u64().unwrap() as usize);
                stack["bytes"] = json!(blob[offset..offset + length]);
            }
        }
        assert_eq!(compact, serde_json::from_str::<Value>(ORIGINAL).unwrap());
        fs::remove_dir_all(output).unwrap();
    }
    #[test]
    fn ambiguous_or_lossy_inputs_are_refused() {
        for (name, input) in [
            ("duplicate", ORIGINAL.replacen("\"hit\": 1,", "\"hit\": 1, \"hit\": 2,", 1)),
            ("float", ORIGINAL.replacen("\"physical\": 8", "\"physical\": 8.0", 1)),
            ("byte", ORIGINAL.replacen("[0, 9]", "[0, 256]", 1)),
            ("reserved", ORIGINAL.replacen("\"schema\": 1", "\"compact\": 1", 1)),
            ("trailing", format!("{ORIGINAL} {{}}")),
            ("missing-views", ORIGINAL.replacen("\"views\": [{\"object\": \"code\", \"physical\": 8, \"bytes\": [0, 9]}]", "\"other\": 0", 1)),
        ] {
            let output = directory(name);
            assert!(transcode(input.as_bytes(), &output).is_err(), "{name}");
            let _ = fs::remove_dir_all(output);
        }
    }
}
