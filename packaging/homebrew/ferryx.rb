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

  zap trash: [
    "~/Library/Application Support/com.ferryx.app",
    "~/Library/Caches/com.ferryx.app",
    "~/Library/Preferences/com.ferryx.app.plist",
    "~/Library/Saved Application State/com.ferryx.app.savedState",
    "~/Library/WebKit/com.ferryx.app",
  ]
end
