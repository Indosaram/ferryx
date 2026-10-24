# typed: strict
# frozen_string_literal: true

# Ferryx desktop app (universal DMG). Rendered per release by scripts/release-packaging.mjs.
cask "ferryx" do
  version "2026.10.24.2"
  sha256 "00cbbd0ddbda2762798f7009838ed34ca1f12b93965813c7df22943bc62166d1"

  url "https://github.com/Indosaram/ferryx/releases/download/v2026.10.24.2/Ferryx_universal.dmg"
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
