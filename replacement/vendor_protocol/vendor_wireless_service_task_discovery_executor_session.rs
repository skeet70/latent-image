use super::executor::{
    validate_unused, OperationReply, OperationRequest, PtpError, PtpOutput, PtpPhase, PtpState,
    ResponseCode, SessionIdentifier,
};
use super::packet::{OperationCode, TransactionId};

#[derive(Clone, Copy)]
struct PendingReply {
    operation: OperationCode,
    transaction: TransactionId,
    response: ResponseCode,
}

pub struct RemoteSessionOperations {
    pending: Option<PendingReply>,
}

impl Default for RemoteSessionOperations {
    fn default() -> Self {
        Self::new()
    }
}

impl RemoteSessionOperations {
    pub const fn new() -> Self {
        Self { pending: None }
    }

    pub fn teardown(&mut self, state: &mut PtpState) {
        self.pending = None;
        state.session_id = None;
    }

    pub fn execute(
        &mut self,
        request: OperationRequest<'_>,
        state: &mut PtpState,
        output: &mut PtpOutput,
    ) -> Result<OperationReply, PtpError> {
        let command = request.command;
        match command.operation.0 {
            0x1002 => {
                let response = if !validate_unused(command, 1) {
                    ResponseCode(0x2006)
                } else if command.parameters[0] == 0 {
                    ResponseCode(0x201d)
                } else if state.session_id.is_some() {
                    ResponseCode(0x201e)
                } else {
                    state.session_id = Some(SessionIdentifier(command.parameters[0]));
                    self.pending = None;
                    ResponseCode(0x2001)
                };
                let reply = reply(response);
                state.response = response;
                output.send_response(reply, command.transaction)?;
                Ok(reply)
            }
            0x1003 => {
                let response = if state.session_id.is_none() {
                    ResponseCode(0x2003)
                } else if !validate_unused(command, 0) {
                    ResponseCode(0x2006)
                } else {
                    state.session_id = None;
                    ResponseCode(0x2001)
                };
                let reply = reply(response);
                state.response = response;
                output.send_response(reply, command.transaction)?;
                Ok(reply)
            }
            0x1001 | 0x1004 | 0x1005 => {
                if !matches!(request.phase, PtpPhase::Command) {
                    let pending = self.pending.as_ref().ok_or(PtpError::InvalidState)?;
                    if pending.operation != command.operation || pending.transaction != command.transaction {
                        return Err(PtpError::InvalidState);
                    }
                    let reply = reply(pending.response);
                    output.send_response(reply, command.transaction)?;
                    state.response = pending.response;
                    self.pending = None;
                    return Ok(reply);
                }
                let response = match command.operation.0 {
                    0x1001 => {
                        if validate_unused(command, 0) { ResponseCode(0x2001) } else { ResponseCode(0x2006) }
                    }
                    0x1004 => {
                        if state.session_id.is_none() {
                            ResponseCode(0x2003)
                        } else if !validate_unused(command, 0) {
                            ResponseCode(0x2006)
                        } else {
                            ResponseCode(0x2001)
                        }
                    }
                    0x1005 => {
                        let storage = RemoteStorageId::parse(command.parameters[0]);
                        if state.session_id.is_none() {
                            ResponseCode(0x2003)
                        } else if storage.is_none() {
                            ResponseCode(0x2008)
                        } else if matches!(storage, Some(RemoteStorageId::Secondary)) {
                            ResponseCode(0x2013)
                        } else if !validate_unused(command, 1) {
                            ResponseCode(0x2006)
                        } else {
                            ResponseCode(0x2001)
                        }
                    }
                    _ => unreachable!(),
                };
                // The installed remote catalog has null device-info and storage-info producers.
                let payload: &[u8] = if command.operation.0 == 0x1004 && response.0 == 0x2001 {
                    &[1, 0, 0, 0, 1, 0, 0, 16]
                } else {
                    &[]
                };
                output.send_data(command.operation, command.transaction, payload)?;
                self.pending = Some(PendingReply {
                    operation: command.operation,
                    transaction: command.transaction,
                    response,
                });
                state.response = response;
                Ok(reply(response))
            }
            _ => Err(PtpError::InvalidState),
        }
    }
}

#[derive(Clone, Copy)]
enum RemoteStorageId {
    Primary,
    Secondary,
    All,
}

impl RemoteStorageId {
    fn parse(raw: u32) -> Option<Self> {
        match raw {
            0x10000001 => Some(Self::Primary),
            0x10000002 => Some(Self::Secondary),
            0xffffffff => Some(Self::All),
            _ => None,
        }
    }
}

fn reply(response: ResponseCode) -> OperationReply {
    OperationReply { response, parameters: [0; 5], parameter_count: 0 }
}

#[cfg(test)]
#[path = "vendor_wireless_service_task_discovery_executor_session_tests.rs"]
mod tests;
