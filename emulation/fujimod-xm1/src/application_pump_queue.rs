use super::*;

struct Call<'a> { before: &'a Sample, target: Guest, continuation: Guest, entered: bool }
struct Receive<'a> {
    entry: &'a Sample, destination: Guest, call: Option<Call<'a>>, status: Option<u32>,
    packet: Option<Guest>, payload: Vec<u8>, copied: bool, release: bool, released: bool,
}
fn bytes(views: &[Words], address: u32, length: usize) -> Result<Vec<u8>, Failure> {
    let mut result = Vec::with_capacity(length);
    for offset in 0..length {
        let address = address.checked_add(offset as u32).ok_or_else(|| refusal(Refusal::QueueProvenance, "packet extent overflows"))?;
        let word = projected(views, address & !3)?;
        result.push((word >> (8 * (address & 3))) as u8);
    }
    Ok(result)
}
fn scalar(views: &[Words], address: u32) -> Result<u32, Failure> {
    projected(views, address).map_err(|failure| refusal(Refusal::QueueProvenance, failure.detail))
}
enum ServiceSlot { Receive, Release }
fn service(sample: &Sample, slot: ServiceSlot) -> Result<Guest, Failure> {
    let domain = KernelDomain::from_selector(sample.registers.r[6])?;
    let executing = KernelDomain::from_identity(sample.identities[usize::from(sample.core)])?;
    require(domain.index() == executing.index(), Refusal::QueueProvenance, "queue kernel selector differs from executing domain")?;
    let root = scalar(&sample.state, 0x96bd20 + 4 * domain.index() as u32)?;
    require(root == [0x4004dae0, 0x400ddaa0][domain.index()], Refusal::QueueProvenance, "queue interface root differs")?;
    let offset = match slot { ServiceSlot::Receive => 0xc8, ServiceSlot::Release => 0xf8 };
    let target = Guest(scalar(&sample.state, root + offset)?);
    require(target.0 & 3 == 0 && target.0 == sample.registers.r[3], Refusal::QueueProvenance, "queue indirect call differs from committed retained slot")?;
    let encoded = 1 | scalar(&sample.state, 0x7cad9c)?.checked_shl(16).ok_or_else(|| refusal(Refusal::QueueProvenance, "queue domain shift overflows"))?;
    require(sample.registers.r[0] == encoded, Refusal::QueueProvenance, "routed queue1 identity differs")?;
    Ok(target)
}
pub(super) fn check(report: &PumpReport) -> Result<(), Failure> {
    let mut active: Vec<Receive<'_>> = Vec::new();
    let mut successful = 0usize;
    for sample in &report.samples {
        let pc = physical(sample.pc);
        if pc == 0x725318 {
            require(sample.registers.r[0] == 1 && sample.registers.r[2] == u32::MAX && !active.iter().any(|receive| receive.entry.core == sample.core && receive.entry.registers.sp == sample.registers.sp), Refusal::QueueProvenance, "blocking queue1 entry/context differs")?;
            task_context(sample)?;
            let destination = Guest(sample.registers.r[1]);
            require(sample.packet.iter().any(|view| view.mapping.guest == destination && view.values.len() == 3), Refusal::QueueProvenance, "destination envelope entry absent")?;
            active.push(Receive { entry: sample, destination, call: None, status: None, packet: None, payload: Vec::new(), copied: false, release: false, released: false });
        }
        let Some(index) = active.iter().position(|receive| receive.entry.core == sample.core && (sample.registers.r[11].checked_add(4) == Some(receive.entry.registers.sp.0) || receive.entry.id == sample.id)) else { continue; };
        let receive = &mut active[index];
        if let Some(call) = &mut receive.call {
            if physical(call.target) == pc {
                require(!call.entered && sample.registers.sp == call.before.registers.sp && sample.registers.r == call.before.registers.r && physical(sample.registers.lr) == call.continuation.0, Refusal::QueueProvenance, "queue service entry differs from call boundary")?;
                call.entered = true;
            } else if pc == call.continuation.0 {
                require(call.entered && sample.registers.sp == call.before.registers.sp && sample.registers.r[4..12] == call.before.registers.r[4..12], Refusal::QueueProvenance, "queue service failed to restore caller context")?;
                receive.call = None;
            }
        }
        match pc {
            0x725378 => {
                let target = service(sample, ServiceSlot::Receive)?;
                require(receive.call.is_none() && sample.registers.r[1] == sample.registers.r[11].checked_sub(32).ok_or_else(|| refusal(Refusal::QueueProvenance, "receive slot underflows"))? && sample.registers.r[2] == u32::MAX, Refusal::QueueProvenance, "kernel receive arguments differ")?;
                receive.call = Some(Call { before: sample, target, continuation: Guest(0x72537c), entered: false });
            }
            0x72537c => {
                require(receive.call.is_none(), Refusal::QueueProvenance, "receive continuation lacks actual kernel return")?;
                receive.status = Some(sample.registers.r[0]);
            }
            0x7253c0 => {
                require(receive.status == Some(0), Refusal::QueueProvenance, "success copy follows a nonzero receive")?;
                let slot = sample.registers.r[11].checked_sub(32).ok_or_else(|| refusal(Refusal::QueueProvenance, "packet slot underflows"))?;
                let packet = Guest(scalar(&sample.packet, slot)?);
                let length = scalar(&sample.packet, packet.0 + 8)?;
                require(length <= 76 && sample.registers.r[5] == receive.destination.0, Refusal::QueueProvenance, "received envelope or payload capacity differs")?;
                receive.packet = Some(packet);
                receive.payload = bytes(&sample.packet, packet.0 + 12, length as usize)?;
                if length == 0 { receive.copied = true; }
            }
            0x7253f8 => {
                let packet = receive.packet.ok_or_else(|| refusal(Refusal::QueueProvenance, "copy lacks source packet"))?;
                require(receive.call.is_none() && !receive.copied && sample.registers.r[0] == scalar(&sample.packet, receive.destination.0 + 8)? && sample.registers.r[1] == packet.0 + 12 && sample.registers.r[2] as usize == receive.payload.len(), Refusal::QueueProvenance, "packet copy arguments differ")?;
                receive.call = Some(Call { before: sample, target: Guest(0x722054), continuation: Guest(0x7253fc), entered: false });
            }
            0x7253fc => {
                require(receive.call.is_none() && receive.status == Some(0), Refusal::QueueProvenance, "packet copy return is absent")?;
                receive.copied = true;
            }
            0x725414 => {
                let target = service(sample, ServiceSlot::Release)?;
                let packet = receive.packet.ok_or_else(|| refusal(Refusal::QueueProvenance, "release lacks received packet"))?;
                require(receive.call.is_none() && receive.copied && !receive.release && sample.registers.r[1] == packet.0, Refusal::QueueProvenance, "packet released before copy, twice or under another identity")?;
                let command = scalar(&sample.packet, packet.0 + 4)?;
                require(scalar(&sample.packet, receive.destination.0)? == command && scalar(&sample.packet, receive.destination.0 + 4)? as usize == receive.payload.len(), Refusal::QueueProvenance, "copied envelope header differs from original packet")?;
                let destination = scalar(&sample.packet, receive.destination.0 + 8)?;
                require(bytes(&sample.packet, destination, receive.payload.len())? == receive.payload, Refusal::QueueProvenance, "copied UI payload differs before packet release")?;
                receive.release = true;
                receive.call = Some(Call { before: sample, target, continuation: Guest(0x725418), entered: false });
            }
            0x725418 => { require(receive.release && receive.call.is_none(), Refusal::QueueProvenance, "packet release lacks its actual return")?; receive.released = true; }
            0x72541c => {
                require(receive.call.is_none() && receive.status.is_some(), Refusal::QueueProvenance, "receive exits with an unfinished service")?;
                if receive.status == Some(0) { require(receive.copied && receive.released && sample.registers.r[0] == 0, Refusal::QueueProvenance, "successful receive lacks copy/release/zero return")?; successful += 1; }
                else { require(!receive.release && sample.registers.r[0] == receive.status.unwrap_or(0), Refusal::QueueProvenance, "error receive released an unowned packet or changed status")?; }
                active.remove(index);
            }
            _ => {}
        }
    }
    require(successful >= 2, Refusal::QueueProvenance, "repeated successful queue1 receive/copy/release not witnessed")
}
