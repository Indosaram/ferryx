# P24 exclusive Darwin Cargo slot request

Owner st_01a09a0a; parent 01a0983f-c995-753d-afa9-593f6d118788.
Registration succeeded through official executeAgentToolkit steer/revise_criterion C002; receipt p24/registration.json preserves full existing scenario.

Lead action needed: grant exclusive shared Cargo target slot to P24, by relay or p24-slot-grant.txt. No Cargo execution has started. Exact intended runner: bash docs/evidence/windows-review-20260913/p24/run-cargo.sh red (then same runner green after intended assertion failures and scoped repair). Only dag::watcher::tests:: selected, ignored live tail excluded. Tests own temporary filesystem journals only; no daemon, audio, dialogs or profile changes.

Native action for runtime owner st_01a099f8: after local GREEN, run identical module tests on native Windows and exercise actual DAG UI in owned debug bun tauri dev: Running checkpoint -> delete entire .omo/senpi-task/dag watch target -> await loss/final scan -> recreate with Completed checkpoint -> UI updates without restart; subsequent update still emitted. Use owned slow scan fixture to prove async sentinel executes before scan release. Capture source/binary hashes, exact events, UI receipt and owned cleanup. Do not touch installed app or user daemon.
