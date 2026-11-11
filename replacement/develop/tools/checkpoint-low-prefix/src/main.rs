use fujimod_xm1::{adjustment::AdjustmentNand, bridge::{self, BootSource, Inputs}, checkpoint::BootIdentity, sd_host::Backing};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, path::{Path, PathBuf}, process::ExitCode};

const LOW_BYTES: usize = 0x5424;
const ATTACH_RECORD: &str = "ram-push-attach.json";
const VECTOR_LOAD: u32 = 0xe59ff018;

#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(transparent)]
struct Sha256Hex(String);
#[derive(Serialize)]
#[serde(transparent)]
struct HexBytes(String);
#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(transparent)]
struct ModelId(String);
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum InputAsset { Run, Dat, Rom, Nand, Card }
#[derive(Serialize)]
struct InputIdentity { asset: InputAsset, sha256: Sha256Hex }
#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
enum Status { Accepted, Refused }
#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
enum MemoryView { Cpu0Low, Cpu1Low, Cpu0Alias, Cpu1Alias }
#[derive(Serialize)]
#[serde(transparent)]
struct SdkFailure(String);
#[derive(Serialize)]
#[serde(tag = "kind", content = "detail", rename_all = "kebab-case")]
enum Refusal {
    Arguments,
    Io { path: PathBuf, error: String },
    AttachRecord(String),
    CheckpointKey,
    CheckpointModel,
    InputIdentity(InputAsset),
    Sdk(SdkFailure),
    CheckpointInstructionCount,
    GuestExecution,
    AliasMismatch(MemoryView),
    UnexpectedVector,
}

#[derive(Deserialize)]
struct IdentityDocument {
    model: ModelId,
    run_document: Sha256Hex,
    installed_dat: Sha256Hex,
    boot_rom: Sha256Hex,
    adjustment_nand: Option<Sha256Hex>,
    card_sha256: Option<Sha256Hex>,
}
#[derive(Deserialize)]
struct PreAttach { instructions: u64 }
#[derive(Deserialize)]
struct AttachRecord { key: Sha256Hex, inputs: serde_json::Value, pre_attach: PreAttach }

