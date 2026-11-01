use rusqlite::{params, Connection, Transaction};

use crate::account::store::{
    AccountStore, BillingStateRecord, DeviceAuthRecord, EnrollmentCodeRecord, GrantRecord,
    LoginCodeRecord, MachineRecord, MachineRequestRecord, OrgInviteRecord, OrgMemberRecord, OrgRecord, PaymentEventRecord,
    SessionRecord, SubscriptionRecord, SupervisorLeaseRecord, UserRecord,
};

/// Column list shared by every `supervisor_leases` read; matches [`supervisor_lease_from_row`].
pub const SUPERVISOR_LEASE_SELECT: &str = "SELECT user_id, holder_machine_id, fence, expires_at, \
     checkpoint_id, checkpoint_at, released, reserved_for, reserved_fence, updated_at \
     FROM supervisor_leases";

pub fn supervisor_lease_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SupervisorLeaseRecord> {
    let checkpoint_at: Option<i64> = row.get(5)?;
    let reserved_fence: Option<i64> = row.get(8)?;
    Ok(SupervisorLeaseRecord {
        user_id: row.get(0)?,
        holder_machine_id: row.get(1)?,
        fence: row.get::<_, i64>(2)? as u64,
        expires_at: row.get::<_, i64>(3)? as u64,
        checkpoint_id: row.get(4)?,
        checkpoint_at: checkpoint_at.map(|v| v as u64),
        released: row.get::<_, i64>(6)? != 0,
        reserved_for: row.get(7)?,
        reserved_fence: reserved_fence.map(|v| v as u64),
        updated_at: row.get::<_, i64>(9)? as u64,
    })
}

/// True when `table` exists. A read-only replica never runs the v3 migration, so lease tables
/// may be absent there and readers must not fail on them.
pub fn table_exists(conn: &Connection, table: &str) -> Result<bool, String> {
    let count: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            params![table],
            |row| row.get(0),
        )
        .map_err(|err| format!("Failed to check for table {table}: {err}"))?;
    Ok(count > 0)
}

