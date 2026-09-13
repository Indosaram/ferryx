# Platform availability preflight

The parent performed read-only SSH probes on the user-designated Linux and
Windows verification hosts. Both commands exited 0. This is environment
availability evidence, not compilation, terminal, native UI or platform
acceptance.

## Linux

- Host: indo@100.91.254.71, the omaki bench.
- Linux 7.1.9-arch1-2, x86_64, 12 logical CPUs.
- Home filesystem: 626579668 available 1024-byte blocks, approximately 597.6 GiB.
- Cargo 1.98.0 and rustc 1.98.0 in /home/indo/.cargo/bin.
- Bun 1.4.0 in /home/indo/.bun/bin; Git 2.55.0.
- Source: PLATFORM-resume-linux-preflight.log.
- Monitor mon_EM7WGJ0C8RRHPY8W / bash_1, completed with exit 0.

The command queried uname, processor count, home filesystem capacity, command
locations and tool versions through a noninteractive SSH login shell. WSL was
not used; the recorded user instruction designates omaki as its replacement.

## Windows

- Host: maho-win.
- Microsoft Windows NT 10.0.26200.0, 12 logical CPUs.
- C: free 197388771328 bytes, approximately 183.8 GiB.
- Cargo 1.97.0 and rustc 1.97.0 in C:\Users\sook\.cargo\bin.
- Bun 1.4.0 in C:\Users\sook\.bun\bin; Git 2.55.0.windows.2.
- Source: PLATFORM-resume-windows-preflight.log.
- Monitor mon_W3Y1B43Y2TD6CVN7 / bash_2, completed with exit 0.

The command queried OS/process counts, Get-PSDrive, Get-Command and tool
versions through noninteractive PowerShell. The raw CLIXML module-preparation
progress remains in the log. No task, application or GUI was launched.

## Acceptance still required

No source was copied to either host, no build or test ran there, and no daemon,
terminal session, installed app or existing checkout was changed. The final
composed source still needs the plan's Linux and Windows checks. SSH process
existence cannot prove an interactive Windows window; native desktop acceptance
requires the prescribed debug launch and manual user verification where needed.
