# Installed Windows compositor: binary evidence

Date: 2026-09-12. Task: st_01a09651. Static, local, read-only PE inspection.

## Conclusion

**The supplied installed executable contains the pre-3fa25a19-style root-HWND
compositor, including an actual call that supplies `layer_backed=false` to the
rejecting composition validator. It is not the real child-HWND implementation
introduced by 3fa25a1942ebb365515f8f8854a765c356bdf154.** This conclusion rests on
constructor instructions, caller-supplied descriptor bytes, validator control
flow, and a referenced error string together, not timestamps or missing strings.

This establishes implementation identity/behavior, **not the exact source commit
of the whole executable, nor the captured runtime rejection of PID 17288**.
If bounds reaches new surface-host creation with a valid Win32 handle, this
binary rejects composition before renderer/surface creation. Earlier failures
can still produce the same historical bare UI banner.

## Artifact identity and constraints

- Input: `/tmp/ferryx-installed-51ab67ee.exe`, opened only for reading.
- Measured size: **35,909,632 bytes**.
- Measured SHA-256:
  `51AB67EE9064D2B2AF7F52267FC8D887F3C0CA61ED816CC2D998B91A18B21BF7`.
- `file`: PE32+ GUI executable, x86-64, Windows. LLVM `objdump` identifies
  `coff-x86-64`; preferred image base is `0x140000000`.
- Addresses below are preferred-image virtual addresses (VA), not live ASLR
  addresses. Subtract `0x140000000` for RVA.
- `.text`: RVA `0x1000`, file offset `0x400`; `.rdata`: RVA `0x168d000`,
  file offset `0x168c000`. `.pdata`: RVA `0x20ec000`, file offset `0x20e7400`.
- PE timestamp prints `Fri Aug 28 20:02:29 2026`; no source attribution relies
  on this or the task-supplied installed file mtime.

Task-supplied runtime context, not re-probed here: installed app PID 17288 has
the bare bounds error; daemon PID 20196 protocol 2 confirms both Strawberry PTYs
`aeb0bde0-188d-4ee4-b024-d4e9f7ddc7f9` and
`28ff42bb-b9cf-4a51-8902-dd2c35967565` alive. Browser CLI list is empty and no CDP
inspection connection is available. Live PTYs do not validate native composition.

## Positive binary evidence

Function names below are source-correlated interpretations, not recovered symbols.
Function ranges for constructor and validator were independently located in PE
`.pdata` RUNTIME_FUNCTION entries.

### 1. Constructor retains the supplied handle, rather than creating a child

Constructor range: **`0x140652110..0x14065225f`** (exclusive end).

```text
14065211d  lea    rcx,[rsp+30h]       ; handle result destination
140652122  call   140614cb0          ; window-handle acquisition helper
140652127  mov    eax,[rsp+30h]
14065212b  cmp    eax,-1             ; handle acquisition error branch
140652130  cmp    eax,9              ; Win32 raw-handle variant
140652133  jne    1406521da          ; Expected Win32... error
140652139  movups xmm6,[rsp+38h]     ; returned 16-byte handle pair
140652143  mov    ecx,20h           ; allocate 32 bytes, alignment 8
14065214d  call   1400e5260
14065215b  mov    qword [rax],1      ; Arc strong count
140652162  mov    qword [rax+8],1    ; Arc weak count
14065216a  movups [rax+10h],xmm6     ; retain SAME pair, no child replacement
14065216e  mov    [rsi+8],rax
140652172  mov    word [rsi],0ffffh  ; success sentinel
```

The full bounded function was disassembled: the successful handle branch has
only allocation between handle acquisition and storing the pair; no window
registration, CreateWindowEx, visibility initialization, or replacement HWND.
The error branch at `0x1406521fe` copies bytes from `0x141902480`, the ASCII
`Expected Win32 window handle on Windows` string (file offset `0x1901480`).
The other error path formats the `Failed to get window handle: ` message through
the formatting object at `0x141905488` (text starts at `0x141905489`).
This matches the old constructor's handle extraction and `Arc<NativeChildViewHandle>`.
The display-handle validity check in historical source need not survive optimization.