pub fn load_store_from_connection(conn: &Connection) -> Result<AccountStore, String> {
    let mut store = AccountStore::default();

    {
        let mut stmt = conn
            .prepare("SELECT user_id, email, created_at FROM users")
            .map_err(|err| format!("Failed to prepare users query: {err}"))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(UserRecord {
                    user_id: row.get(0)?,
                    email: row.get(1)?,
                    created_at: row.get::<_, i64>(2)? as u64,
                })
            })
            .map_err(|err| format!("Failed to query users: {err}"))?;
        for user in rows {
            let record = user.map_err(|err| format!("Failed to read user row: {err}"))?;
            store.users.insert(record.user_id.clone(), record);
        }
    }

    {
        let mut stmt = conn
            .prepare("SELECT token_hash, user_id, expires_at FROM sessions")
            .map_err(|err| format!("Failed to prepare sessions query: {err}"))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    SessionRecord {
                        user_id: row.get(1)?,
                        expires_at: row.get::<_, i64>(2)? as u64,
                    },
                ))
            })
            .map_err(|err| format!("Failed to query sessions: {err}"))?;
        for row in rows {
            let (hash, record) = row.map_err(|err| format!("Failed to read session row: {err}"))?;
            store.sessions.insert(hash, record);
        }
    }

    {
        let mut stmt = conn
            .prepare("SELECT code_hash, email, expires_at, consumed_at, login_handle_hash FROM login_codes")
            .map_err(|err| format!("Failed to prepare login_codes query: {err}"))?;
        let rows = stmt
            .query_map([], |row| {
                let consumed: Option<i64> = row.get(3)?;
                Ok((
                    row.get::<_, String>(0)?,
                    LoginCodeRecord {
                        email: row.get(1)?,
                        expires_at: row.get::<_, i64>(2)? as u64,
                        consumed_at: consumed.map(|v| v as u64),
                        login_handle_hash: row.get(4)?,
                    },
                ))
            })
            .map_err(|err| format!("Failed to query login_codes: {err}"))?;
        for row in rows {
            let (hash, record) =
                row.map_err(|err| format!("Failed to read login_code row: {err}"))?;
            store.login_codes.insert(hash, record);
        }
    }

    {
        let mut stmt = conn
            .prepare(
                "SELECT code_hash, user_id, account_origin, expires_at, kind, key_id, label, \
                 created_at, consumed_at, consumed_machine_id, bound_machine_id, bound_public_key \
                 FROM enrollment_codes",
            )
            .map_err(|err| format!("Failed to prepare enrollment_codes query: {err}"))?;
        let rows = stmt
            .query_map([], |row| {
                let consumed: Option<i64> = row.get(8)?;
                Ok((
                    row.get::<_, String>(0)?,
                    EnrollmentCodeRecord {
                        user_id: row.get(1)?,
                        account_origin: row.get(2)?,
                        expires_at: row.get::<_, i64>(3)? as u64,
                        kind: row.get(4)?,
                        key_id: row.get(5)?,
                        label: row.get(6)?,
                        created_at: row.get::<_, i64>(7)? as u64,
                        consumed_at: consumed.map(|v| v as u64),
                        consumed_machine_id: row.get(9)?,
                        bound_machine_id: row.get(10)?,
                        bound_public_key: row.get(11)?,
                    },
                ))
            })
            .map_err(|err| format!("Failed to query enrollment_codes: {err}"))?;
        for row in rows {
            let (hash, record) =
                row.map_err(|err| format!("Failed to read enrollment_code row: {err}"))?;
            store.enrollment_codes.insert(hash, record);
        }
    }

    {
        let mut stmt = conn
            .prepare(
                "SELECT machine_record_id, user_id, machine_id, display_name, public_key, \
                 attach_public_key, relay_origin, platform, enrollment_epoch, enrolled_at, last_seen_at \
                 FROM machines",
            )
            .map_err(|err| format!("Failed to prepare machines query: {err}"))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(MachineRecord {
                    machine_record_id: row.get(0)?,
                    owner_user_id: row.get(1)?,
                    machine_id: row.get(2)?,
                    display_name: row.get(3)?,
                    public_key: row.get(4)?,
                    attach_public_key: row.get(5)?,
                    relay_origin: row.get(6)?,
                    platform: row.get(7)?,
                    enrollment_epoch: row.get::<_, i64>(8)? as u64,
                    enrolled_at: row.get::<_, i64>(9)? as u64,
                    last_seen_at: row.get::<_, i64>(10)? as u64,
                })
            })
            .map_err(|err| format!("Failed to query machines: {err}"))?;
        for machine in rows {
            let record = machine.map_err(|err| format!("Failed to read machine row: {err}"))?;
            store
                .machines
                .insert(record.machine_record_id.clone(), record);
        }
    }

    {
        let mut stmt = conn
            .prepare(
                "SELECT grant_id, machine_record_id, owner_user_id, pairing_token_hash, \
                 grant_scope, device_attach_public_key, installation_id, issued_at, expires_at \
                 FROM grants",
            )
            .map_err(|err| format!("Failed to prepare grants query: {err}"))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(GrantRecord {
                    grant_id: row.get(0)?,
                    machine_record_id: row.get(1)?,
                    owner_user_id: row.get(2)?,
                    pairing_token_hash: row.get(3)?,
                    grant_scope: row.get(4)?,
                    device_attach_public_key: row.get(5)?,
                    installation_id: row.get(6)?,
                    issued_at: row.get::<_, i64>(7)? as u64,
                    expires_at: row.get::<_, i64>(8)? as u64,
                })
            })
            .map_err(|err| format!("Failed to query grants: {err}"))?;
        for grant in rows {
            let record = grant.map_err(|err| format!("Failed to read grant row: {err}"))?;
            store.grants.insert(record.grant_id.clone(), record);
        }
    }

    {
        let mut stmt = conn
            .prepare(
                "SELECT device_code_hash, user_code, email, email_token_hash, enrollment_code, expires_at \
                 FROM device_auths",
            )
            .map_err(|err| format!("Failed to prepare device_auths query: {err}"))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(DeviceAuthRecord {
                    device_code_hash: row.get(0)?,
                    user_code: row.get(1)?,
                    email: row.get(2)?,
                    email_token_hash: row.get(3)?,
                    enrollment_code: row.get(4)?,
                    expires_at: row.get::<_, i64>(5)? as u64,
                })
            })
            .map_err(|err| format!("Failed to query device_auths: {err}"))?;
        for auth in rows {
            let record = auth.map_err(|err| format!("Failed to read device_auth row: {err}"))?;
            store
                .device_auths
                .insert(record.device_code_hash.clone(), record);
        }
    }

    {
        let mut stmt = conn
            .prepare(
                "SELECT subscription_id, owner_user_id, org_id, plan_key, seats, host_packs, \
                 kind, status, ends_at, ls_customer_id, ls_updated_at, manage_url \
                 FROM subscriptions",
            )
            .map_err(|err| format!("Failed to prepare subscriptions query: {err}"))?;
        let rows = stmt
            .query_map([], |row| {
                let ends_at: Option<i64> = row.get(8)?;
                Ok(SubscriptionRecord {
                    subscription_id: row.get(0)?,
                    owner_user_id: row.get(1)?,
                    org_id: row.get(2)?,
                    plan_key: row.get(3)?,
                    seats: row.get::<_, i64>(4)? as u32,
                    host_packs: row.get::<_, i64>(5)? as u32,
                    kind: row.get(6)?,
                    status: row.get(7)?,
                    ends_at: ends_at.map(|v| v as u64),
                    ls_customer_id: row.get(9)?,
                    ls_updated_at: row.get::<_, i64>(10)? as u64,
                    manage_url: row.get(11)?,
                })
            })
            .map_err(|err| format!("Failed to query subscriptions: {err}"))?;
        for sub in rows {
            let record = sub.map_err(|err| format!("Failed to read subscription row: {err}"))?;
            store
                .subscriptions
                .insert(record.subscription_id.clone(), record);
        }
    }

    {
        let mut stmt = conn
            .prepare(
                "SELECT owner_key, grace_started_at, stopped_at, last_notice FROM billing_states",
            )
            .map_err(|err| format!("Failed to prepare billing_states query: {err}"))?;
        let rows = stmt
            .query_map([], |row| {
                let grace: Option<i64> = row.get(1)?;
                let stopped: Option<i64> = row.get(2)?;
                Ok(BillingStateRecord {
                    owner_key: row.get(0)?,
                    grace_started_at: grace.map(|v| v as u64),
                    stopped_at: stopped.map(|v| v as u64),
                    last_notice: row.get(3)?,
                })
            })
            .map_err(|err| format!("Failed to query billing_states: {err}"))?;
        for bs in rows {
            let record = bs.map_err(|err| format!("Failed to read billing_state row: {err}"))?;
            store
                .billing_states
                .insert(record.owner_key.clone(), record);
        }
    }

    {
        let mut stmt = conn
            .prepare("SELECT event_key, event_name, received_at, applied_at, outcome FROM payment_events")
            .map_err(|err| format!("Failed to prepare payment_events query: {err}"))?;
        let rows = stmt
            .query_map([], |row| {
                let applied: Option<i64> = row.get(3)?;
                Ok(PaymentEventRecord {
                    event_key: row.get(0)?,
                    event_name: row.get(1)?,
                    received_at: row.get::<_, i64>(2)? as u64,
                    applied_at: applied.map(|v| v as u64),
                    outcome: row.get(4)?,
                })
            })
            .map_err(|err| format!("Failed to query payment_events: {err}"))?;
        for pe in rows {
            let record = pe.map_err(|err| format!("Failed to read payment_event row: {err}"))?;
            store
                .payment_events
                .insert(record.event_key.clone(), record);
        }
    }

    {
        let mut stmt = conn
            .prepare("SELECT org_id, owner_user_id, name, created_at FROM orgs")
            .map_err(|err| format!("Failed to prepare orgs query: {err}"))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(OrgRecord {
                    org_id: row.get(0)?,
                    owner_user_id: row.get(1)?,
                    name: row.get(2)?,
                    created_at: row.get::<_, i64>(3)? as u64,
                })
            })
            .map_err(|err| format!("Failed to query orgs: {err}"))?;
        for org in rows {
            let record = org.map_err(|err| format!("Failed to read org row: {err}"))?;
            store.orgs.insert(record.org_id.clone(), record);
        }
    }

    {
        let mut stmt = conn
            .prepare("SELECT org_id, user_id, role, joined_at FROM org_members")
            .map_err(|err| format!("Failed to prepare org_members query: {err}"))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(OrgMemberRecord {
                    org_id: row.get(0)?,
                    user_id: row.get(1)?,
                    role: row.get(2)?,
                    joined_at: row.get::<_, i64>(3)? as u64,
                })
            })
            .map_err(|err| format!("Failed to query org_members: {err}"))?;
        for member in rows {
            let record = member.map_err(|err| format!("Failed to read org_member row: {err}"))?;
            let key = format!("{}:{}", record.org_id, record.user_id);
            store.org_members.insert(key, record);
        }
    }

    {
        let mut stmt = conn
            .prepare("SELECT token_hash, org_id, email, expires_at FROM org_invites")
            .map_err(|err| format!("Failed to prepare org_invites query: {err}"))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(OrgInviteRecord {
                    token_hash: row.get(0)?,
                    org_id: row.get(1)?,
                    email: row.get(2)?,
                    expires_at: row.get::<_, i64>(3)? as u64,
                })
            })
            .map_err(|err| format!("Failed to query org_invites: {err}"))?;
        for invite in rows {
            let record = invite.map_err(|err| format!("Failed to read org_invite row: {err}"))?;
            store
                .org_invites
                .insert(record.token_hash.clone(), record);
        }
    }

    {
        let mut stmt = conn
            .prepare(
                "SELECT request_id, device_code_hash, user_code, machine_id, public_key, \
                 display_name, platform, app_version, email_hint, requester_ip, created_at, \
                 expires_at, status, decided_by, decided_at, enrollment_code, last_polled_at \
                 FROM machine_requests",
            )
            .map_err(|err| format!("Failed to prepare machine_requests query: {err}"))?;
        let rows = stmt
            .query_map([], |row| {
                let decided_at: Option<i64> = row.get(14)?;
                let last_polled_at: Option<i64> = row.get(16)?;
                Ok(MachineRequestRecord {
                    request_id: row.get(0)?,
                    device_code_hash: row.get(1)?,
                    user_code: row.get(2)?,
                    machine_id: row.get(3)?,
                    public_key: row.get(4)?,
                    display_name: row.get(5)?,
                    platform: row.get(6)?,
                    app_version: row.get(7)?,
                    email_hint: row.get(8)?,
                    requester_ip: row.get(9)?,
                    created_at: row.get::<_, i64>(10)? as u64,
                    expires_at: row.get::<_, i64>(11)? as u64,
                    status: row.get(12)?,
                    decided_by: row.get(13)?,
                    decided_at: decided_at.map(|v| v as u64),
                    enrollment_code: row.get(15)?,
                    last_polled_at: last_polled_at.map(|v| v as u64),
                })
            })
            .map_err(|err| format!("Failed to query machine_requests: {err}"))?;
        for request in rows {
            let record =
                request.map_err(|err| format!("Failed to read machine_request row: {err}"))?;
            store
                .machine_requests
                .insert(record.request_id.clone(), record);
        }
    }

    if table_exists(conn, "supervisor_leases")? {
        let mut stmt = conn
            .prepare(SUPERVISOR_LEASE_SELECT)
            .map_err(|err| format!("Failed to prepare supervisor_leases query: {err}"))?;
        let rows = stmt
            .query_map([], supervisor_lease_from_row)
            .map_err(|err| format!("Failed to query supervisor_leases: {err}"))?;
        for lease in rows {
            let record =
                lease.map_err(|err| format!("Failed to read supervisor_lease row: {err}"))?;
            store
                .supervisor_leases
                .insert(record.user_id.clone(), record);
        }
    }

    Ok(store)
}

