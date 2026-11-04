# Task 29: foreign layout snapshots

Snapshots retain their origin machineId and machine-local revision. LayoutOffer
stores a foreign tree as an offer in the receiving window, without modifying its
tabs. LayoutImport requires the offered revision and allocates a fresh ws-* window.
Imported terminal references become disconnectedTerminal content, including when
the foreign backend id collides with a local id. No terminal service is called.

The desktop Detached section exposes the explicit open action. Pending offers and
imported placeholders use the existing durable layout files and handover snapshot.
The protocol additions are owner-only; protocol version remains 5.

Isolated QA exchange helper (Bun only):

```
bun scripts/qa/layout-intents.mjs --runtime <A-runtime> --get > <snapshot.json>
bun scripts/qa/layout-intents.mjs --runtime <B-runtime> --offer <snapshot.json>
bun scripts/qa/layout-intents.mjs --runtime <B-runtime> --import <machineId> --revision <revision>
```

Use the source snapshot's machineId and revision for the final command. A newer
source revision is still offer-only. Compare B's ferryx-cli agents list before and
after import. Import creates layout state only; desktop open additionally creates
the actual window. Failed desktop window creation leaves the imported layout on
disk rather than discarding it.

Implementation-first handoff: no build, test, QA, mutation, or host gate was run.
Integration must run the plan acceptance command and frontend tests. server.rs and
protocol.rs edits are required to expose offer/import requests and capability;
client tracing, owner authorization, IPC registration, and the capability fixture
are additive supporting changes. Account/paired snapshot producers should submit
LayoutOffer, not use the trusted same-host handover adoption path. No account
layout upload/download mechanism existed in the referenced code; this task adds
the conflict boundary, not a new account sync transport.
