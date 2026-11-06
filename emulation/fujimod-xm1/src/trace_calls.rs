use super::register;
use fujimod_emulator::session::Session;
use fujimod_emulator_cpu::arm;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Base { Register(u8), StackWord(u32), RegisterWord { register: u8, offset: u32 } }

#[derive(Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Read {
    base: Base,
    #[serde(default)] offset: u32,
    #[serde(default)] indirect: bool,
    count: usize,
    count_register: Option<u8>,
    count_base: Option<Base>,
    #[serde(default)] on_success: bool,
}

#[derive(Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capture {
    #[serde(default)] pub entry: Vec<Read>,
    #[serde(default)] pub returns: Option<Vec<Read>>,
}

#[derive(Clone)]
struct Resolved { address: u32, indirect: bool, count: usize, on_success: bool }
#[derive(Clone)]
struct Pending { core: usize, site: u32, lr: u32, sp: u32, reads: Vec<Resolved> }
#[derive(Clone, Default)]
pub struct Calls { pub captures: BTreeMap<u32, Capture>, pending: Vec<Pending> }

#[derive(Clone, Debug, Serialize)]
pub struct Bytes { address: u32, bytes: String }
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum Observation { Entry { site: u32, reads: Vec<Bytes> }, Return { site: u32, reads: Vec<Bytes> } }

fn physical(address: u32) -> u64 { u64::from(address & 0x3fff_ffff) }
fn word(session: &Session, address: u32) -> Result<u32, String> {
    let mut bytes = [0; 4];
    session.board.read_physical(physical(address), &mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}
fn base(session: &Session, core: usize, base: Base) -> Result<u32, String> {
    match base {
        Base::Register(index) => register(session, core, arm::Register::R(index)),
        Base::StackWord(offset) => word(session, register(session, core, arm::Register::R(13))?.wrapping_add(offset)),
        Base::RegisterWord { register: index, offset } => word(session, register(session, core, arm::Register::R(index))?.wrapping_add(offset)),
    }
}
fn resolve(session: &Session, core: usize, read: &Read) -> Result<Resolved, String> {
    let address = base(session, core, read.base)?.wrapping_add(read.offset);
    let count = match read.count_register {
        Some(index) => usize::try_from(register(session, core, arm::Register::R(index))?).map_err(|_| "capture length exceeds host")?,
        None => match read.count_base {
            Some(source) => usize::try_from(base(session, core, source)?).map_err(|_| "capture length exceeds host")?,
            None => read.count,
        },
    };
    if count > 262144 { return Err("call capture length exceeds 262144".into()); }
    Ok(Resolved { address, indirect: read.indirect, count, on_success: read.on_success })
}
fn sample(session: &Session, core: usize, read: &Resolved) -> Result<Bytes, String> {
    let address = if read.indirect && read.address != 0 { word(session, read.address)? } else { read.address };
    let successful = !read.on_success || register(session, core, arm::Register::R(0))? == 0;
    let mut bytes = vec![0; if address == 0 || !successful { 0 } else { read.count }];
    if !bytes.is_empty() { session.board.read_physical(physical(address), &mut bytes)?; }
    Ok(Bytes { address, bytes: crate::ff80::encode_hex(&bytes) })
}
impl Calls {
    pub fn return_sites(&self) -> impl Iterator<Item = u32> + '_ {
        self.pending.iter().map(|call| call.lr)
    }
    pub fn pending_return(&self, site: u32) -> bool {
        self.pending.iter().any(|call| call.lr == site)
    }
    pub fn validate(captures: &BTreeMap<u32, Capture>) -> Result<(), String> {
        for capture in captures.values() {
            for read in capture.entry.iter().chain(capture.returns.iter().flatten()) {
                let invalid = |base| matches!(base, Base::Register(r) if r > 14)
                    || matches!(base, Base::RegisterWord { register: r, .. } if r > 14);
                if read.count > 262144 || read.count_register.is_some_and(|r| r > 14)
                    || invalid(read.base) || read.count_base.is_some_and(invalid) {
                    return Err("invalid call capture register or length".into());
                }
            }
        }
        Ok(())
    }
    pub fn observe(&mut self, session: &Session, core: usize, site: u32) -> Result<(Option<Observation>, Option<u32>), String> {
        let sp = register(session, core, arm::Register::R(13))?;
        if let Some(index) = self.pending.iter().rposition(|call| call.core == core && call.lr == site && call.sp == sp) {
            let call = self.pending.remove(index);
            let reads = call.reads.iter().map(|read| sample(session, core, read)).collect::<Result<_, _>>()?;
            return Ok((Some(Observation::Return { site: call.site, reads }), None));
        }
        let Some(capture) = self.captures.get(&site) else { return Ok((None, None)); };
        let reads = capture.entry.iter().map(|read| resolve(session, core, read).and_then(|read| sample(session, core, &read))).collect::<Result<_, _>>()?;
        let lr = if let Some(returns) = &capture.returns {
            let lr = register(session, core, arm::Register::R(14))? & !1;
            let returns = returns.iter().map(|read| resolve(session, core, read)).collect::<Result<_, _>>()?;
            if self.pending.len() >= 1024 { return Err("call capture pending bound reached".into()); }
            self.pending.push(Pending { core, site, lr, sp, reads: returns });
            Some(lr)
        } else { None };
        Ok((Some(Observation::Entry { site, reads }), lr))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capture_rejects_invalid_registers_and_lengths() {
        for raw in [r#"{"entry":[{"base":{"register":15},"count":4}]}"#,
            r#"{"returns":[{"base":{"stack-word":16},"count":262145}]}"#] {
            let capture = serde_json::from_str(raw).unwrap();
            assert!(Calls::validate(&BTreeMap::from([(4, capture)])).is_err());
        }
        let capture = serde_json::from_str(r#"{"entry":[{"base":{"register":13},"count":20}],"returns":[{"base":{"stack-word":16},"count":4}]}"#).unwrap();
        assert!(Calls::validate(&BTreeMap::from([(4, capture)])).is_ok());
    }
}