pub fn save_store_to_transaction(tx: &Transaction, store: &AccountStore) -> Result<(), String> {
    {
        let mut del_stmt = tx
            .prepare("DELETE FROM users WHERE user_id = ?1")
            .map_err(|e| format!("Failed to prepare delete user: {e}"))?;
        let mut existing_keys = Vec::new();
        {
            let mut select_stmt = tx
                .prepare("SELECT user_id FROM users")
                .map_err(|e| format!("Failed to prepare select user ids: {e}"))?;
            let rows = select_stmt
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(|e| format!("Failed to query user ids: {e}"))?;
            for k in rows {
                existing_keys.push(k.map_err(|e| format!("Failed to read user id: {e}"))?);
            }
        }
        for k in existing_keys {
            if !store.users.contains_key(&k) {
                del_stmt
                    .execute(params![k])
                    .map_err(|e| format!("Failed to delete user {k}: {e}"))?;
            }
        }

        let mut upsert_stmt = tx
            .prepare(
                "INSERT INTO users (user_id, email, created_at) VALUES (?1, ?2, ?3) \
                 ON CONFLICT(user_id) DO UPDATE SET email=excluded.email, created_at=excluded.created_at",
            )
            .map_err(|e| format!("Failed to prepare user upsert: {e}"))?;
        for user in store.users.values() {
            upsert_stmt
                .execute(params![user.user_id, user.email, user.created_at as i64])
                .map_err(|e| format!("Failed to upsert user {}: {e}", user.user_id))?;
        }
    }

    {
        let mut existing_keys = Vec::new();
        {
            let mut select_stmt = tx
                .prepare("SELECT org_id FROM orgs")
                .map_err(|e| format!("Failed to prepare select org ids: {e}"))?;
            let rows = select_stmt
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(|e| format!("Failed to query org ids: {e}"))?;
            for k in rows {
                existing_keys.push(k.map_err(|e| format!("Failed to read org id: {e}"))?);
            }
        }
        let mut del_stmt = tx
            .prepare("DELETE FROM orgs WHERE org_id = ?1")
            .map_err(|e| format!("Failed to prepare delete org: {e}"))?;
        for k in existing_keys {
            if !store.orgs.contains_key(&k) {
                del_stmt
                    .execute(params![k])
                    .map_err(|e| format!("Failed to delete org {k}: {e}"))?;
            }
        }

        let mut upsert_stmt = tx
            .prepare(
                "INSERT INTO orgs (org_id, owner_user_id, name, created_at) VALUES (?1, ?2, ?3, ?4) \
                 ON CONFLICT(org_id) DO UPDATE SET owner_user_id=excluded.owner_user_id, name=excluded.name, created_at=excluded.created_at",
            )
            .map_err(|e| format!("Failed to prepare org upsert: {e}"))?;
        for org in store.orgs.values() {
            upsert_stmt
                .execute(params![
                    org.org_id,
                    org.owner_user_id,
                    org.name,
                    org.created_at as i64
                ])
                .map_err(|e| format!("Failed to upsert org {}: {e}", org.org_id))?;
        }
    }

    {
        let mut existing_keys = Vec::new();
        {
            let mut select_stmt = tx
                .prepare("SELECT machine_record_id FROM machines")
                .map_err(|e| format!("Failed to prepare select machine ids: {e}"))?;
            let rows = select_stmt
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(|e| format!("Failed to query machine ids: {e}"))?;
            for k in rows {
                existing_keys.push(k.map_err(|e| format!("Failed to read machine id: {e}"))?);
            }
        }
        let mut del_stmt = tx
            .prepare("DELETE FROM machines WHERE machine_record_id = ?1")
            .map_err(|e| format!("Failed to prepare delete machine: {e}"))?;
        for k in existing_keys {
            if !store.machines.contains_key(&k) {
                del_stmt
                    .execute(params![k])
                    .map_err(|e| format!("Failed to delete machine {k}: {e}"))?;
            }
        }

        let mut upsert_stmt = tx
            .prepare(
                "INSERT INTO machines (machine_record_id, user_id, machine_id, display_name, \
                 public_key, attach_public_key, relay_origin, platform, enrollment_epoch, \
                 enrolled_at, last_seen_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11) \
                 ON CONFLICT(machine_record_id) DO UPDATE SET \
                 user_id=excluded.user_id, machine_id=excluded.machine_id, display_name=excluded.display_name, \
                 public_key=excluded.public_key, attach_public_key=excluded.attach_public_key, \
                 relay_origin=excluded.relay_origin, platform=excluded.platform, \
                 enrollment_epoch=excluded.enrollment_epoch, enrolled_at=excluded.enrolled_at, \
                 last_seen_at=excluded.last_seen_at",
            )
            .map_err(|e| format!("Failed to prepare machine upsert: {e}"))?;
        for machine in store.machines.values() {
            upsert_stmt
                .execute(params![
                    machine.machine_record_id,
                    machine.owner_user_id,
                    machine.machine_id,
                    machine.display_name,
                    machine.public_key,
                    machine.attach_public_key,
                    machine.relay_origin,
                    machine.platform,
                    machine.enrollment_epoch as i64,
                    machine.enrolled_at as i64,
                    machine.last_seen_at as i64
                ])
                .map_err(|e| {
                    format!("Failed to upsert machine {}: {e}", machine.machine_record_id)
                })?;
        }
    }

    {
        let mut existing_keys = Vec::new();
        {
            let mut select_stmt = tx
                .prepare("SELECT token_hash FROM sessions")
                .map_err(|e| format!("Failed to prepare select session tokens: {e}"))?;
            let rows = select_stmt
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(|e| format!("Failed to query session tokens: {e}"))?;
            for k in rows {
                existing_keys.push(k.map_err(|e| format!("Failed to read session token: {e}"))?);
            }
        }
        let mut del_stmt = tx
            .prepare("DELETE FROM sessions WHERE token_hash = ?1")
            .map_err(|e| format!("Failed to prepare delete session: {e}"))?;
        for k in existing_keys {
            if !store.sessions.contains_key(&k) {
                del_stmt
                    .execute(params![k])
                    .map_err(|e| format!("Failed to delete session {k}: {e}"))?;
            }
        }

        let mut upsert_stmt = tx
            .prepare(
                "INSERT INTO sessions (token_hash, user_id, expires_at) VALUES (?1, ?2, ?3) \
                 ON CONFLICT(token_hash) DO UPDATE SET user_id=excluded.user_id, expires_at=excluded.expires_at",
            )
            .map_err(|e| format!("Failed to prepare session upsert: {e}"))?;
        for (hash, session) in &store.sessions {
            upsert_stmt
                .execute(params![hash, session.user_id, session.expires_at as i64])
                .map_err(|e| format!("Failed to upsert session {hash}: {e}"))?;
        }
    }

    {
        let mut existing_keys = Vec::new();
        {
            let mut select_stmt = tx
                .prepare("SELECT code_hash FROM login_codes")
                .map_err(|e| format!("Failed to prepare select login_code hashes: {e}"))?;
            let rows = select_stmt
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(|e| format!("Failed to query login_code hashes: {e}"))?;
            for k in rows {
                existing_keys.push(k.map_err(|e| format!("Failed to read login_code hash: {e}"))?);
            }
        }
        let mut del_stmt = tx
            .prepare("DELETE FROM login_codes WHERE code_hash = ?1")
            .map_err(|e| format!("Failed to prepare delete login_code: {e}"))?;
        for k in existing_keys {
            if !store.login_codes.contains_key(&k) {
                del_stmt
                    .execute(params![k])
                    .map_err(|e| format!("Failed to delete login_code {k}: {e}"))?;
            }
        }

        let mut upsert_stmt = tx
            .prepare(
                "INSERT INTO login_codes (code_hash, email, expires_at, consumed_at, login_handle_hash) \
                 VALUES (?1, ?2, ?3, ?4, ?5) \
                 ON CONFLICT(code_hash) DO UPDATE SET \
                 email=excluded.email, expires_at=excluded.expires_at, \
                 consumed_at=excluded.consumed_at, login_handle_hash=excluded.login_handle_hash",
            )
            .map_err(|e| format!("Failed to prepare login_code upsert: {e}"))?;
        for (hash, code) in &store.login_codes {
            upsert_stmt
                .execute(params![
                    hash,
                    code.email,
                    code.expires_at as i64,
                    code.consumed_at.map(|v| v as i64),
                    code.login_handle_hash
                ])
                .map_err(|e| format!("Failed to upsert login_code {hash}: {e}"))?;
        }
    }

    {
        let mut existing_keys = Vec::new();
        {
            let mut select_stmt = tx
                .prepare("SELECT code_hash FROM enrollment_codes")
                .map_err(|e| format!("Failed to prepare select enrollment_code hashes: {e}"))?;
            let rows = select_stmt
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(|e| format!("Failed to query enrollment_code hashes: {e}"))?;
            for k in rows {
                existing_keys
                    .push(k.map_err(|e| format!("Failed to read enrollment_code hash: {e}"))?);
            }
        }
        let mut del_stmt = tx
            .prepare("DELETE FROM enrollment_codes WHERE code_hash = ?1")
            .map_err(|e| format!("Failed to prepare delete enrollment_code: {e}"))?;
        for k in existing_keys {
            if !store.enrollment_codes.contains_key(&k) {
                del_stmt
                    .execute(params![k])
                    .map_err(|e| format!("Failed to delete enrollment_code {k}: {e}"))?;
            }
        }

        let mut upsert_stmt = tx
            .prepare(
                "INSERT INTO enrollment_codes (code_hash, user_id, account_origin, expires_at, \
                 kind, key_id, label, created_at, consumed_at, consumed_machine_id, \
                 bound_machine_id, bound_public_key) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12) \
                 ON CONFLICT(code_hash) DO UPDATE SET \
                 user_id=excluded.user_id, account_origin=excluded.account_origin, expires_at=excluded.expires_at, \
                 kind=excluded.kind, key_id=excluded.key_id, label=excluded.label, \
                 created_at=excluded.created_at, consumed_at=excluded.consumed_at, \
                 consumed_machine_id=excluded.consumed_machine_id, \
                 bound_machine_id=excluded.bound_machine_id, bound_public_key=excluded.bound_public_key",
            )
            .map_err(|e| format!("Failed to prepare enrollment_code upsert: {e}"))?;
        for (hash, code) in &store.enrollment_codes {
            upsert_stmt
                .execute(params![
                    hash,
                    code.user_id,
                    code.account_origin,
                    code.expires_at as i64,
                    code.kind,
                    code.key_id,
                    code.label,
                    code.created_at as i64,
                    code.consumed_at.map(|v| v as i64),
                    code.consumed_machine_id,
                    code.bound_machine_id,
                    code.bound_public_key
                ])
                .map_err(|e| format!("Failed to upsert enrollment_code {hash}: {e}"))?;
        }
    }

    {
        let mut existing_keys = Vec::new();
        {
            let mut select_stmt = tx
                .prepare("SELECT grant_id FROM grants")
                .map_err(|e| format!("Failed to prepare select grant ids: {e}"))?;
            let rows = select_stmt
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(|e| format!("Failed to query grant ids: {e}"))?;
            for k in rows {
                existing_keys.push(k.map_err(|e| format!("Failed to read grant id: {e}"))?);
            }
        }
        let mut del_stmt = tx
            .prepare("DELETE FROM grants WHERE grant_id = ?1")
            .map_err(|e| format!("Failed to prepare delete grant: {e}"))?;
        for k in existing_keys {
            if !store.grants.contains_key(&k) {
                del_stmt
                    .execute(params![k])
                    .map_err(|e| format!("Failed to delete grant {k}: {e}"))?;
            }
        }

        let mut upsert_stmt = tx
            .prepare(
                "INSERT INTO grants (grant_id, machine_record_id, owner_user_id, pairing_token_hash, \
                 grant_scope, device_attach_public_key, installation_id, issued_at, expires_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
                 ON CONFLICT(grant_id) DO UPDATE SET \
                 machine_record_id=excluded.machine_record_id, owner_user_id=excluded.owner_user_id, \
                 pairing_token_hash=excluded.pairing_token_hash, grant_scope=excluded.grant_scope, \
                 device_attach_public_key=excluded.device_attach_public_key, installation_id=excluded.installation_id, \
                 issued_at=excluded.issued_at, expires_at=excluded.expires_at",
            )
            .map_err(|e| format!("Failed to prepare grant upsert: {e}"))?;
        for grant in store.grants.values() {
            upsert_stmt
                .execute(params![
                    grant.grant_id,
                    grant.machine_record_id,
                    grant.owner_user_id,
                    grant.pairing_token_hash,
                    grant.grant_scope,
                    grant.device_attach_public_key,
                    grant.installation_id,
                    grant.issued_at as i64,
                    grant.expires_at as i64
                ])
                .map_err(|e| format!("Failed to upsert grant {}: {e}", grant.grant_id))?;
        }
    }

    {
        let mut existing_keys = Vec::new();
        {
            let mut select_stmt = tx
                .prepare("SELECT device_code_hash FROM device_auths")
                .map_err(|e| format!("Failed to prepare select device_auth hashes: {e}"))?;
            let rows = select_stmt
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(|e| format!("Failed to query device_auth hashes: {e}"))?;
            for k in rows {
                existing_keys.push(k.map_err(|e| format!("Failed to read device_auth hash: {e}"))?);
            }
        }
        let mut del_stmt = tx
            .prepare("DELETE FROM device_auths WHERE device_code_hash = ?1")
            .map_err(|e| format!("Failed to prepare delete device_auth: {e}"))?;
        for k in existing_keys {
            if !store.device_auths.contains_key(&k) {
                del_stmt
                    .execute(params![k])
                    .map_err(|e| format!("Failed to delete device_auth {k}: {e}"))?;
            }
        }

        let mut upsert_stmt = tx
            .prepare(
                "INSERT INTO device_auths (device_code_hash, user_code, email, email_token_hash, enrollment_code, expires_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
                 ON CONFLICT(device_code_hash) DO UPDATE SET \
                 user_code=excluded.user_code, email=excluded.email, email_token_hash=excluded.email_token_hash, \
                 enrollment_code=excluded.enrollment_code, expires_at=excluded.expires_at",
            )
            .map_err(|e| format!("Failed to prepare device_auth upsert: {e}"))?;
        for auth in store.device_auths.values() {
            upsert_stmt
                .execute(params![
                    auth.device_code_hash,
                    auth.user_code,
                    auth.email,
                    auth.email_token_hash,
                    auth.enrollment_code,
                    auth.expires_at as i64
                ])
                .map_err(|e| {
                    format!("Failed to upsert device_auth {}: {e}", auth.device_code_hash)
                })?;
        }
    }

    {
        let mut existing_keys = Vec::new();
        {
            let mut select_stmt = tx
                .prepare("SELECT subscription_id FROM subscriptions")
                .map_err(|e| format!("Failed to prepare select subscription ids: {e}"))?;
            let rows = select_stmt
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(|e| format!("Failed to query subscription ids: {e}"))?;
            for k in rows {
                existing_keys.push(k.map_err(|e| format!("Failed to read subscription id: {e}"))?);
            }
        }
        let mut del_stmt = tx
            .prepare("DELETE FROM subscriptions WHERE subscription_id = ?1")
            .map_err(|e| format!("Failed to prepare delete subscription: {e}"))?;
        for k in existing_keys {
            if !store.subscriptions.contains_key(&k) {
                del_stmt
                    .execute(params![k])
                    .map_err(|e| format!("Failed to delete subscription {k}: {e}"))?;
            }
        }

        let mut upsert_stmt = tx
            .prepare(
                "INSERT INTO subscriptions (subscription_id, owner_user_id, org_id, plan_key, seats, \
                 host_packs, kind, status, ends_at, ls_customer_id, ls_updated_at, manage_url) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12) \
                 ON CONFLICT(subscription_id) DO UPDATE SET \
                 owner_user_id=excluded.owner_user_id, org_id=excluded.org_id, plan_key=excluded.plan_key, \
                 seats=excluded.seats, host_packs=excluded.host_packs, kind=excluded.kind, \
                 status=excluded.status, ends_at=excluded.ends_at, ls_customer_id=excluded.ls_customer_id, \
                 ls_updated_at=excluded.ls_updated_at, manage_url=excluded.manage_url",
            )
            .map_err(|e| format!("Failed to prepare subscription upsert: {e}"))?;
        for sub in store.subscriptions.values() {
            upsert_stmt
                .execute(params![
                    sub.subscription_id,
                    sub.owner_user_id,
                    sub.org_id,
                    sub.plan_key,
                    sub.seats as i64,
                    sub.host_packs as i64,
                    sub.kind,
                    sub.status,
                    sub.ends_at.map(|v| v as i64),
                    sub.ls_customer_id,
                    sub.ls_updated_at as i64,
                    sub.manage_url
                ])
                .map_err(|e| {
                    format!("Failed to upsert subscription {}: {e}", sub.subscription_id)
                })?;
        }
    }

    {
        let mut existing_keys = Vec::new();
        {
            let mut select_stmt = tx
                .prepare("SELECT owner_key FROM billing_states")
                .map_err(|e| format!("Failed to prepare select billing_state keys: {e}"))?;
            let rows = select_stmt
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(|e| format!("Failed to query billing_state keys: {e}"))?;
            for k in rows {
                existing_keys.push(k.map_err(|e| format!("Failed to read billing_state key: {e}"))?);
            }
        }
        let mut del_stmt = tx
            .prepare("DELETE FROM billing_states WHERE owner_key = ?1")
            .map_err(|e| format!("Failed to prepare delete billing_state: {e}"))?;
        for k in existing_keys {
            if !store.billing_states.contains_key(&k) {
                del_stmt
                    .execute(params![k])
                    .map_err(|e| format!("Failed to delete billing_state {k}: {e}"))?;
            }
        }

        let mut upsert_stmt = tx
            .prepare(
                "INSERT INTO billing_states (owner_key, grace_started_at, stopped_at, last_notice) \
                 VALUES (?1, ?2, ?3, ?4) \
                 ON CONFLICT(owner_key) DO UPDATE SET \
                 grace_started_at=excluded.grace_started_at, stopped_at=excluded.stopped_at, last_notice=excluded.last_notice",
            )
            .map_err(|e| format!("Failed to prepare billing_state upsert: {e}"))?;
        for bs in store.billing_states.values() {
            upsert_stmt
                .execute(params![
                    bs.owner_key,
                    bs.grace_started_at.map(|v| v as i64),
                    bs.stopped_at.map(|v| v as i64),
                    bs.last_notice
                ])
                .map_err(|e| format!("Failed to upsert billing_state {}: {e}", bs.owner_key))?;
        }
    }

    {
        let mut existing_keys = Vec::new();
        {
            let mut select_stmt = tx
                .prepare("SELECT event_key FROM payment_events")
                .map_err(|e| format!("Failed to prepare select payment_event keys: {e}"))?;
            let rows = select_stmt
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(|e| format!("Failed to query payment_event keys: {e}"))?;
            for k in rows {
                existing_keys.push(k.map_err(|e| format!("Failed to read payment_event key: {e}"))?);
            }
        }
        let mut del_stmt = tx
            .prepare("DELETE FROM payment_events WHERE event_key = ?1")
            .map_err(|e| format!("Failed to prepare delete payment_event: {e}"))?;
        for k in existing_keys {
            if !store.payment_events.contains_key(&k) {
                del_stmt
                    .execute(params![k])
                    .map_err(|e| format!("Failed to delete payment_event {k}: {e}"))?;
            }
        }

        let mut upsert_stmt = tx
            .prepare(
                "INSERT INTO payment_events (event_key, event_name, received_at, applied_at, outcome) \
                 VALUES (?1, ?2, ?3, ?4, ?5) \
                 ON CONFLICT(event_key) DO UPDATE SET \
                 event_name=excluded.event_name, received_at=excluded.received_at, \
                 applied_at=excluded.applied_at, outcome=excluded.outcome",
            )
            .map_err(|e| format!("Failed to prepare payment_event upsert: {e}"))?;
        for pe in store.payment_events.values() {
            upsert_stmt
                .execute(params![
                    pe.event_key,
                    pe.event_name,
                    pe.received_at as i64,
                    pe.applied_at.map(|v| v as i64),
                    pe.outcome
                ])
                .map_err(|e| format!("Failed to upsert payment_event {}: {e}", pe.event_key))?;
        }
    }

    {
        let mut existing_keys = Vec::new();
        {
            let mut select_stmt = tx
                .prepare("SELECT org_id, user_id FROM org_members")
                .map_err(|e| format!("Failed to prepare select org_members: {e}"))?;
            let rows = select_stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
                .map_err(|e| format!("Failed to query org_members: {e}"))?;
            for k in rows {
                existing_keys.push(k.map_err(|e| format!("Failed to read org_member: {e}"))?);
            }
        }
        let mut del_stmt = tx
            .prepare("DELETE FROM org_members WHERE org_id = ?1 AND user_id = ?2")
            .map_err(|e| format!("Failed to prepare delete org_member: {e}"))?;
        for (org_id, user_id) in existing_keys {
            let key = format!("{org_id}:{user_id}");
            if !store.org_members.contains_key(&key) {
                del_stmt
                    .execute(params![org_id, user_id])
                    .map_err(|e| format!("Failed to delete org_member {key}: {e}"))?;
            }
        }

        let mut upsert_stmt = tx
            .prepare(
                "INSERT INTO org_members (org_id, user_id, role, joined_at) VALUES (?1, ?2, ?3, ?4) \
                 ON CONFLICT(org_id, user_id) DO UPDATE SET role=excluded.role, joined_at=excluded.joined_at",
            )
            .map_err(|e| format!("Failed to prepare org_member upsert: {e}"))?;
        for member in store.org_members.values() {
            upsert_stmt
                .execute(params![
                    member.org_id,
                    member.user_id,
                    member.role,
                    member.joined_at as i64
                ])
                .map_err(|e| {
                    format!(
                        "Failed to upsert org_member {}:{}: {e}",
                        member.org_id, member.user_id
                    )
                })?;
        }
    }

    {
        let mut existing_keys = Vec::new();
        {
            let mut select_stmt = tx
                .prepare("SELECT token_hash FROM org_invites")
                .map_err(|e| format!("Failed to prepare select org_invites: {e}"))?;
            let rows = select_stmt
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(|e| format!("Failed to query org_invites: {e}"))?;
            for k in rows {
                existing_keys.push(k.map_err(|e| format!("Failed to read org_invite token: {e}"))?);
            }
        }
        let mut del_stmt = tx
            .prepare("DELETE FROM org_invites WHERE token_hash = ?1")
            .map_err(|e| format!("Failed to prepare delete org_invite: {e}"))?;
        for k in existing_keys {
            if !store.org_invites.contains_key(&k) {
                del_stmt
                    .execute(params![k])
                    .map_err(|e| format!("Failed to delete org_invite {k}: {e}"))?;
            }
        }

        let mut upsert_stmt = tx
            .prepare(
                "INSERT INTO org_invites (token_hash, org_id, email, expires_at) VALUES (?1, ?2, ?3, ?4) \
                 ON CONFLICT(token_hash) DO UPDATE SET org_id=excluded.org_id, email=excluded.email, expires_at=excluded.expires_at",
            )
            .map_err(|e| format!("Failed to prepare org_invite upsert: {e}"))?;
        for invite in store.org_invites.values() {
            upsert_stmt
                .execute(params![
                    invite.token_hash,
                    invite.org_id,
                    invite.email,
                    invite.expires_at as i64
                ])
                .map_err(|e| {
                    format!("Failed to upsert org_invite {}: {e}", invite.token_hash)
                })?;
        }
    }

    {
        let mut existing_keys = Vec::new();
        {
            let mut select_stmt = tx
                .prepare("SELECT request_id FROM machine_requests")
                .map_err(|e| format!("Failed to prepare select machine_request ids: {e}"))?;
            let rows = select_stmt
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(|e| format!("Failed to query machine_request ids: {e}"))?;
            for k in rows {
                existing_keys
                    .push(k.map_err(|e| format!("Failed to read machine_request id: {e}"))?);
            }
        }
        let mut del_stmt = tx
            .prepare("DELETE FROM machine_requests WHERE request_id = ?1")
            .map_err(|e| format!("Failed to prepare delete machine_request: {e}"))?;
        for k in existing_keys {
            if !store.machine_requests.contains_key(&k) {
                del_stmt
                    .execute(params![k])
                    .map_err(|e| format!("Failed to delete machine_request {k}: {e}"))?;
            }
        }

        let mut upsert_stmt = tx
            .prepare(
                "INSERT INTO machine_requests (request_id, device_code_hash, user_code, machine_id, \
                 public_key, display_name, platform, app_version, email_hint, requester_ip, \
                 created_at, expires_at, status, decided_by, decided_at, enrollment_code, last_polled_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17) \
                 ON CONFLICT(request_id) DO UPDATE SET \
                 device_code_hash=excluded.device_code_hash, user_code=excluded.user_code, \
                 machine_id=excluded.machine_id, public_key=excluded.public_key, \
                 display_name=excluded.display_name, platform=excluded.platform, \
                 app_version=excluded.app_version, email_hint=excluded.email_hint, \
                 requester_ip=excluded.requester_ip, created_at=excluded.created_at, \
                 expires_at=excluded.expires_at, status=excluded.status, \
                 decided_by=excluded.decided_by, decided_at=excluded.decided_at, \
                 enrollment_code=excluded.enrollment_code, last_polled_at=excluded.last_polled_at",
            )
            .map_err(|e| format!("Failed to prepare machine_request upsert: {e}"))?;
        for request in store.machine_requests.values() {
            upsert_stmt
                .execute(params![
                    request.request_id,
                    request.device_code_hash,
                    request.user_code,
                    request.machine_id,
                    request.public_key,
                    request.display_name,
                    request.platform,
                    request.app_version,
                    request.email_hint,
                    request.requester_ip,
                    request.created_at as i64,
                    request.expires_at as i64,
                    request.status,
                    request.decided_by,
                    request.decided_at.map(|v| v as i64),
                    request.enrollment_code,
                    request.last_polled_at.map(|v| v as i64)
                ])
                .map_err(|e| {
                    format!("Failed to upsert machine_request {}: {e}", request.request_id)
                })?;
        }
    }

    // Lease rows are never deleted, and a whole-store save must never move a lease backwards:
    // `account::supervisor_lease` writes them with targeted SQL, so a store snapshot only
    // overwrites a row it is strictly newer than (higher fence, or same fence and later
    // update), and the per-account fence high-water mark only ever grows.
    if !store.supervisor_leases.is_empty() && table_exists(tx, "supervisor_leases")? {
        let mut upsert_stmt = tx
            .prepare(
                "INSERT INTO supervisor_leases (user_id, holder_machine_id, fence, expires_at, \
                 checkpoint_id, checkpoint_at, released, reserved_for, reserved_fence, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) \
                 ON CONFLICT(user_id) DO UPDATE SET \
                 holder_machine_id=excluded.holder_machine_id, fence=excluded.fence, \
                 expires_at=excluded.expires_at, checkpoint_id=excluded.checkpoint_id, \
                 checkpoint_at=excluded.checkpoint_at, released=excluded.released, \
                 reserved_for=excluded.reserved_for, reserved_fence=excluded.reserved_fence, \
                 updated_at=excluded.updated_at \
                 WHERE excluded.fence > supervisor_leases.fence \
                 OR (excluded.fence = supervisor_leases.fence \
                     AND excluded.updated_at > supervisor_leases.updated_at)",
            )
            .map_err(|e| format!("Failed to prepare supervisor_lease upsert: {e}"))?;
        let mut fence_stmt = tx
            .prepare(
                "INSERT INTO supervisor_lease_fences (user_id, last_fence) VALUES (?1, ?2) \
                 ON CONFLICT(user_id) DO UPDATE SET \
                 last_fence = MAX(supervisor_lease_fences.last_fence, excluded.last_fence)",
            )
            .map_err(|e| format!("Failed to prepare supervisor_lease_fences upsert: {e}"))?;
        for lease in store.supervisor_leases.values() {
            upsert_stmt
                .execute(params![
                    lease.user_id,
                    lease.holder_machine_id,
                    lease.fence as i64,
                    lease.expires_at as i64,
                    lease.checkpoint_id,
                    lease.checkpoint_at.map(|v| v as i64),
                    i64::from(lease.released),
                    lease.reserved_for,
                    lease.reserved_fence.map(|v| v as i64),
                    lease.updated_at as i64
                ])
                .map_err(|e| format!("Failed to upsert supervisor_lease {}: {e}", lease.user_id))?;
            fence_stmt
                .execute(params![lease.user_id, lease.fence as i64])
                .map_err(|e| {
                    format!("Failed to upsert supervisor_lease_fence {}: {e}", lease.user_id)
                })?;
        }
    }

    Ok(())
}
