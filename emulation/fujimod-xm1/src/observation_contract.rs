use crate::{exact_cpu_archive::{self, CheckpointModelId, ImmutableBindings, Sha256Identity}, run::{self, RunDocument}};
use fujimod_emulator::board::factory::MemoryMap;
use serde::{Serialize, Serializer};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, ffi::OsString, fs::File, io::Read, path::Path};

const MAX_INPUT: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy)]
enum Operation { ValidateFingerprint, MappingCatalogue }
#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum Status { Complete }
struct WireDigest(Sha256Identity);
impl Serialize for WireDigest {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&exact_cpu_archive::hex(&self.0.0))
    }
}
struct WireModel(CheckpointModelId);
impl Serialize for WireModel {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.0)
    }
}
#[derive(Serialize)]
pub struct Validation {
    schema: u32, status: Status,
    checkpoint_model_id: WireModel,
    model_definition_sha256: WireDigest,
    executable_sha256: WireDigest,
    machine_sha256: WireDigest,
    counts: [u64; 2], now: u64,
}
#[derive(Serialize)]
pub struct Catalogue {
    schema: u32, status: Status,
    checkpoint_model_id: WireModel,
    executable_sha256: WireDigest,
    run_document_sha256: WireDigest,
    #[serde(serialize_with = "crate::af_ownership::serialize_mappings")]
    mappings: Vec<MemoryMap>,
}
#[derive(Serialize)]
#[serde(untagged)]
pub enum Response { Validation(Validation), Catalogue(Catalogue) }

fn digest(text: &str) -> Result<Sha256Identity, String> {
    if text.len() != 64 { return Err("expected a lowercase SHA-256 identity".into()); }
    let digit = |byte| match byte { b'0'..=b'9' => Ok(byte - b'0'), b'a'..=b'f' => Ok(byte - b'a' + 10), _ => Err("expected a lowercase SHA-256 identity".to_owned()) };
    let mut bytes = [0; 32];
    for (output, pair) in bytes.iter_mut().zip(text.as_bytes().chunks_exact(2)) {
        *output = digit(pair[0])? * 16 + digit(pair[1])?;
    }
    Ok(Sha256Identity(bytes))
}
fn bounded(input: impl Read) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    input.take(MAX_INPUT as u64 + 1).read_to_end(&mut bytes).map_err(|error| error.to_string())?;
    if bytes.len() > MAX_INPUT { return Err("observation contract input capacity exhausted".into()); }
    Ok(bytes)
}
fn executable_digest() -> Result<Sha256Identity, String> {
    let mut input = File::open(std::env::current_exe().map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
    let mut buffer = [0; 65536];
    let mut hash = Sha256::new();
    loop {
        let count = input.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 { break; }
        hash.update(&buffer[..count]);
    }
    Ok(Sha256Identity(hash.finalize().into()))
}
fn fields<'a>(args: &'a [OsString], allowed: &[&str]) -> Result<BTreeMap<&'a str, &'a std::ffi::OsStr>, String> {
    let pairs = args.chunks_exact(2);
    if !pairs.remainder().is_empty() { return Err("observation contract arguments require values".into()); }
    let mut fields = BTreeMap::new();
    for pair in pairs {
        let name = pair[0].to_str().ok_or("invalid observation contract flag")?;
        if !allowed.contains(&name) || fields.insert(name, pair[1].as_os_str()).is_some() {
            return Err("unknown or repeated observation contract argument".into());
        }
    }
    if fields.len() != allowed.len() { return Err("missing observation contract argument".into()); }
    Ok(fields)
}
fn validate_envelope(receipt: &serde_json::Value) -> Result<(), String> {
    let fields = receipt.as_object().ok_or("machine fingerprint must be an object")?;
    const FIELDS: [&str; 10] = ["schema", "status", "machine_sha256", "counts", "now", "logical_bytes",
        "included_state", "excluded_host_fields", "components", "exact_state"];
    if fields.len() != FIELDS.len() || FIELDS.iter().any(|name| !fields.contains_key(*name))
        || receipt["schema"] != 1 || receipt["status"] != "complete"
        || receipt["included_state"] != "session-v2/board-v1-complete-machine"
        || receipt["excluded_host_fields"] != serde_json::json!(["board.entries", "board.hooks", "board.observations"])
        || receipt["logical_bytes"].as_u64().is_none_or(|bytes| bytes == 0 || bytes > 2 * 1024 * 1024 * 1024) {
        return Err("machine fingerprint envelope differs".into());
    }
    Ok(())
}
fn dispatch(operation: Operation, args: &[OsString], input: impl Read) -> Result<Response, String> {
    let executable = executable_digest()?;
    let model = env!("XM1_CHECKPOINT_MODEL_ID");
    digest(model)?;
    match operation {
        Operation::ValidateFingerprint => {
            let fields = fields(args, &["--expected-checkpoint-model", "--expected-definition", "--expected-executable"])?;
            let value = |name| fields.get(name).and_then(|value| value.to_str()).ok_or("invalid observation contract identity");

            let expected_model = value("--expected-checkpoint-model")?;
            digest(expected_model)?;
            let expected = ImmutableBindings {
                checkpoint_model_id: CheckpointModelId(expected_model.to_owned()),
                model_definition: digest(value("--expected-definition")?)?,
                executable: digest(value("--expected-executable")?)?,
            };
            if expected.executable != executable { return Err("validator executable identity differs".into()); }
            let receipt: serde_json::Value = serde_json::from_slice(&bounded(input)?).map_err(|error| error.to_string())?;
            validate_envelope(&receipt)?;
            exact_cpu_archive::authenticate_bindings(&receipt, &expected)?;
            let machine_sha256 = digest(receipt["machine_sha256"].as_str().ok_or("missing machine identity")?)?;
            let counts = serde_json::from_value(receipt["counts"].clone()).map_err(|error| error.to_string())?;
            let now = receipt["now"].as_u64().ok_or("missing board time")?;
            Ok(Response::Validation(Validation { schema: 1, status: Status::Complete,
                checkpoint_model_id: WireModel(expected.checkpoint_model_id),
                model_definition_sha256: WireDigest(expected.model_definition),
                executable_sha256: WireDigest(executable), machine_sha256: WireDigest(machine_sha256), counts, now }))
        }
        Operation::MappingCatalogue => {
            fujimod_emulator_models::register();
            let fields = fields(args, &["--run"])?;
            let path = Path::new(fields["--run"]);
            let bytes = bounded(File::open(path).map_err(|error| error.to_string())?)?;
            let document = RunDocument::parse(&bytes)?;
            let mappings = run::mapping_catalogue(&document)?;
            if mappings.is_empty() || mappings.len() > 4096 { return Err("mapping catalogue geometry differs".into()); }
            Ok(Response::Catalogue(Catalogue { schema: 1, status: Status::Complete,
                checkpoint_model_id: WireModel(CheckpointModelId(model.to_owned())),
                executable_sha256: WireDigest(executable),
                run_document_sha256: WireDigest(Sha256Identity(Sha256::digest(&bytes).into())), mappings }))
        }
    }
}

