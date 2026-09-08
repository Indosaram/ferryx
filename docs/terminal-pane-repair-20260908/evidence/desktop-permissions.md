# Desktop verification prerequisite

Read-only probe:

```sh
osascript -e 'tell application "System Events" to tell process "ferryx" to get {name, position, size} of every window'
```

Exit 1:

```text
System Events got an error: osascript is not allowed assistive access. (-1728)
```

Independent installed-tool probe:

```sh
peekaboo permissions status --json
```

Exit 0 means the permission query succeeded, not that permissions are granted:

```json
{
  "success": true,
  "data": {
    "permissions": [
      { "name": "Screen Recording", "isRequired": true, "isGranted": false },
      { "name": "Accessibility", "isRequired": true, "isGranted": false }
    ],
    "source": "local"
  }
}
```

Neither native window geometry nor pixel/input interaction was verified.
`orca` is unavailable on PATH; installed `maho` exposes browser controls, not
an alternative native desktop validation channel. No application was launched,
modified, restarted, signed or terminated by these probes.