#[derive(Serialize)]
struct Receipt {
    schema_version: u8,
    status: Status,
    refusal: Option<Refusal>,
    checkpoint_key: Option<Sha256Hex>,
    checkpoint_model: Option<ModelId>,
    session_sha256: Option<Sha256Hex>,
    attach_record_sha256: Option<Sha256Hex>,
    reader_sha256: Option<Sha256Hex>,
    argv: Vec<PathBuf>,
    inputs: Vec<InputIdentity>,
    entered_instructions_before: Option<Vec<u64>>,
    entered_instructions_after: Option<Vec<u64>>,
    physical_hex: Option<HexBytes>,
    cpu0_low_hex: Option<HexBytes>,
    cpu1_low_hex: Option<HexBytes>,
    cpu0_alias_hex: Option<HexBytes>,
    cpu1_alias_hex: Option<HexBytes>,
    low_word4: Option<u32>,
}
impl Receipt {
    fn new(argv: Vec<PathBuf>) -> Self {
        Self {
            schema_version: 2, status: Status::Refused, refusal: None,
            checkpoint_key: None, checkpoint_model: None, session_sha256: None, attach_record_sha256: None,
            reader_sha256: None, argv, inputs: Vec::with_capacity(5),
            entered_instructions_before: None, entered_instructions_after: None,
            physical_hex: None, cpu0_low_hex: None, cpu1_low_hex: None,
            cpu0_alias_hex: None, cpu1_alias_hex: None, low_word4: None,
        }
    }
}
fn io_error(path: &Path, error: std::io::Error) -> Refusal {
    Refusal::Io { path: path.to_owned(), error: error.to_string() }
}
fn digest(bytes: &[u8]) -> Sha256Hex { Sha256Hex(format!("{:x}", Sha256::digest(bytes))) }
fn file_digest(path: &Path) -> Result<Sha256Hex, Refusal> {
    let mut file = File::open(path).map_err(|error| io_error(path, error))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let count = file.read(&mut buffer).map_err(|error| io_error(path, error))?;
        if count == 0 { break; }
        hasher.update(&buffer[..count]);
    }
    Ok(Sha256Hex(format!("{:x}", hasher.finalize())))
}
fn bytes_hex(bytes: &[u8]) -> HexBytes {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(DIGITS[(byte >> 4) as usize] as char);
        text.push(DIGITS[(byte & 15) as usize] as char);
    }
    HexBytes(text)
}
fn verified(asset: InputAsset, path: &Path, expected: Option<&Sha256Hex>, identities: &mut Vec<InputIdentity>) -> Result<Vec<u8>, Refusal> {
    let bytes = std::fs::read(path).map_err(|error| io_error(path, error))?;
    let sha256 = digest(&bytes);
    if expected != Some(&sha256) { return Err(Refusal::InputIdentity(asset)); }
    identities.push(InputIdentity { asset, sha256 });
    Ok(bytes)
}
fn sdk(error: String) -> Refusal { Refusal::Sdk(SdkFailure(error)) }
fn compiled_model() -> Option<ModelId> {
    BootIdentity::new(&[], &[], &[], &[], None, &[]).document().get("model")?.as_str().map(|model| ModelId(model.to_owned()))
}
fn extract(receipt: &mut Receipt) -> Result<(), Refusal> {
    let [_, run, dat, rom, nand, card, checkpoint] = receipt.argv.as_slice() else { return Err(Refusal::Arguments) };
    let (run, dat, rom, nand, card, checkpoint) = (run.clone(), dat.clone(), rom.clone(), nand.clone(), card.clone(), checkpoint.clone());
    let executable = std::env::current_exe().map_err(|error| io_error(Path::new("<executable>"), error))?;
    receipt.reader_sha256 = Some(file_digest(&executable)?);
    let record_path = checkpoint.join(ATTACH_RECORD);
    let record_bytes = std::fs::read(&record_path).map_err(|error| io_error(&record_path, error))?;
    receipt.attach_record_sha256 = Some(digest(&record_bytes));
    let record: AttachRecord = serde_json::from_slice(&record_bytes).map_err(|error| Refusal::AttachRecord(error.to_string()))?;
    let identity: IdentityDocument = serde_json::from_value(record.inputs.clone()).map_err(|error| Refusal::AttachRecord(error.to_string()))?;
    let identity_bytes = serde_json::to_vec(&record.inputs).map_err(|error| Refusal::AttachRecord(error.to_string()))?;
    let named = checkpoint.file_name().and_then(|name| name.to_str()) == Some(record.key.0.as_str());
    if !named || digest(&identity_bytes) != record.key { return Err(Refusal::CheckpointKey); }
    receipt.checkpoint_key = Some(record.key);
    if compiled_model().as_ref() != Some(&identity.model) { return Err(Refusal::CheckpointModel); }
    receipt.checkpoint_model = Some(identity.model);
    let document_bytes = verified(InputAsset::Run, &run, Some(&identity.run_document), &mut receipt.inputs)?;
    let update = verified(InputAsset::Dat, &dat, Some(&identity.installed_dat), &mut receipt.inputs)?;
    let boot_rom = verified(InputAsset::Rom, &rom, Some(&identity.boot_rom), &mut receipt.inputs)?;
    let adjustment = verified(InputAsset::Nand, &nand, identity.adjustment_nand.as_ref(), &mut receipt.inputs)?;
    receipt.session_sha256 = Some(file_digest(&checkpoint.join(bridge::SNAPSHOT))?);
    fujimod_emulator_exr2_board::register_board_builder();
    fujimod_emulator_models::register();
    let card = bridge::load_card(&document_bytes, &card, Backing::Sparse).map_err(sdk)?;
    let card_sha256 = Sha256Hex(card.image.sha256.clone());
    if identity.card_sha256.as_ref() != Some(&card_sha256) { return Err(Refusal::InputIdentity(InputAsset::Card)); }
    receipt.inputs.push(InputIdentity { asset: InputAsset::Card, sha256: card_sha256 });
    let inputs = Inputs {
        document_bytes, update, boot_rom,
        nand: BootSource::InitialSeeded { adjustment: AdjustmentNand::parse(&adjustment).map_err(sdk)?, records: None },
        card: Some(card),
    };
    let machine = bridge::restore(&inputs, &checkpoint).map_err(sdk)?;
    let before = machine.session.board.instruction_counts();
    let expected_count = before.len() == 2 && before.iter().copied().sum::<u64>() == record.pre_attach.instructions;
    receipt.entered_instructions_before = Some(before);
    if !expected_count { return Err(Refusal::CheckpointInstructionCount); }
    let mut physical = [0u8; LOW_BYTES];
    machine.session.board.read_physical(0, &mut physical).map_err(sdk)?;
    receipt.physical_hex = Some(bytes_hex(&physical));
    let vector = u32::from_le_bytes(physical[4..8].try_into().expect("fixed vector width"));
    receipt.low_word4 = Some(vector);
    let mut observed = [0u8; LOW_BYTES];
    let mut mismatch = None;
    for (core, address, view) in [
        (0, 0, MemoryView::Cpu0Low), (1, 0, MemoryView::Cpu1Low),
        (0, 0x40000000, MemoryView::Cpu0Alias), (1, 0x40000000, MemoryView::Cpu1Alias),
    ] {
        machine.session.board.read_memory(core, address, &mut observed).map_err(sdk)?;
        let equal = observed == physical;
        match view {
            MemoryView::Cpu0Low => receipt.cpu0_low_hex = Some(bytes_hex(&observed)),
            MemoryView::Cpu1Low => receipt.cpu1_low_hex = Some(bytes_hex(&observed)),
            MemoryView::Cpu0Alias => receipt.cpu0_alias_hex = Some(bytes_hex(&observed)),
            MemoryView::Cpu1Alias => receipt.cpu1_alias_hex = Some(bytes_hex(&observed)),
        }
        if !equal && mismatch.is_none() { mismatch = Some(view); }
    }
    let after = machine.session.board.instruction_counts();
    let unchanged = receipt.entered_instructions_before.as_ref() == Some(&after);
    receipt.entered_instructions_after = Some(after);
    if !unchanged { return Err(Refusal::GuestExecution); }
    if let Some(view) = mismatch { return Err(Refusal::AliasMismatch(view)); }
    if vector != VECTOR_LOAD { return Err(Refusal::UnexpectedVector); }
    receipt.status = Status::Accepted;
    Ok(())
}
fn main() -> ExitCode {
    let mut receipt = Receipt::new(std::env::args_os().map(PathBuf::from).collect());
    let result = extract(&mut receipt);
    let code = if let Err(refusal) = result { receipt.refusal = Some(refusal); ExitCode::FAILURE } else { ExitCode::SUCCESS };
    match serde_json::to_writer(std::io::stdout().lock(), &receipt) {
        Ok(()) => code,
        Err(error) => { eprintln!("checkpoint receipt: {error}"); ExitCode::FAILURE }
    }
}
