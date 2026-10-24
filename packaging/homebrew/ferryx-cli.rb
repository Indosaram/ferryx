# typed: strict
# frozen_string_literal: true

# Ferryx headless CLI and daemon for linux x64. Rendered per release by scripts/release-packaging.mjs.
class FerryxCli < Formula
  desc "Headless terminal daemon and CLI for Ferryx"
  homepage "https://ferryx.dev/"
  url "https://github.com/Indosaram/ferryx/releases/download/v2026.09.08.1/ferryx-cli-linux-amd64"
  version "2026.09.08.1"
  sha256 "0000000000000000000000000000000000000000000000000000000000000000"
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
