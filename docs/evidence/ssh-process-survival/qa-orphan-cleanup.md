# Phase B orphan QA cleanup

Lead process audit found 78 detached QA daemons after their fixtures had returned.
All entries below had PPID 1, the exact helper executable and isolated test root.
Producers were told to stop further spawning before cleanup. No production daemon
or user session matches this allowlist. Each PID is revalidated before signaling.

## Result

All 78 PIDs still matched their captured PPID, executable, root and host ID when
the lead revalidated them. Only those exact processes received SIGTERM.
Monitor `mon_CH4ZCYRZVBKSPH5X` exited 0 with `QA_ORPHAN_HELPERS_GONE`.
All 78 parent fixture directories were already absent. That explains why the
old fixture's post-deletion `lsof` lookup could not find the detached owner.

This receipt retracts earlier Phase B cleanup claims that counted sent signals
as reaping. Bridge-only tests must own their daemon `Child` and kill/wait before
dropping the directory; the dedicated setup scenario separately verifies
detached ownership. Future runs must prove their own cleanup rather than rely
on this retrospective process sweep.

```json
[
  {
    "pid": 18689,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpqYnRNQ/state",
    "host": "test-reap-7d3394be-4661-4d3d-8659-3c3dbd5900ed"
  },
  {
    "pid": 18690,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpKWxWL6/state",
    "host": "test-independent-streams-859c6559-d23d-4d22-a468-262129460d61"
  },
  {
    "pid": 18691,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpXwyrOD/state",
    "host": "test-handshake-bc1b23a0-286b-43db-b6c8-774b9b9f7770"
  },
  {
    "pid": 18692,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpkdR91E/state",
    "host": "test-spawn-retry-e9519d58-3b31-4ab0-b900-770f04575593"
  },
  {
    "pid": 18693,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpxfbLlH/state",
    "host": "test-single-attempt-2e212325-5d70-45a5-8548-de4472f4620e"
  },
  {
    "pid": 18694,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpMIA9ks/state",
    "host": "test-target-expired-980e74fe-e30c-4107-ba9f-52fb04383cd4"
  },
  {
    "pid": 18713,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpX3cCA0/state",
    "host": "test-live-loopback-0f5a0ae6-9995-4a26-ad64-48bad43fad41"
  },
  {
    "pid": 18806,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmp5IFmTA/state",
    "host": "test-independent-streams-c6473703-8b28-474f-a4f4-68d1cbdfb7c8"
  },
  {
    "pid": 18807,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpxtqUup/state",
    "host": "test-spawn-retry-02691bf8-f34f-4a4d-8111-02d226f5cb0c"
  },
  {
    "pid": 18808,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpYHZQKE/state",
    "host": "test-target-expired-425e11f4-c8ad-46c7-a6aa-62a9bf583b4c"
  },
  {
    "pid": 18809,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpHwvuOY/state",
    "host": "test-handshake-ce28eb87-fd0d-4308-b401-c3f46ece62c7"
  },
  {
    "pid": 18810,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpzeaYb6/state",
    "host": "test-single-attempt-21174afa-4d0a-44ab-9159-8c1ca2881aaa"
  },
  {
    "pid": 18811,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpnQbwQl/state",
    "host": "test-reap-55fb0397-e35b-419c-9e26-22f2ac285da1"
  },
  {
    "pid": 18831,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmp2NIeAx/state",
    "host": "test-live-loopback-a4fc0717-e68a-4762-bd91-538a3fb6f27a"
  },
  {
    "pid": 18920,
    "exe": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/ferryx-bridge-qa-TV6J5J/bin/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/ferryx-bridge-qa-TV6J5J/state",
    "host": "qa-bridge-ecae2486"
  },
  {
    "pid": 18962,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmphQVMoP/state",
    "host": "test-target-expired-4639350e-c609-4855-9d07-e8a8efe9155c"
  },
  {
    "pid": 18963,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpq2JOZI/state",
    "host": "test-handshake-46c1c729-d445-46df-ae6f-d777ce17aa81"
  },
  {
    "pid": 18964,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmp2hCLWG/state",
    "host": "test-spawn-retry-6ba7bd85-1287-414a-8f29-e45f41f18489"
  },
  {
    "pid": 18965,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpUuBHbK/state",
    "host": "test-reap-739ad73a-a1c0-4813-94c8-5ae3c0c4efa6"
  },
  {
    "pid": 18966,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpNyUTfu/state",
    "host": "test-single-attempt-d7fa6bae-e53a-4775-9407-23afe2ae7bdd"
  },
  {
    "pid": 18967,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpyVG3ea/state",
    "host": "test-independent-streams-b955dd2b-0d27-4685-8a47-ba1585ee6d73"
  },
  {
    "pid": 18988,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpZW2qJd/state",
    "host": "test-live-loopback-48aa9390-d06f-4847-8631-0671e1fc8daa"
  },
  {
    "pid": 20005,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpppNSiJ/state",
    "host": "test-reap-02f79010-c92f-404a-8f09-917fd99cac0d"
  },
  {
    "pid": 20006,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpPy4b83/state",
    "host": "test-single-attempt-5b82bef8-c87f-4b19-8be2-a802fd1e20ce"
  },
  {
    "pid": 20007,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpjZAXUh/state",
    "host": "test-spawn-retry-ac0d953c-df0d-40b5-b12a-a044e4756c3d"
  },
  {
    "pid": 20008,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpLDplST/state",
    "host": "test-independent-streams-56f9d2a0-7130-4956-9819-afd28abaf17b"
  },
  {
    "pid": 20009,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmp5rqksl/state",
    "host": "test-target-expired-5160faaf-cb7c-4de5-9cce-8a06b6b2f73b"
  },
  {
    "pid": 20010,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpP5mftc/state",
    "host": "test-handshake-71c0f4ae-3f3b-4368-8ebd-ff22bee4911f"
  },
  {
    "pid": 20030,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpL3yiio/state",
    "host": "test-live-loopback-608229e9-ad00-45f5-9b17-10fcdfa2ef64"
  },
  {
    "pid": 20418,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpP5hAgG/state",
    "host": "test-spawn-retry-1198a473-3954-4649-a42a-62d867a92aaf"
  },
  {
    "pid": 20419,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpLJgQwj/state",
    "host": "test-independent-streams-4c2a2e0d-f9ad-466e-92e8-b2016742aba7"
  },
  {
    "pid": 20420,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpGp3hWT/state",
    "host": "test-target-expired-c561e179-d12c-43c2-86f5-f4e002c32643"
  },
  {
    "pid": 20421,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpaRfzy5/state",
    "host": "test-handshake-f9e90aed-79ab-4289-8840-7cc740b2dee4"
  },
  {
    "pid": 20422,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpD4LCiR/state",
    "host": "test-reap-ef743fd5-92f3-4b61-a63f-79a997d17bf8"
  },
  {
    "pid": 20423,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmp88xQxS/state",
    "host": "test-single-attempt-0a5ed56b-06f8-4de7-ad54-c848846b21fa"
  },
  {
    "pid": 20443,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpkNA2eQ/state",
    "host": "test-live-loopback-71e5b609-6c6e-4d03-bd5e-f81f1bc20a94"
  },
  {
    "pid": 20487,
    "exe": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/ferryx-bridge-qa-LFx6h4/bin/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/ferryx-bridge-qa-LFx6h4/state",
    "host": "qa-bridge-7876350b"
  },
  {
    "pid": 20628,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpZeBKV0/state",
    "host": "test-single-attempt-f9518a84-1dbb-465d-8944-bcd1c2aa3117"
  },
  {
    "pid": 20629,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpYYaG5H/state",
    "host": "test-handshake-0d374935-c511-42db-8780-dfa9fb179ad7"
  },
  {
    "pid": 20630,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmppW6PV7/state",
    "host": "test-independent-streams-9bb94bcd-8462-43e3-b9c1-9c90d5db9692"
  },
  {
    "pid": 20631,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpT19yvX/state",
    "host": "test-spawn-retry-c9031ce2-bfca-40a6-895e-13bafb685e4e"
  },
  {
    "pid": 20632,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpEABQgE/state",
    "host": "test-reap-4d9275f7-ef6f-4284-a221-46b930561a89"
  },
  {
    "pid": 20633,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpWpCLjH/state",
    "host": "test-target-expired-a18f3e25-b755-4064-a24d-7486b3b0042d"
  },
  {
    "pid": 20653,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpXeznsx/state",
    "host": "test-live-loopback-1a9afb05-8a3d-414e-bf8a-4a4d01df4e3a"
  },
  {
    "pid": 20843,
    "exe": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/ferryx-bridge-qa-LQr4Pv/bin/ferryx-remote-helper",
    "root": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/ferryx-bridge-qa-LQr4Pv/state",
    "host": "qa-bridge-3a278430"
  },
  {
    "pid": 22092,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/private/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpuUlr3U/state",
    "host": "test-independent-streams-c1d8b6d4-0b1b-4248-989c-c3bbf4cac8b6"
  },
  {
    "pid": 22093,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/private/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpTUMnpN/state",
    "host": "test-reap-c0bba5ec-22a9-49fb-8a71-edb5cf5187b5"
  },
  {
    "pid": 22094,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/private/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmp5lbMoG/state",
    "host": "test-single-attempt-eb9c89a0-bc04-4040-9d09-2a1b1f3e844f"
  },
  {
    "pid": 22095,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/private/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpGo9B3L/state",
    "host": "test-live-loopback-d3efce4e-7c4f-43dc-be2e-2db8cdd1be68"
  },
  {
    "pid": 22096,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/private/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpV2q3MY/state",
    "host": "test-spawn-retry-4f04267c-9b00-47ba-b2b3-726d997c9725"
  },
  {
    "pid": 22097,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/private/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmp0BPG1z/state",
    "host": "test-target-expired-8c1c1d10-1294-4ef9-85d0-b37f0d5f09c5"
  },
  {
    "pid": 22098,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/private/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpWYdm3h/state",
    "host": "test-handshake-2ff3f189-f6dd-4e0c-bf9e-16fe93d24642"
  },
  {
    "pid": 22258,
    "exe": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/ferryx-bridge-qa-m4bCGI/bin/ferryx-remote-helper",
    "root": "/private/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/ferryx-bridge-qa-m4bCGI/state",
    "host": "qa-bridge-4f1261b5"
  },
  {
    "pid": 22370,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/private/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpTLV64W/state",
    "host": "test-target-expired-d6cf1e24-c5cf-449c-a4dc-6fd2d865be0f"
  },
  {
    "pid": 22371,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/private/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpuq0Ejf/state",
    "host": "test-independent-streams-0486d629-62af-46f7-80a6-7455d52bed0b"
  },
  {
    "pid": 22372,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/private/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmp0Bjm6G/state",
    "host": "test-reap-e1fca61b-4bfd-4153-a2d5-9ffc7ad08758"
  },
  {
    "pid": 22373,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/private/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpEhOfsM/state",
    "host": "test-spawn-retry-d93f5c17-cfa5-4ccb-bc33-9aeccb066420"
  },
  {
    "pid": 22374,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/private/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpHfXV4U/state",
    "host": "test-single-attempt-6c432acf-536b-404e-b9f6-23bd532b1788"
  },
  {
    "pid": 22375,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/private/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpFfgDRO/state",
    "host": "test-handshake-34ed7f15-f14a-4faa-b195-86a57e0b1275"
  },
  {
    "pid": 22395,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/private/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpOyhPCB/state",
    "host": "test-live-loopback-850e9e21-d4df-4245-8837-054159edd189"
  },
  {
    "pid": 22541,
    "exe": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/ferryx-bridge-qa-98rFi4/bin/ferryx-remote-helper",
    "root": "/private/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/ferryx-bridge-qa-98rFi4/state",
    "host": "qa-bridge-e6c41e7c"
  },
  {
    "pid": 23664,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/private/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpqQnqOe/state",
    "host": "test-handshake-961c669e-ebe5-4f91-9c10-f6f5da7ef84a"
  },
  {
    "pid": 23665,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/private/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpQXQ2Gk/state",
    "host": "test-target-mismatch-5f0c0cec-daf3-4d22-8f14-df05dbe6ae37"
  },
  {
    "pid": 23666,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/private/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpHlVZVk/state",
    "host": "test-single-attempt-6410b607-5ed2-4d74-abda-2ebe8ce9139a"
  },
  {
    "pid": 23667,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/private/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmp84584k/state",
    "host": "test-independent-streams-85462d73-4b60-43ce-a8c6-d42fdf0c50b8"
  },
  {
    "pid": 23668,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/private/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmp4Ef37U/state",
    "host": "test-lifecycle-reaping-7a5ea65f-93d0-4da9-aa2c-ee3a5bbf6cd5"
  },
  {
    "pid": 23669,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/private/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpChnJdq/state",
    "host": "test-spawn-retry-b07b10a8-fe3f-4dec-9b64-af6a0f7cea95"
  },
  {
    "pid": 23670,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/private/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmp3jWtca/state",
    "host": "test-target-expired-f9e65bf8-ef5b-4038-8112-47b4d47699e1"
  },
  {
    "pid": 23694,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/private/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpAJMTrQ/state",
    "host": "test-live-loopback-7aed6771-ffc6-48ff-bc9e-f7fdb9c98981"
  },
  {
    "pid": 23826,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/private/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmp83pckX/state",
    "host": "test-independent-streams-e6e21e7c-b7fd-4354-8bfc-5c51703aecf3"
  },
  {
    "pid": 23827,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/private/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpltKV1i/state",
    "host": "test-handshake-81cbf383-5267-451a-8256-a873bf879536"
  },
  {
    "pid": 23828,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/private/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpghsjFB/state",
    "host": "test-target-mismatch-20b6506c-6236-468c-b2db-04883bb898dd"
  },
  {
    "pid": 23829,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/private/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpZuXKoT/state",
    "host": "test-lifecycle-reaping-d47f1c87-9828-4466-ab09-70ee61f9c1e6"
  },
  {
    "pid": 23830,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/private/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpIbmPTT/state",
    "host": "test-single-attempt-eb73ad91-4ef2-414f-a3c2-0c67746c3f2c"
  },
  {
    "pid": 23831,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/private/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpU3fRbR/state",
    "host": "test-spawn-retry-566e3c33-e343-492f-8748-b4d71b2e6572"
  },
  {
    "pid": 23832,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/private/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpWmHhd2/state",
    "host": "test-target-expired-c9085160-23a4-4216-bac4-11619f3f3358"
  },
  {
    "pid": 23853,
    "exe": "/Users/indo/code/project/orca-lite/src-tauri/../remote-helper/target/debug/ferryx-remote-helper",
    "root": "/private/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpqRaGUZ/state",
    "host": "test-live-loopback-22d9cf17-676b-449c-9935-c4f1b2ebfa8e"
  },
  {
    "pid": 24742,
    "exe": "/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/ferryx-bridge-qa-NuBd1z/bin/ferryx-remote-helper",
    "root": "/private/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/ferryx-bridge-qa-NuBd1z/state",
    "host": "qa-bridge-6d5d12f0"
  }
]
```
