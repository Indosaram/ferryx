# Ferryx v2026.08.30.2

Built locally from `release/v2026.08.30.2`. No CI artifacts are used.

## Windows rendering and startup

- Keeps the Windows main window and application chrome opaque while retaining macOS native-terminal transparency.
- Removes the macOS application menu from Windows.
- Makes clean `bun tauri dev` checkouts prepare bundled web assets before launch.
- Owns the Vite server lifecycle and serves the frontend from the correct `ui/` root.
- Resolves Tailwind from the `ui/` workspace so layout and theme utilities such as `h-screen`, `w-screen`, and `bg-background` are present in the live WebView stylesheet.
- Adds per-terminal shell selection for Default, PowerShell, Windows PowerShell, Command Prompt, and WSL.

## Verification

- Real Vite-server regression test fetches `/src/index.css` and confirms required Ferryx Tailwind utilities.
- Windows opacity, menu, clean-dev resource, native-terminal platform transparency, shell selection, and edge-probe contracts are included.
- UI production build and platform release bundling are executed from clean release trees.
