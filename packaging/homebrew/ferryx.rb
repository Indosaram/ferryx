# typed: strict
# frozen_string_literal: true

# Ferryx desktop app (universal DMG). Rendered per release by scripts/release-packaging.mjs.
cask "ferryx" do
  version "2026.09.08.1"
  sha256 "0000000000000000000000000000000000000000000000000000000000000000"

  url "https://github.com/Indosaram/ferryx/releases/download/v2026.09.08.1/Ferryx_universal.dmg"
  name "Ferryx"
  desc "Workspace and AI agent launcher"
  homepage "https://ferryx.dev/"

  auto_updates true
  depends_on macos: ">= :catalina"

  app "Ferryx.app"

  # Removes the daemon's launchd jobs (every runtime generation, com.ferryx.daemon.<key>, and the
  # legacy com.ferryx.daemon / com.rorca.daemon) through the app's own CLI, which owns the label
  # naming. A running daemon is never stopped: only idle jobs are booted out, and a running job only
  # loses its plist, so open terminals survive until it exits. No `launchctl:` directive on purpose:
  # Homebrew's `launchctl remove` would stop the running daemon and end its terminals.
  # Through /bin/sh so an app the user already deleted by hand does not fail the uninstall.
  uninstall early_script: {
    executable:   "/bin/sh",
    args:         [
      "-c",
      'cli="$1/Ferryx.app/Contents/MacOS/ferryx-cli"; [ -x "$cli" ] && "$cli" service uninstall; exit 0',
      "sh",
      appdir.to_s,
    ],
    must_succeed: false,
  }

  zap trash: [
    "~/Library/LaunchAgents/com.ferryx.daemon.*.plist",
    "~/Library/LaunchAgents/com.ferryx.daemon.plist",
    "~/Library/LaunchAgents/com.rorca.daemon.plist",
    "~/Library/Application Support/com.ferryx.app",
    "~/Library/Caches/com.ferryx.app",
    "~/Library/Preferences/com.ferryx.app.plist",
    "~/Library/Saved Application State/com.ferryx.app.savedState",
    "~/Library/WebKit/com.ferryx.app",
  ]
end
