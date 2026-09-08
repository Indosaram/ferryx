# Windows prerequisite recovery

Date: 2026-09-08. This is direct lead verification while the DAG model providers remain unavailable, not a successful DAG-node claim. It supplements the historical snapshots in probe-evidence.md. No build or runtime behavior was exercised.

## Executed scenario

SSH flags: BatchMode=yes, ConnectTimeout=10, ServerAliveInterval=5, ServerAliveCountMax=2. Host: maho-win. Payload was encoded with Buffer.from(script, "utf16le").toString("base64") and passed to powershell -NoProfile -NonInteractive -EncodedCommand. Exact PowerShell payload:

```powershell
$ProgressPreference='SilentlyContinue'; $ErrorActionPreference='Continue'; Write-Output ('HOST='+$env:COMPUTERNAME); $roots=@(${env:ProgramFiles(x86)},$env:ProgramFiles) | Where-Object { $_ }; foreach($root in $roots) { $vs=Join-Path $root 'Microsoft Visual Studio/Installer/vswhere.exe'; Write-Output ('VSWHERE_CANDIDATE='+$vs+' EXISTS='+(Test-Path -LiteralPath $vs)); if(Test-Path -LiteralPath $vs) { & $vs -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath; Write-Output ('VSWHERE_EXIT='+$LASTEXITCODE); & $vs -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -find 'VC/Tools/MSVC/**/bin/Hostx64/x64/link.exe'; Write-Output ('LINK_FIND_EXIT='+$LASTEXITCODE) }; $sdk=Join-Path $root 'Windows Kits/10/bin'; Write-Output ('SDK_CANDIDATE='+$sdk+' EXISTS='+(Test-Path -LiteralPath $sdk)); if(Test-Path -LiteralPath $sdk) { Get-ChildItem -LiteralPath $sdk -Recurse -Filter MakeAppx.exe -ErrorAction Continue | ForEach-Object { Write-Output ('MAKEAPPX='+$_.FullName) } } }; foreach($name in @('ferryx-winbuild','ferryx-win-build','rel-0905')) { $base=Join-Path $env:USERPROFILE $name; foreach($d in @($base,(Join-Path $base 'orca-lite'))) { if(Test-Path -LiteralPath $d) { Write-Output ('CHECKOUT_CANDIDATE='+$d); git -C $d rev-parse --show-toplevel; Write-Output ('ROOT_EXIT='+$LASTEXITCODE); git -C $d rev-parse HEAD; Write-Output ('HEAD_EXIT='+$LASTEXITCODE) } } }; Write-Output 'WINDOWS_READONLY_PROBE_FINISHED'
```

Exact transport command:

```sh
ssh -o BatchMode=yes -o ConnectTimeout=10 -o ServerAliveInterval=5 -o ServerAliveCountMax=2 maho-win powershell -NoProfile -NonInteractive -EncodedCommand JABQAHIAbwBnAHIAZQBzAHMAUAByAGUAZgBlAHIAZQBuAGMAZQA9ACcAUwBpAGwAZQBuAHQAbAB5AEMAbwBuAHQAaQBuAHUAZQAnADsAIAAkAEUAcgByAG8AcgBBAGMAdABpAG8AbgBQAHIAZQBmAGUAcgBlAG4AYwBlAD0AJwBDAG8AbgB0AGkAbgB1AGUAJwA7ACAAVwByAGkAdABlAC0ATwB1AHQAcAB1AHQAIAAoACcASABPAFMAVAA9ACcAKwAkAGUAbgB2ADoAQwBPAE0AUABVAFQARQBSAE4AQQBNAEUAKQA7ACAAJAByAG8AbwB0AHMAPQBAACgAJAB7AGUAbgB2ADoAUAByAG8AZwByAGEAbQBGAGkAbABlAHMAKAB4ADgANgApAH0ALAAkAGUAbgB2ADoAUAByAG8AZwByAGEAbQBGAGkAbABlAHMAKQAgAHwAIABXAGgAZQByAGUALQBPAGIAagBlAGMAdAAgAHsAIAAkAF8AIAB9ADsAIABmAG8AcgBlAGEAYwBoACgAJAByAG8AbwB0ACAAaQBuACAAJAByAG8AbwB0AHMAKQAgAHsAIAAkAHYAcwA9AEoAbwBpAG4ALQBQAGEAdABoACAAJAByAG8AbwB0ACAAJwBNAGkAYwByAG8AcwBvAGYAdAAgAFYAaQBzAHUAYQBsACAAUwB0AHUAZABpAG8ALwBJAG4AcwB0AGEAbABsAGUAcgAvAHYAcwB3AGgAZQByAGUALgBlAHgAZQAnADsAIABXAHIAaQB0AGUALQBPAHUAdABwAHUAdAAgACgAJwBWAFMAVwBIAEUAUgBFAF8AQwBBAE4ARABJAEQAQQBUAEUAPQAnACsAJAB2AHMAKwAnACAARQBYAEkAUwBUAFMAPQAnACsAKABUAGUAcwB0AC0AUABhAHQAaAAgAC0ATABpAHQAZQByAGEAbABQAGEAdABoACAAJAB2AHMAKQApADsAIABpAGYAKABUAGUAcwB0AC0AUABhAHQAaAAgAC0ATABpAHQAZQByAGEAbABQAGEAdABoACAAJAB2AHMAKQAgAHsAIAAmACAAJAB2AHMAIAAtAGwAYQB0AGUAcwB0ACAALQBwAHIAbwBkAHUAYwB0AHMAIAAnACoAJwAgAC0AcgBlAHEAdQBpAHIAZQBzACAATQBpAGMAcgBvAHMAbwBmAHQALgBWAGkAcwB1AGEAbABTAHQAdQBkAGkAbwAuAEMAbwBtAHAAbwBuAGUAbgB0AC4AVgBDAC4AVABvAG8AbABzAC4AeAA4ADYALgB4ADYANAAgAC0AcAByAG8AcABlAHIAdAB5ACAAaQBuAHMAdABhAGwAbABhAHQAaQBvAG4AUABhAHQAaAA7ACAAVwByAGkAdABlAC0ATwB1AHQAcAB1AHQAIAAoACcAVgBTAFcASABFAFIARQBfAEUAWABJAFQAPQAnACsAJABMAEEAUwBUAEUAWABJAFQAQwBPAEQARQApADsAIAAmACAAJAB2AHMAIAAtAGwAYQB0AGUAcwB0ACAALQBwAHIAbwBkAHUAYwB0AHMAIAAnACoAJwAgAC0AcgBlAHEAdQBpAHIAZQBzACAATQBpAGMAcgBvAHMAbwBmAHQALgBWAGkAcwB1AGEAbABTAHQAdQBkAGkAbwAuAEMAbwBtAHAAbwBuAGUAbgB0AC4AVgBDAC4AVABvAG8AbABzAC4AeAA4ADYALgB4ADYANAAgAC0AZgBpAG4AZAAgACcAVgBDAC8AVABvAG8AbABzAC8ATQBTAFYAQwAvACoAKgAvAGIAaQBuAC8ASABvAHMAdAB4ADYANAAvAHgANgA0AC8AbABpAG4AawAuAGUAeABlACcAOwAgAFcAcgBpAHQAZQAtAE8AdQB0AHAAdQB0ACAAKAAnAEwASQBOAEsAXwBGAEkATgBEAF8ARQBYAEkAVAA9ACcAKwAkAEwAQQBTAFQARQBYAEkAVABDAE8ARABFACkAIAB9ADsAIAAkAHMAZABrAD0ASgBvAGkAbgAtAFAAYQB0AGgAIAAkAHIAbwBvAHQAIAAnAFcAaQBuAGQAbwB3AHMAIABLAGkAdABzAC8AMQAwAC8AYgBpAG4AJwA7ACAAVwByAGkAdABlAC0ATwB1AHQAcAB1AHQAIAAoACcAUwBEAEsAXwBDAEEATgBEAEkARABBAFQARQA9ACcAKwAkAHMAZABrACsAJwAgAEUAWABJAFMAVABTAD0AJwArACgAVABlAHMAdAAtAFAAYQB0AGgAIAAtAEwAaQB0AGUAcgBhAGwAUABhAHQAaAAgACQAcwBkAGsAKQApADsAIABpAGYAKABUAGUAcwB0AC0AUABhAHQAaAAgAC0ATABpAHQAZQByAGEAbABQAGEAdABoACAAJABzAGQAawApACAAewAgAEcAZQB0AC0AQwBoAGkAbABkAEkAdABlAG0AIAAtAEwAaQB0AGUAcgBhAGwAUABhAHQAaAAgACQAcwBkAGsAIAAtAFIAZQBjAHUAcgBzAGUAIAAtAEYAaQBsAHQAZQByACAATQBhAGsAZQBBAHAAcAB4AC4AZQB4AGUAIAAtAEUAcgByAG8AcgBBAGMAdABpAG8AbgAgAEMAbwBuAHQAaQBuAHUAZQAgAHwAIABGAG8AcgBFAGEAYwBoAC0ATwBiAGoAZQBjAHQAIAB7ACAAVwByAGkAdABlAC0ATwB1AHQAcAB1AHQAIAAoACcATQBBAEsARQBBAFAAUABYAD0AJwArACQAXwAuAEYAdQBsAGwATgBhAG0AZQApACAAfQAgAH0AIAB9ADsAIABmAG8AcgBlAGEAYwBoACgAJABuAGEAbQBlACAAaQBuACAAQAAoACcAZgBlAHIAcgB5AHgALQB3AGkAbgBiAHUAaQBsAGQAJwAsACcAZgBlAHIAcgB5AHgALQB3AGkAbgAtAGIAdQBpAGwAZAAnACwAJwByAGUAbAAtADAAOQAwADUAJwApACkAIAB7ACAAJABiAGEAcwBlAD0ASgBvAGkAbgAtAFAAYQB0AGgAIAAkAGUAbgB2ADoAVQBTAEUAUgBQAFIATwBGAEkATABFACAAJABuAGEAbQBlADsAIABmAG8AcgBlAGEAYwBoACgAJABkACAAaQBuACAAQAAoACQAYgBhAHMAZQAsACgASgBvAGkAbgAtAFAAYQB0AGgAIAAkAGIAYQBzAGUAIAAnAG8AcgBjAGEALQBsAGkAdABlACcAKQApACkAIAB7ACAAaQBmACgAVABlAHMAdAAtAFAAYQB0AGgAIAAtAEwAaQB0AGUAcgBhAGwAUABhAHQAaAAgACQAZAApACAAewAgAFcAcgBpAHQAZQAtAE8AdQB0AHAAdQB0ACAAKAAnAEMASABFAEMASwBPAFUAVABfAEMAQQBOAEQASQBEAEEAVABFAD0AJwArACQAZAApADsAIABnAGkAdAAgAC0AQwAgACQAZAAgAHIAZQB2AC0AcABhAHIAcwBlACAALQAtAHMAaABvAHcALQB0AG8AcABsAGUAdgBlAGwAOwAgAFcAcgBpAHQAZQAtAE8AdQB0AHAAdQB0ACAAKAAnAFIATwBPAFQAXwBFAFgASQBUAD0AJwArACQATABBAFMAVABFAFgASQBUAEMATwBEAEUAKQA7ACAAZwBpAHQAIAAtAEMAIAAkAGQAIAByAGUAdgAtAHAAYQByAHMAZQAgAEgARQBBAEQAOwAgAFcAcgBpAHQAZQAtAE8AdQB0AHAAdQB0ACAAKAAnAEgARQBBAEQAXwBFAFgASQBUAD0AJwArACQATABBAFMAVABFAFgASQBUAEMATwBEAEUAKQAgAH0AIAB9ACAAfQA7ACAAVwByAGkAdABlAC0ATwB1AHQAcAB1AHQAIAAnAFcASQBOAEQATwBXAFMAXwBSAEUAQQBEAE8ATgBMAFkAXwBQAFIATwBCAEUAXwBGAEkATgBJAFMASABFAEQAJwA=
```

