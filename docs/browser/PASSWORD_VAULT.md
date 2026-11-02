# Password vault

Ferryx's built-in browser can save, autofill, list and generate passwords. Saved entries are
encrypted with a per-vault key; the key itself never sits in the vault file.

## Where the key lives

| OS | Key storage |
| --- | --- |
| macOS | The login Keychain. |
| Windows | A DPAPI-protected file, `key-<id>.dpapi`, beside the vault store. Only the same Windows account on the same machine can unprotect it. |
| Linux | The Secret Service (GNOME Keyring, KWallet, and so on). |

On Linux the Secret Service may ask the user to unlock the keyring. Ferryx waits at most 120
seconds for an answer (`PROMPT_TIMEOUT` in `src-tauri/src/browser/vault/keystore/linux.rs`),
then dismisses the prompt and the operation fails instead of hanging. The macOS Keychain
prompt has no Ferryx-side limit.

Opening the vault (the first operation that needs the key) runs on a blocking thread under
its own `opening` lock. The store and pending save-prompt locks are not held while the
keystore call runs, so closing a tab and reporting the vault status never wait on a keystore
prompt. Operations that need the open store (list, fill, save, reveal) wait for the open to
finish, on a blocking thread, never on an async worker.

## Writes

The vault store and the Windows key file are written atomically: the new content goes to a
temporary file, is fsynced, and is renamed over the old file; on macOS and Linux the directory
is then fsynced so the rename itself is durable. A crash leaves either the old or the new file,
never a half-written one.

## Re-authentication before reveal or copy

Showing or copying a saved password asks the OS to confirm the user first, every time:

- macOS: LocalAuthentication with the device-owner policy (Touch ID, Apple Watch or the
  login password).
- Windows: Windows Hello (face, fingerprint or PIN). If Hello is not set up, the request is
  refused.
- Linux: polkit, using Ferryx's own action `com.ferryx.app.vault-reveal`. Its default for an
  active session is `auth_self` (the user's own password, not an administrator's, and not
  remembered between reveals). The .deb and .rpm packages install the policy at
  `/usr/share/polkit-1/actions/com.ferryx.app.vault-reveal.policy` (source:
  `src-tauri/packaging/linux/com.ferryx.app.vault-reveal.policy`).

AppImage and source builds cannot install a system polkit action. Without the action, reveal
and copy are refused (fail closed) until an administrator copies the policy file into
`/usr/share/polkit-1/actions/`. Autofill and saving passwords still work without it.

## Relation to the plan

Plan `.omo/plans/ferryx-daily-browser.md`, todo 9 (port the Maho vault), specifies the
keychain-wrapped key, the save prompt, autofill, the list UI and the generator. It says nothing
about re-authentication for reveal; the re-authentication design above is Ferryx's addition.