pub fn execute(args: &[OsString], input: impl Read) -> Option<Result<Response, String>> {
    let operation = match args.first().and_then(|arg| arg.to_str()) {
        Some("--validate-machine-fingerprint") => Operation::ValidateFingerprint,
        Some("--af-mapping-catalogue") => Operation::MappingCatalogue,
        _ => return None,
    };
    Some(dispatch(operation, &args[1..], input))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn contract_modes_reject_missing_repeated_unknown_and_incomplete_arguments() {
        for args in [vec!["--af-mapping-catalogue"], vec!["--af-mapping-catalogue", "--run"],
            vec!["--af-mapping-catalogue", "--run", "a", "--run", "b"],
            vec!["--af-mapping-catalogue", "--unknown", "a"], vec!["--validate-machine-fingerprint"]] {
            let args = args.into_iter().map(OsString::from).collect::<Vec<_>>();
            assert!(execute(&args, &b""[..]).unwrap().is_err());
        }
        assert!(execute(&[OsString::from("--fresh-boot")], &b""[..]).is_none());
    }
    #[test]
    fn contract_input_refuses_excess_bytes_and_preserves_exact_limit() {
        let bytes = vec![0x31; MAX_INPUT];
        assert_eq!(bounded(bytes.as_slice()).unwrap(), bytes);
        assert!(bounded(std::io::repeat(0x31).take(MAX_INPUT as u64 + 1)).is_err());
    }
    #[test]
    fn identities_reject_noncanonical_or_wrong_width_values() {
        assert_eq!(digest(&"01".repeat(32)).unwrap(), Sha256Identity([1;32]));
        for text in ["A1".repeat(32), "01".repeat(31), "01".repeat(33), "gg".repeat(32)] { assert!(digest(&text).is_err()); }
    }
}
