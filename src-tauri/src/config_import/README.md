# Terminal configuration import

`cmd_config_import` accepts at most 1 MiB of user-selected bytes and produces a
preview. It does not read referenced files, resolve external themes, execute
commands, or mutate preferences. The UI applies only the supported fields after
explicit confirmation using the existing persisted terminal settings path.
Onboarding completion uses the existing durable daemon onboarding store.

Ghostty accepts scalar assignments and explicit palette entries. The byte-based
`scrollback-limit` is skipped; `scrollback-limit-lines` maps to Ferryx lines.
iTerm2 accepts XML and binary plists, selecting the default profile (first profile
when no default matches). Other profiles and unsupported fields are reported.

WezTerm accepts a returned static table: literal strings, decimal numbers, nested
tables and line comments. Expressions, function calls (including `wezterm.font`),
variables and scripts are skipped as dynamic configuration, not evaluated. Use
`font = 'Menlo'` in a static export. Static input is evaluated in a fresh Luau VM
with an empty environment, no host bindings, 4 MiB allocation cap, 10 ms interrupt
budget and a post-extraction deadline check. Named color schemes are not loaded.

## Deferred integration verification

No builds, tests, mutation checks, or application QA were run on the owner's Mac,
per the implement-first instruction. Run the plan acceptance on integration:

```
cargo test --manifest-path src-tauri/Cargo.toml --lib config_import
cargo test --manifest-path src-tauri/Cargo.toml --lib config_import::ghostty_maps_font_theme_cursor
```

Also run the UI tests (ImportSection and onboarding) and compile all supported
platforms in the combined verification. Existing todo 23 introduces mlua and
cpu-time but its inherited Cargo.lock does not yet list them under ferryx; lock
resolution for those pre-existing dependencies belongs to the integration lane.

QA: on maho-mac open the debug app's Settings > Import, select Ghostty, choose a
fixture with font-family, background/foreground and cursor-style assignments.
Capture the preview, apply, and confirm appearance persists after reopening.
Choose an invalid iTerm2 plist: expect IMPORT_PARSE_ERROR and no settings changes.
Check optional Import follows Features in onboarding and Skip continues normally.
