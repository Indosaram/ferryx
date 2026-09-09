# Windows native stdio byte probe

The lead's hypothesis that the bridge's sole native command necessarily passes
through a PowerShell text pipeline was disproved by actual Windows OpenSSH.
No production change is justified by that hypothesis.

Host: existing trusted `maho-win`. Options: `-T`, `BatchMode=yes`,
`StrictHostKeyChecking=yes`, `UpdateHostKeys=no`, `ConnectTimeout=5`.
The remote command was Windows PowerShell with `-NoLogo -NoProfile
-NonInteractive -EncodedCommand`; the script was encoded as UTF-16LE Base64.

Output-only script:

```powershell
& bun -e 'process.stdout.write(Buffer.from([0,255,128,10,13,65]))'
```

- Expected hex: `00ff800a0d41`.
- Received hex: `00ff800a0d41`.
- Exit: 0; stderr empty.

Bidirectional script:

```powershell
& bun -e 'for await (const c of process.stdin) process.stdout.write(c)'
```

- Input hex: `0000000600ff800a0d41`.
- Received hex: `0000000600ff800a0d41`.
- Exit: 0; stderr empty.

This invocation uses one native executable, not a PowerShell object pipeline.
The null bytes, non-UTF-8 bytes, LF and CR were all preserved. There were no
remote files, helper daemons or PTYs to clean up, and both SSH children exited.
This refutes the specific text-conversion claim; it does not substitute for a
real helper handshake, framed session operations or process-survival proof.
