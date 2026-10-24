# typed: strict
# frozen_string_literal: true

# Ferryx headless CLI and daemon for linux x64. Rendered per release by scripts/release-packaging.mjs.
class FerryxCli < Formula
  desc "Headless terminal daemon and CLI for Ferryx"
  homepage "https://ferryx.dev/"
  url "https://github.com/Indosaram/ferryx/releases/download/v2026.10.24.2/ferryx-cli-linux-amd64"
  version "2026.10.24.2"
  sha256 "aac1cd68d0e2760b83e90214da039873232d1b0912669816d29252dce71490d7"
  license :cannot_represent

  depends_on arch: :x86_64
  depends_on :linux

  def install
    bin.install "ferryx-cli-linux-amd64" => "ferryx-cli"
  end

  test do
    assert_path_exists bin/"ferryx-cli"
  end
end
