class Contexere < Formula
  desc "The context layer for AI coding agents: less noise, fewer tokens, nothing lost"
  homepage "https://github.com/fajarhide/contexere"
  version "0.9.0"
  license "Apache-2.0"

  on_macos do
    on_arm do
      url "https://github.com/fajarhide/contexere/releases/download/v#{version}/contexere-v#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "ab42246f27cb5b39de02c1aefcdd087b5a5ca88c11981bf7eb971b48e886f1de"
    end
    on_intel do
      url "https://github.com/fajarhide/contexere/releases/download/v#{version}/contexere-v#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "64b06b2d11ade7231cff260c4b59b9569a218558730c8206872be8361ab88973"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/fajarhide/contexere/releases/download/v#{version}/contexere-v#{version}-aarch64-unknown-linux-musl.tar.gz"
      sha256 "05a9c43da378eb40d208a79667ce69c037dc363b134c5b9ceea9442401fa9979"
    end
    on_intel do
      url "https://github.com/fajarhide/contexere/releases/download/v#{version}/contexere-v#{version}-x86_64-unknown-linux-musl.tar.gz"
      sha256 "5683cd57fe20f902948e310df14d0bfee121c39932fab068ad6eea0ce018b4d7"
    end
  end

  def install
    bin.install "contexere"
  end

  def caveats
    <<~EOS
      Quick start:
        contexere init         # pick your agents (interactive)
        contexere doctor       # check the hooks fire and redact
        contexere stats        # what it saved on your own history

      0.9.0 no longer installs `omni`. Hooks that still call it stop working:
      run `contexere init` again.
    EOS
  end

  test do
    assert_match "Contexere", shell_output("#{bin}/contexere version")
    assert_match "Signal Report", shell_output("#{bin}/contexere stats 2>&1", 0)
    assert_match "Contexere Doctor", shell_output("#{bin}/contexere doctor 2>&1", 0)
    refute_path_exists bin/"omni"
  end
end
