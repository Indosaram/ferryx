use crate::types::*;
use crate::wire::{record, Bytes, Codec, DResult, Put, Reader, Reason};

record!(Hello { major: u16, minor: u16, capabilities: u64, client_kind: ClientKind, client_instance_id: Uuid });
record!(HelloAck {
    major: u16,
    minor: u16,
    capabilities: u64,
    host_instance_id: Uuid,
    supervisor: Supervisor,
    pid: u32,
    process_start_time: u64,
    host_version: String,
});
record!(Spawn {
    operation_id: OperationId,
    cols: u16,
    rows: u16,
    program: String,
    args: Vec<String>,
    env: Vec<(String, String)>,
    cwd: String,
});
record!(SpawnResult {
    operation_id: OperationId,
    session_id: Uuid,
    session_incarnation: Uuid,
    pid: u32,
    created: bool,
    created_table_revision: u64,
});
record!(AcquireLease { client_instance_id: Uuid, lease_request_id: u64, scope: Scope });
record!(PriorEpoch { epoch: u64, final_accepted: u64, committed: u64 });
record!(ResizeApplied { epoch: u64, seq: u64, cols: u16, rows: u16 });
record!(LeaseGranted {
    client_instance_id: Uuid,
    lease_request_id: u64,
    scope: Scope,
    epoch: u64,
    prior: Option<PriorEpoch>,
    resize_applied: Option<ResizeApplied>,
} check |g| {
    let input = g.scope == Scope::Input;
    if (g.prior.is_some() && !input) || (g.resize_applied.is_some() == input) {
        return Err(Reason::OptCondition);
    }
    Ok(())
});
record!(ReleaseLease { client_instance_id: Uuid, lease_request_id: u64, scope: Scope, epoch: u64 });
record!(LeaseReleased { client_instance_id: Uuid, lease_request_id: u64, scope: Scope, epoch: u64, released: bool });
record!(LeaseRevoked { holder_instance_id: Uuid, scope: Scope, revoked_epoch: u64, new_epoch: u64 });
record!(Reclaim { client_instance_id: Uuid, lease_request_id: u64, scope: Scope, epoch: u64 });
record!(InputProgress { accepted: u64, committed: u64 });
record!(InputFinal { final_accepted: u64, committed: u64, pending_dropped: u64 });
record!(ReclaimResult {
    client_instance_id: Uuid,
    lease_request_id: u64,
    scope: Scope,
    requested_epoch: u64,
    ok: bool,
    current_epoch: u64,
    input: Option<InputProgress>,
    input_final: Option<InputFinal>,
    resize: Option<ResizeApplied>,
} check |r| {
    let input = r.scope == Scope::Input;
    if r.input.is_some() != (input && r.ok)
        || r.input_final.is_some() != (input && !r.ok)
        || r.resize.is_some() != !input
    {
        return Err(Reason::OptCondition);
    }
    if r.ok && r.current_epoch != r.requested_epoch {
        return Err(Reason::BadShape);
    }
    Ok(())
});
record!(WriteInput { client_instance_id: Uuid, epoch: u64, start: u64, bytes: Bytes, crc32c: u32 });
record!(InputAck { client_instance_id: Uuid, epoch: u64, accepted: u64, committed: u64 });
record!(GetEpochState { client_instance_id: Uuid, epoch: u64 });
record!(EpochState { epoch: u64, final_accepted: u64, committed: u64, pending_dropped: u64, fenced: bool });
record!(Resize { resize_epoch: u64, resize_seq: u64, cols: u16, rows: u16 });
record!(ResizeAck { epoch: u64, seq: u64, cols: u16, rows: u16 });
record!(Subscribe {
    subscriber_id: Uuid,
    attach_seq: u64,
    client_known_revision: Option<u64>,
    client_known_incarnation: Option<Uuid>,
});
record!(SubscribeAck {
    attach_seq: u64,
    subscription_id: u64,
    session_incarnation: Uuid,
    revision: u64,
    snapshot_follows: bool,
});
record!(Resync { subscription_id: u64 });
record!(Unsubscribe { subscription_id: u64 });
record!(ListSessions { request_token: (Uuid, u64) });
record!(Kill { operation_id: OperationId, signal: Signal });
record!(KillResult { operation_id: OperationId, delivered: bool });
record!(ChildExited {
    session_incarnation: Uuid,
    exit_code: Option<i32>,
    posix_signal: Option<u8>,
    table_revision: u64,
    pending_dropped_by_epoch: Vec<(u64, u64)>,
} check |c| if c.pending_dropped_by_epoch.len() > 128 { Err(Reason::BadShape) } else { Ok(()) });
record!(ErrorFrame { code: u16, message: String, detail: Bytes });
record!(ReserveOperation { kind: OperationKind });
record!(OperationReserved { operation_id: OperationId });
record!(LeaseVacated { holder_instance_id: Uuid, scope: Scope, epoch: u64 });