### 2. The actual constructor caller supplies false/false/WindowsChildWindow

The direct constructor call was found at **`0x140472a59`** and confirmed by
disassembly. On its success sentinel, the caller branches to `0x140472b71`:

```text
140472b7f  mov byte [rbp+0c87h],2
140472b86  mov word [rbp+0c85h],0
140472b8f  lea rcx,[rbp+600h]        ; validator result
140472b96  lea rdx,[rbp+0c85h]       ; descriptor bytes 00 00 02
140472b9d  call 1406045f0
140472bab  cmp r13w,-1              ; success sentinel check
140472bb0  je 140472da5             ; only success continues
140472bda  jmp 140473372            ; propagate failure path
```

This is a real caller passing constants, not merely a dormant generic validator.
Descriptor offsets are proven by the validator below: offset 0 pointer
transparency, offset 1 layer backing, offset 2 kind. The kind value 2 corresponds
to WindowsChildWindow in both historical source and the binary's jump table.

### 3. Validator deterministically rejects those exact bytes

Validator range: **`0x1406045f0..0x140604aee`** (exclusive end).

```text
1406045f8  movzx eax,byte [rdx+2]    ; kind
1406045fc  lea rcx,[1418f7ce4]       ; signed rel32 jump table
140604603  movsxd rax,dword [rcx+rax*4]
140604607  add rax,rcx
14060460a  jmp rax
```

Decoded table entries 0..4 at `0x1418f7ce4`:
`0x14060460c`, `0x1406047a6`, **`0x14060482e`**, `0x1406046fd`, `0x140604697`.
Thus kind 2 executes:

```text
14060482e  cmp byte [rdx+1],0       ; layer_backed
140604832  je 140604942             ; TAKEN for caller's zero byte
140604838  cmp byte [rdx],0         ; pointer_transparent, not reached
140604942  call 14007de10
140604947  mov ecx,66h              ; 102-byte error string allocation
140604951  call 1400e5260
...                                 ; copies full message in vector chunks
140604996  movups xmm0,[1418f41ab]   ; first 16 message bytes
14060499d  movups [rax],xmm0
1406049ae  mov word [rsi],0dh        ; error discriminant, not ffff success
1406049b3  mov qword [rsi+8],66h
1406049bb  mov [rsi+10h],rax
1406049bf  mov qword [rsi+18h],66h
1406049cf  ret
```

At file offset **`0x18f31ab`**, VA **`0x1418f41ab`**, the exact 102 ASCII bytes are:

> Windows native terminal composition target is not layer-backed (child window clipping not established)

The generic function's message alone would not prove use. The caller's constant
descriptor and the above taken branch establish that use statically.

## Historical source comparison and symptom chain

Specimens read directly with `git show`, not asserted as exact installed revision:

- `3fa25a19^:src-tauri/src/native_terminal/platform/windows.rs`: `new` obtains
  the parent window's Win32 HWND/HINSTANCE, stores them in an Arc;
  `descriptor` returns WindowsChildWindow, false, false; viewport/reveal are no-ops.
- `3fa25a19^:src-tauri/src/native_terminal/composition.rs:202-213,271-323`:
  kind enum and validator, including the exact Windows error above and checking
  layer backing before pointer transparency. These agree with the binary branches.
- `3fa25a19^:src-tauri/src/native_terminal/surface_host.rs:1013-1035,1095-1117`:
  render lazily inserts a host; host construction creates the platform target,
  validates its descriptor, then creates the renderer and GPU surface. This
  explains why working PTYs cannot prevent this first-bounds rejection.
