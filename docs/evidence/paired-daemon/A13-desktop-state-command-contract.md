# A13 desktop state: native IPC handoff (not full A13 acceptance)

Commands are implemented by the parent IPC lane; the UI adapter uses Tauri core invoke.
All commands must restrict use to native desktop. No response returns a bearer.

```ts
interface HostView {
  hostId: string; relayOrigin: string; machineId: string; displayLabel: string;
  grantScope: "mirror" | "machine";
  generation: string; // canonical decimal u64
  authStatus: "paired" | "needsMachineGrant" | "revoked" | "unknown";
  online: boolean;
}
interface MigrationReceipt { hostId: string; generation: string }
```

| Command | Tauri arguments | Response |
| --- | --- | --- |
| `paired_host_list` | none | `HostView[]` |
| `paired_host_capabilities` | none | `{ pairedHostInventoryV1: boolean, pairedDaemonProxyV1: boolean }` |
| `paired_host_pair` | `{ request: { relayOrigin, pin, displayLabel } }` | `HostView` |
| `paired_host_migrate_legacy` | `{ request: { relayOrigin, machineId, displayLabel, deviceToken } }` | `MigrationReceipt` |
| `paired_host_read` | `{ request: { hostId, generation } }` | `HostView` |
| `paired_host_forget` | `{ request: { hostId, generation } }` | `void` |

All omitted field types above are strings. Origin is normalized HTTPS without userinfo/path/query/fragment. UI does not enable loopback HTTP. Native must enforce its own origin/redirect/credential-forwarding policy.

`paired_host_read` must perform durable private-record readback for the exact host/generation and reject missing/stale records. UI verifies the returned identity/generation before any cleanup. Migration must not replace a concurrently re-paired record, must be safe to retry after renderer/storage failure, and must establish grant scope natively rather than trusting frontend token presence. Existing browser origin-wide keys and ambiguous aliases are never migration inputs.

Pairing is PIN-only from React: native performs the exchange and durable commit, increments inventory generation and cancels stale native work. Forget is called only after the caller's confirmation; native removes exact-generation local credentials/references and cancels native work, without remote deletion or another host's token. UI adapter returns false on failure and retains rows.

Inventory capability is separate from paired proxy support. `pairedDaemonProxyV1` MUST stay false until A15/A16 support exists. Unknown local capability responses fail closed with no daemon restart. Inventory list errors must not be represented as a successful empty list. Returned errors must be sanitized natively as well; UI replaces raw invoke errors with a fixed code.

Renderer request revisions and captured host generations fence refresh/migration/pair/forget callbacks. These are not substitutes for native cancellation or authorization. Parent owns real IPC registration, Rust inventory wiring, durable readback/permissions, native HTTP policy, and full A13 acceptance.
