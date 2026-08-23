# Fix Packet: P-favicon

- **Packet ID**: P-favicon
- **Finding**: F-bundle-02
- **Files changed**:
  - `ui/index.html`

## Description

Updated `ui/index.html` favicon link to point to the existing Ferryx icon asset `/src/assets/ferryx-icon.png` instead of the non-existent `/src/assets/ferryx-icon.svg` (which triggered 404 network requests during webview/browser initialization).

- **Old href**: `/src/assets/ferryx-icon.svg`
- **New href**: `/src/assets/ferryx-icon.png`
- **Target file verified**: `ui/src/assets/ferryx-icon.png` exists on disk (1024x1024 PNG image data).

## Verification

### RED Run
Command:
```bash
grep -n "ferryx-icon" ui/index.html && test -f ui/src/assets/ferryx-icon.svg || echo "RED: target file does not exist"
```
Output:
```
5:    <link rel="icon" type="image/svg+xml" href="/src/assets/ferryx-icon.svg" />
RED: target file does not exist
```

### GREEN Run
Command:
```bash
grep "ferryx-icon.png" ui/index.html && test -f ui/src/assets/ferryx-icon.png && echo "GREEN: favicon points to existing ferryx-icon.png"
```
Output:
```
    <link rel="icon" type="image/png" href="/src/assets/ferryx-icon.png" />
GREEN: favicon points to existing ferryx-icon.png
```

## Leftover Risk
None. The asset `/src/assets/ferryx-icon.png` exists on disk and is bundled/served properly. No external dependencies or branding invariants violated.
