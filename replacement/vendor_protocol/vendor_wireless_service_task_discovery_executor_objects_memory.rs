use super::*;
use super::super::NetworkPacketPool;
use super::super::super::interface_modes::{InterfaceMode, InterfaceModes, MemoryObject, RemoteControl, TransferLength, TransferOffset};

impl ConcreteRemoteObjectOperations {
    pub fn execute_memory(&mut self, request: OperationRequest<'_>, state: &mut PtpState, output: &mut PtpOutput, mode: InterfaceMode, interfaces: &mut InterfaceModes, packets: &NetworkPacketPool<'_>) -> Result<OperationReply, PtpError> {
        let command = request.command;
        if request.phase == PtpPhase::Cancel {
            self.cancel_memory(output, mode, interfaces)?;
            return self.respond(command, state.response.0, output);
        }
        if request.phase == PtpPhase::Data {
            let original = match &self.pending {
                PendingOperation::MemoryRead { command, .. } | PendingOperation::Response { command, .. } | PendingOperation::UploadInfo { command } => *command,
                _ => return Err(PtpError::InvalidState),
            };
            if original.operation != command.operation || original.transaction != command.transaction { return Err(PtpError::InvalidState); }
            if matches!(self.pending, PendingOperation::UploadInfo { .. }) {
                let response = upload_info_response(command, request.data, None);
                return self.respond(command, response, output);
            }
            return self.continue_memory_output(output, interfaces, packets);
        }
        if !matches!(self.pending, PendingOperation::Idle) || !output.is_empty() { return Err(PtpError::OutputFull); }
        if state.session_id.is_none() { return self.respond(command, 0x2003, output); }
        let metadata = interfaces.metadata(mode);
        let response = match command.operation.0 {
            0x1006 | 0x1007 => {
                let storage = match StorageSelector::parse(command.parameters[0]) { Ok(storage) => storage, Err(response) => return self.respond(command, response.wire(), output) };
                if storage == StorageSelector::Default { return self.respond(command, 0x2008, output); }
                if storage == StorageSelector::Secondary { return self.respond(command, 0x2013, output); }
                if !FormatFilter::from_wire(command.parameters[1]).supported() { return self.respond(command, 0x200b, output); }
                if !unused_parameters_zero(&command.parameters, 3) { return self.respond(command, 0x2006, output); }
                let parent = ParentFilter::from_wire(command.parameters[2]);
                if matches!(parent, ParentFilter::Object(_)) { return self.respond(command, if metadata.is_some() && command.parameters[2] == 1 { 0x201a } else { 0x2009 }, output); }
                let count = u32::from(metadata.is_some_and(|metadata| FormatFilter::from_wire(command.parameters[1]).matches(u32::from(metadata.format.transfer_code()), command.operation.0 == 0x1006)));
                if command.operation.0 == 0x1006 {
                    let reply = self.queue_response(command, 0x2001, &[count]);
                    output.send_response(reply, command.transaction)?;
                    self.pending = PendingOperation::Idle;
                    return Ok(reply);
                }
                let mut bytes = [0; 8];
                bytes[..4].copy_from_slice(&count.to_le_bytes());
                bytes[4..].copy_from_slice(&1u32.to_le_bytes());
                output.send_data(command.operation, command.transaction, &bytes[..4 + count as usize * 4])?;
                self.queue_response(command, 0x2001, &[]);
                return Ok(OperationReply::new(ResponseCode(0x2001)));
            }
            0x1008 | 0x1009 | 0x100a | 0x101b => {
                if command.parameters[0] != 1 || metadata.is_none() { return self.respond(command, 0x2009, output); }
                let metadata = metadata.ok_or(PtpError::InvalidState)?;
                if command.operation.0 == 0x101b && (command.parameters[1] >= metadata.length || command.parameters[2] == 0) { return self.respond(command, 0x201d, output); }
                if !unused_parameters_zero(&command.parameters, if command.operation.0 == 0x101b { 3 } else { 1 }) { return self.respond(command, 0x2006, output); }
                if command.operation.0 == 0x1008 {
                    let bytes = interfaces.transfer_data(mode, TransferOffset::Primary(0), None, packets).map_err(|_| PtpError::InvalidState)?;
                    let media = match media_format(metadata.format.transfer_code()) {
                        Some(format) => Some(media_parser::parse(bytes, format).map_err(|_| PtpError::InvalidState)?),
                        None => None,
                    };
                    let alternate = interfaces.transfer_data(mode, TransferOffset::Alternate, None, packets).map_err(|_| PtpError::InvalidState)?;
                    let mut dataset = [0; 256];
                    let length = encode_memory_info(metadata, media.as_ref(), alternate, &mut dataset)?;
                    output.send_data(command.operation, command.transaction, &dataset[..length])?;
                    self.queue_response(command, 0x2001, &[]);
                    return Ok(OperationReply::new(ResponseCode(0x2001)));
                }
                let (offset, total, alternate) = if command.operation.0 == 0x100a {
                    let alternate = interfaces.transfer_data(mode, TransferOffset::Alternate, None, packets).map_err(|_| PtpError::InvalidState)?;
                    if alternate.is_empty() { return self.respond(command, 0x2010, output); }
                    (0, alternate.len() as u32, true)
                } else if command.operation.0 == 0x101b {
                    let window = match ReadWindow::partial(ObjectLength(metadata.length), ObjectOffset(command.parameters[1]), ObjectLength(command.parameters[2]), self.previous_end, interfaces.partial_reads_unsupported(mode)) {
                        Ok(window) => window,
                        Err(response) => return self.respond(command, response.wire(), output),
                    };
                    (window.offset.0, window.length.0, false)
                } else { (0, metadata.length, false) };
                output.begin_data(command.operation, command.transaction, total)?;
                self.pending = PendingOperation::MemoryRead { command, mode, generation: interfaces.configuration_generation(mode), offset, remaining: total, total, alternate };
                self.previous_end = ObjectOffset(offset.wrapping_add(total));
                return Ok(OperationReply::new(ResponseCode(0x2001)));
            }
            0x100c => {
                if !unused_parameters_zero(&command.parameters, 2) { 0x2006 }
                else if command.parameters[0] == 0 && command.parameters[1] != 0 { 0x201d }
                else if StorageSelector::parse(command.parameters[0]).is_err() { 0x2008 }
                else if command.parameters[0] == 0x10000002 { 0x2013 }
                else if command.parameters[1] != 0 && command.parameters[1] != u32::MAX { 0x201a }
                else { self.pending = PendingOperation::UploadInfo { command }; return Ok(OperationReply::new(ResponseCode(0x2001))); }
            }
            0x100d => 0x2015,
            _ => return Err(PtpError::InvalidState),
        };
        self.respond(command, response, output)
    }