- `3fa25a19:src-tauri/src/native_terminal/platform/windows.rs`: replaces that
  constructor with RegisterClassExW/`FerryxNativeTerm`, CreateWindowExW using
  extended style `0x08000020`, style `0x44000000`, parent HWND and initial 1x1
  size; stores the returned child HWND; descriptor is true/true; viewport/reveal
  use SetWindowPos/ShowWindow; Drop destroys the child. The inspected constructor
  and descriptor call site do not implement this.

The earlier source/UI/IPC chain and bare-banner information loss are documented
in `installed-build-diagnosis.md`; no window-lookup historical survey was repeated.

## Corroboration, pitfalls, and tooling limits

- PE imports include CreateWindowExW, RegisterClassExW, SetWindowPos, ShowWindow,
  DestroyWindow, plus CreateWindowExA/RegisterClassExA. **Import presence is not
  child-compositor evidence**: the app/framework can use these independently.
- Whole-file exact byte searches in UTF-8 and UTF-16LE found neither
  `FerryxNativeTerm` nor `Failed to create native terminal child window`.
  These absences corroborate the positive instructions; neither is used alone.
- A first LEA-only reference search missed the validator string because LLVM
  copies it with `movups`. A broad displacement scan then produced candidates;
  actual disassembly confirmed `0x140604996` and rejected the apparent hit at
  `0x14120324d` as an instruction-interior false positive. Only decoded references
  are evidence above.
- Native-binary, Ghidra tool, and bundled-app debugging references were read.
  Ghidra/analyzeHeadless were not on PATH or in the checked standard Homebrew /
  Applications locations; Python pefile/capstone/lief were unavailable. No tool
  installation was necessary: built-in LLVM objdump plus Python standard-library
  PE byte parsing resolved this narrow question. No full-image decompilation.
- No live process memory, raw IPC rejection, symbols/PDB, or exact release-build
  source mapping was obtained. Static evidence cannot establish the failing
  process's actual branch history.

## Reproduction and next discriminating probe

Read-only local commands (no execution of the PE):

```sh
shasum -a 256 /tmp/ferryx-installed-51ab67ee.exe
stat -f '%z bytes' /tmp/ferryx-installed-51ab67ee.exe
objdump -h /tmp/ferryx-installed-51ab67ee.exe
objdump -p /tmp/ferryx-installed-51ab67ee.exe
objdump -d --start-address=0x140652110 --stop-address=0x14065225f /tmp/ferryx-installed-51ab67ee.exe
objdump -d --start-address=0x140472a4f --stop-address=0x140472bdf /tmp/ferryx-installed-51ab67ee.exe
objdump -d --start-address=0x1406045f0 --stop-address=0x140604aee /tmp/ferryx-installed-51ab67ee.exe
```

**No further binary probe is needed to distinguish these two implementations.**
The next discriminator for the *live incident* is the already-retained raw
`cmd_native_terminal_set_bounds` rejection from the installed WebView's console,
if it can be exposed without reload/retry/resize or another command invocation.
An error message containing the exact 102-byte layer-backed rejection above
selects this proven binary defect; a different message selects an earlier or
different failure despite this defect being present. Preserve raw code/message/
details rather than `String(error)` or the banner. The current blocker is concrete:
there is no available CDP inspection connection and Browser CLI list is empty;
those interfaces do not currently expose that retained rejection. Do not infer
it from PTY liveness or substitute a successful QA app's console.

## Verification and cleanup

Hash/size, source specimens, imports, function ranges, descriptor bytes, jump
table and bounded disassembly were inspected in this task. No production edits,
builds, tests, executable launches, remote processes, desktop interactions, or
live debugger attachments were performed. This report is the only created file,
added with `apply_patch`. Analysis scripts ran inline and wrote no local artifacts.
The pre-existing input `/tmp/ferryx-installed-51ab67ee.exe` remains untouched;
its cleanup belongs to the parent task after evidence retention. No temporary
analysis artifact paths require cleanup.
