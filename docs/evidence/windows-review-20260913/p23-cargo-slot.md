# P23 exclusive execution request (st_01a09a09)

Lead action needed: grant this child exclusive shared Darwin Cargo slot, or execute staged runner `bash docs/evidence/windows-review-20260913/p23-run.sh red` and relay logs. No Cargo commands have been run by this child. Production changes must wait for intended RED. Native GB-03 coordinator fixture RED and GREEN belong to runtime owner st_01a099f8; run only exact coordinator test on owned Windows profile and PTY.

Requested exact targets (one selected test each, --exact --nocapture --test-threads=1):
- remote::relay_server::tests::test_relay_key_store_transaction_is_cross_process
- remote::relay_server::tests::test_relay_key_store_merges_concurrent_enrollments_and_rejects_corrupt_records
- remote::relay_server::tests::test_relay_dispatch_rejects_a_replaced_control_channel
- remote::relay_server::tests::test_relay_register_pairing_caps_lease_lifetime
- remote::relay_server::tests::coordinator_pairs_through_relay_to_real_gateway

No broad filter, daemon, release/install, remote mutation or competing target directory authorized/requested.