    pub fn execute_memory_delete(&mut self, request: OperationRequest<'_>, state: &mut PtpState, output: &mut PtpOutput, mode: InterfaceMode, interfaces: &mut InterfaceModes, packets: &mut NetworkPacketPool<'_>, control: &mut RemoteControl) -> Result<OperationReply, PtpError> {
        let command = request.command;
        if command.operation.0 != 0x100b || request.phase != PtpPhase::Command || !request.data.is_empty() { return Err(PtpError::InvalidState); }
        if !matches!(self.pending, PendingOperation::Idle) || !output.is_empty() { return Err(PtpError::OutputFull); }
        if state.session_id.is_none() { return self.respond(command, 0x2003, output); }
        if !unused_parameters_zero(&command.parameters, 1) { return self.respond(command, 0x2006, output); }
        let object = match command.parameters[0] { 0 => MemoryObject::Record0, 1 => MemoryObject::Record1, _ => return self.respond(command, 0x2009, output) };
        let status = interfaces.delete_memory_object(mode, object, packets, control).map_err(|_| PtpError::InvalidState)?;
        self.respond(command, DeleteFailure::from_device(status.raw() as u32).map_or(0x2001, DeleteFailure::response), output)
    }

    pub fn continue_memory_output(&mut self, output: &mut PtpOutput, interfaces: &InterfaceModes, packets: &NetworkPacketPool<'_>) -> Result<OperationReply, PtpError> {
        if !output.is_empty() { return Err(PtpError::OutputFull); }
        let pending = core::mem::replace(&mut self.pending, PendingOperation::Idle);
        match pending {
            PendingOperation::Response { command, reply } => { output.send_response(reply, command.transaction)?; Ok(reply) }
            PendingOperation::MemoryRead { command, mode, generation, offset, remaining, total, alternate } => {
                if generation != interfaces.configuration_generation(mode) { output.discard(); return self.respond(command, 0x200f, output); }
                let source_offset = if alternate { TransferOffset::Alternate } else { TransferOffset::Primary(offset) };
                let bytes = interfaces.transfer_data(mode, source_offset, if alternate { None } else { Some(TransferLength(remaining)) }, packets).map_err(|_| PtpError::InvalidState)?;
                let bytes = if alternate { bytes.get(offset as usize..).ok_or(PtpError::InvalidLength)? } else { bytes };
                let count = output.write_data(&bytes[..bytes.len().min(remaining as usize)])?;
                if count == 0 && remaining != 0 { output.discard(); return self.respond(command, 0x200f, output); }
                let remaining = remaining - count as u32;
                if remaining == 0 {
                    if command.operation.0 == 0x101b { self.queue_response(command, 0x2001, &[total]); }
                    else { self.queue_response(command, 0x2001, &[]); }
                } else { self.pending = PendingOperation::MemoryRead { command, mode, generation, offset: offset + count as u32, remaining, total, alternate }; }
                Ok(OperationReply::new(ResponseCode(0x2001)))
            }
            other => { self.pending = other; Err(PtpError::InvalidState) }
        }
    }