macro_rules! messages {
    ($($id:literal => $v:ident($t:ty)),* $(,)?) => {
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub enum Message { $($v($t),)* GetSessionState }

        impl Message {
            pub fn command(&self) -> u16 {
                match self { $(Message::$v(_) => $id,)* Message::GetSessionState => 0x18 }
            }

            pub fn encode_payload(&self, w: &mut Vec<u8>) {
                match self { $(Message::$v(m) => m.enc(w),)* Message::GetSessionState => {} }
            }

            pub fn decode_payload(command: u16, payload: &[u8]) -> Result<Message, PayloadError> {
                let mut r = Reader::new(payload);
                let msg = match command {
                    $($id => Message::$v(<$t>::dec(&mut r).map_err(PayloadError::Violation)?),)*
                    0x18 => Message::GetSessionState,
                    other => return Err(PayloadError::UnknownCommand(other)),
                };
                Ok(msg)
            }
        }
    };
}

messages! {
    0x01 => Hello(Hello),
    0x02 => HelloAck(HelloAck),
    0x03 => Spawn(Spawn),
    0x04 => SpawnResult(SpawnResult),
    0x05 => AcquireLease(AcquireLease),
    0x06 => LeaseGranted(LeaseGranted),
    0x07 => ReleaseLease(ReleaseLease),
    0x08 => LeaseReleased(LeaseReleased),
    0x09 => LeaseRevoked(LeaseRevoked),
    0x0A => Reclaim(Reclaim),
    0x0B => ReclaimResult(ReclaimResult),
    0x0C => WriteInput(WriteInput),
    0x0D => InputAck(InputAck),
    0x0E => GetEpochState(GetEpochState),
    0x0F => EpochState(EpochState),
    0x10 => Resize(Resize),
    0x11 => ResizeAck(ResizeAck),
    0x12 => Subscribe(Subscribe),
    0x13 => SubscribeAck(SubscribeAck),
    0x14 => SnapshotFrame(SnapshotFrame),
    0x15 => Delta(Delta),
    0x16 => Resync(Resync),
    0x17 => Unsubscribe(Unsubscribe),
    0x19 => SessionState(SessionState),
    0x1A => ListSessions(ListSessions),
    0x1B => SessionList(SessionList),
    0x1C => Kill(Kill),
    0x1D => KillResult(KillResult),
    0x1E => ChildExited(ChildExited),
    0x1F => Error(ErrorFrame),
    0x20 => ReserveOperation(ReserveOperation),
    0x21 => OperationReserved(OperationReserved),
    0x22 => LeaseVacated(LeaseVacated),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PayloadError {
    UnknownCommand(u16),
    Violation(Reason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorDetail {
    StaleLease { scope: Scope, current_epoch: u64, epoch_final_accepted: u64 },
    InputGap { accepted: u64 },
    InputDiverged { epoch: u64, accepted: u64, committed: u64 },
    InputUnverifiable { epoch: u64, accepted: u64, committed: u64, retained_start: u64 },
    InputBackpressure { accepted: u64 },
    CorruptFrame,
    StaleResize { applied_epoch: u64, applied_seq: u64, cols: u16, rows: u16 },
    SessionNotFound,
    OperationConflict(OperationId),
    OperationExpired(OperationId),
    OperationUnknown(OperationId),
    SlowConsumer { subscription_id: u64 },
    VersionUnsupported { host_major: u16, host_minor: u16 },
    UnsupportedCapability { capability_bit: u8 },
    UnknownCommand { command: u16 },
    LimitExceeded { limit_kind: u8, limit: u64 },
    ProtocolViolation(Reason),
    StaleViewport,
    StaleSubscribe { current_attach_seq: u64 },
    EpochUnknown { epoch: u64 },
    InstanceRetired,
    OwnerUnreachable,
    SpawnFailed,
}

impl ErrorDetail {
    pub fn code(&self) -> u16 {
        use ErrorDetail::*;
        match self {
            StaleLease { .. } => 1,
            InputGap { .. } => 2,
            InputDiverged { .. } => 3,
            InputUnverifiable { .. } => 4,
            InputBackpressure { .. } => 5,
            CorruptFrame => 6,
            StaleResize { .. } => 7,
            SessionNotFound => 8,
            OperationConflict(_) => 9,
            OperationExpired(_) => 10,
            OperationUnknown(_) => 11,
            SlowConsumer { .. } => 12,
            VersionUnsupported { .. } => 13,
            UnsupportedCapability { .. } => 14,
            UnknownCommand { .. } => 15,
            LimitExceeded { .. } => 16,
            ProtocolViolation(_) => 17,
            StaleViewport => 18,
            StaleSubscribe { .. } => 19,
            EpochUnknown { .. } => 20,
            InstanceRetired => 21,
            OwnerUnreachable => 22,
            SpawnFailed => 23,
        }
    }

    pub fn encode_detail(&self) -> Vec<u8> {
        use ErrorDetail::*;
        let mut w = Vec::new();
        match self {
            StaleLease { scope, current_epoch, epoch_final_accepted } => {
                scope.enc(&mut w);
                w.put_u64(*current_epoch);
                w.put_u64(*epoch_final_accepted);
            }
            InputGap { accepted } | InputBackpressure { accepted } => w.put_u64(*accepted),
            InputDiverged { epoch, accepted, committed } => {
                w.put_u64(*epoch);
                w.put_u64(*accepted);
                w.put_u64(*committed);
            }
            InputUnverifiable { epoch, accepted, committed, retained_start } => {
                w.put_u64(*epoch);
                w.put_u64(*accepted);
                w.put_u64(*committed);
                w.put_u64(*retained_start);
            }
            StaleResize { applied_epoch, applied_seq, cols, rows } => {
                w.put_u64(*applied_epoch);
                w.put_u64(*applied_seq);
                w.put_u16(*cols);
                w.put_u16(*rows);
            }
            OperationConflict(id) | OperationExpired(id) | OperationUnknown(id) => id.enc(&mut w),
            SlowConsumer { subscription_id } => w.put_u64(*subscription_id),
            VersionUnsupported { host_major, host_minor } => {
                w.put_u16(*host_major);
                w.put_u16(*host_minor);
            }
            UnsupportedCapability { capability_bit } => w.put_u8(*capability_bit),
            UnknownCommand { command } => w.put_u16(*command),
            LimitExceeded { limit_kind, limit } => {
                w.put_u8(*limit_kind);
                w.put_u64(*limit);
            }
            ProtocolViolation(reason) => reason.enc(&mut w),
            StaleSubscribe { current_attach_seq } => w.put_u64(*current_attach_seq),
            EpochUnknown { epoch } => w.put_u64(*epoch),
            CorruptFrame | SessionNotFound | StaleViewport | InstanceRetired | OwnerUnreachable | SpawnFailed => {}
        }
        w
    }

    pub fn decode_detail(code: u16, detail: &[u8]) -> DResult<ErrorDetail> {
        use ErrorDetail::*;
        let mut r = Reader::new(detail);
        let r = &mut r;
        Ok(match code {
            1 => StaleLease { scope: Codec::dec(r)?, current_epoch: r.u64()?, epoch_final_accepted: r.u64()? },
            2 => InputGap { accepted: r.u64()? },
            3 => InputDiverged { epoch: r.u64()?, accepted: r.u64()?, committed: r.u64()? },
            4 => InputUnverifiable { epoch: r.u64()?, accepted: r.u64()?, committed: r.u64()?, retained_start: r.u64()? },
            5 => InputBackpressure { accepted: r.u64()? },
            6 => CorruptFrame,
            7 => StaleResize { applied_epoch: r.u64()?, applied_seq: r.u64()?, cols: r.u16()?, rows: r.u16()? },
            8 => SessionNotFound,
            9 => OperationConflict(Codec::dec(r)?),
            10 => OperationExpired(Codec::dec(r)?),
            11 => OperationUnknown(Codec::dec(r)?),
            12 => SlowConsumer { subscription_id: r.u64()? },
            13 => VersionUnsupported { host_major: r.u16()?, host_minor: r.u16()? },
            14 => UnsupportedCapability { capability_bit: r.u8()? },
            15 => UnknownCommand { command: r.u16()? },
            16 => LimitExceeded { limit_kind: r.u8()?, limit: r.u64()? },
            17 => ProtocolViolation(Codec::dec(r)?),
            18 => StaleViewport,
            19 => StaleSubscribe { current_attach_seq: r.u64()? },
            20 => EpochUnknown { epoch: r.u64()? },
            21 => InstanceRetired,
            22 => OwnerUnreachable,
            23 => SpawnFailed,
            _ => return Err(Reason::BadEnum),
        })
    }

    pub fn to_frame(&self, message: impl Into<String>) -> ErrorFrame {
        ErrorFrame { code: self.code(), message: message.into(), detail: Bytes(self.encode_detail()) }
    }
}