Binary checks: VS query exit 0 with an installation path; linker query exit 0 with an x64 link.exe path; SDK enumeration with an x64 MakeAppx path; git root and HEAD exit 0 at the nested checkout. A successful outer SSH exit does not erase failed git checks on parent directories.

## Captured output

Monitor mon_MC3N0JE1MHN7N17T, terminal bash_8:

```text
status: completed exit_code: 0
HOST=DESKTOP-1LAPJMP
VSWHERE_CANDIDATE=C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe EXISTS=True
C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools
VSWHERE_EXIT=0
C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Tools\MSVC\14.44.35207\bin\Hostx64\x64\link.exe
LINK_FIND_EXIT=0
SDK_CANDIDATE=C:\Program Files (x86)\Windows Kits\10\bin EXISTS=True
MAKEAPPX=C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\arm64\makeappx.exe
MAKEAPPX=C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64\makeappx.exe
MAKEAPPX=C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x86\makeappx.exe
VSWHERE_CANDIDATE=C:\Program Files\Microsoft Visual Studio\Installer\vswhere.exe EXISTS=False
SDK_CANDIDATE=C:\Program Files\Windows Kits\10\bin EXISTS=False
CHECKOUT_CANDIDATE=C:\Users\sook\ferryx-winbuild
ROOT_EXIT=128
HEAD_EXIT=128
CHECKOUT_CANDIDATE=C:\Users\sook\ferryx-winbuild\orca-lite
C:/Users/sook/ferryx-winbuild/orca-lite
ROOT_EXIT=0
fatal: not a git repository (or any of the parent directories): .git
fatal: not a git repository (or any of the parent directories): .git
e2a19066fe36f126d62ffecc952f3dc0b5f3258a
HEAD_EXIT=0
CHECKOUT_CANDIDATE=C:\Users\sook\ferryx-win-build
ROOT_EXIT=128
fatal: not a git repository (or any of the parent directories): .git
HEAD_EXIT=128
fatal: not a git repository (or any of the parent directories): .git
WINDOWS_READONLY_PROBE_FINISHED
```