    pub fn cancel_memory(&mut self, output: &mut PtpOutput, mode: InterfaceMode, interfaces: &mut InterfaceModes) -> Result<(), PtpError> {
        if matches!(self.pending, PendingOperation::Read { .. }) { return Err(PtpError::InvalidState); }
        self.pending = PendingOperation::Idle;
        self.previous_end = ObjectOffset(0);
        interfaces.clear_data_request(mode);
        output.discard();
        Ok(())
    }
}

fn encode_memory_info(object: super::super::super::interface_modes::ObjectMetadata, media: Option<&MediaMetadata>, alternate: &[u8], bytes: &mut [u8; 256]) -> Result<usize, PtpError> {
    let dimensions = if alternate.is_empty() { None } else { Some(media_parser::parse_thumbnail(alternate).map_err(|_| PtpError::InvalidState)?) };
    let header = ObjectInfoHeader {
        storage: StorageSelector::Primary, format: object.format.transfer_code(), protection: 0, size: ObjectLength(object.length),
        thumbnail_format: if alternate.is_empty() { 0 } else { 0x3808 }, thumbnail_size: ObjectLength(alternate.len() as u32),
        thumbnail_width: dimensions.as_ref().map_or(0, |dimensions| dimensions.width), thumbnail_height: dimensions.as_ref().map_or(0, |dimensions| dimensions.height),
        image_width: media.map_or(0, |media| media.secondary_width), image_height: media.map_or(0, |media| media.secondary_height), bit_depth: media.map_or(0, |media| media.bit_depth),
        parent: ParentFilter::Any, association_type: 0, association_description: 0, sequence: 0,
    };
    bytes[..52].copy_from_slice(&header.encode());
    let name = object.name.bytes();
    bytes[52] = (name.len() + 1) as u8;
    let mut position = 53;
    for byte in name.iter().copied().chain(core::iter::once(0)) { bytes[position..position + 2].copy_from_slice(&u16::from(byte).to_le_bytes()); position += 2; }
    bytes[position] = 16; position += 1;
    for byte in object.timestamp.bytes()[..8].iter().copied().chain(core::iter::once(b'T')).chain(object.timestamp.bytes()[8..14].iter().copied()).chain(core::iter::once(0)) {
        bytes[position..position + 2].copy_from_slice(&u16::from(byte).to_le_bytes()); position += 2;
    }
    bytes[position..position + 2].fill(0);
    Ok(position + 2)
}