## Verified conclusions

- Visual Studio 2022 Build Tools is installed at C:/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools.
- MSVC linker exists at VC/Tools/MSVC/14.44.35207/bin/Hostx64/x64/link.exe below that installation.
- Windows SDK 10.0.26100.0 provides x64 MakeAppx under C:/Program Files (x86)/Windows Kits/10/bin.
- The actual repository root is C:/Users/sook/ferryx-winbuild/orca-lite; HEAD is e2a19066fe36f126d62ffecc952f3dc0b5f3258a. The parent ferryx-winbuild and separate ferryx-win-build directories returned git exit 128; they are not the verified root.
- This corrects the main assessment's unverified MSVC/SDK/checkout limitation. It does not establish compiler execution, successful linking, packaging, signing, runtime rendering, or suitability of that old commit for a new release.
- Mac and omaki observations remain in the existing evidence; they were not repeated. Existing release tests remain 26 pass / 1 pre-existing failure.

## Cleanup and boundaries

The monitor completed with exit 0 and the completion sentinel was observed. No temporary script was written: the payload was encoded in memory. No remote files, checkouts, packages, credentials, apps, daemons or GitHub state were changed. All paths were constructed with Join-Path and forward-slash child paths; unlike prior probes, no JS backslash escapes corrupted them.
