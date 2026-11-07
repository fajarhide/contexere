class Contexere < Formula
  desc "The context layer for AI coding agents: less noise, fewer tokens, nothing lost"
  homepage "https://github.com/fajarhide/contexere"
  version "0.8.6"
  license "Apache-2.0"

  on_macos do
    on_arm do
      url "https://github.com/fajarhide/contexere/releases/download/v#{version}/contexere-v#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "2c1fc3f87261af93da763f49a31ec2ba1d9f242edee2dc4c8769262c7c7fc60f"
    end
    on_intel do
      url "https://github.com/fajarhide/contexere/releases/download/v#{version}/contexere-v#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "c205e326c6db7fd9beb6e90961aefa3e8d66c720c1b16bc6f991c76d49cfa99e"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/fajarhide/contexere/releases/download/v#{version}/contexere-v#{version}-aarch64-unknown-linux-musl.tar.gz"
      sha256 "2e43276de07b79a694af86df746504374d44397f232c176475282bbd6b8e4367"
    end
    on_intel do
      url "https://github.com/fajarhide/contexere/releases/download/v#{version}/contexere-v#{version}-x86_64-unknown-linux-musl.tar.gz"
      sha256 "b52988f29e9e069dcfc9b5bd88e00e631b14aba28eee0eae84c98be8002e036e"
    end
  end

  def install
    bin.install "contexere"
    # Hooks and MCP entries registered before the rename call `omni` (#876).
    bin.install_symlink "contexere" => "omni"
  end

  def caveats
    <<~EOS
      Quick start:
        contexere init         # pick your agents (interactive)
        contexere doctor       # check the hooks fire and redact
        contexere stats        # what it saved on your own history

      `omni` is the same program, so existing hooks keep working.
    EOS
  end

  test do
    assert_match "omni", shell_output("#{bin}/omni version")
    assert_match "Signal Report", shell_output("#{bin}/omni stats 2>&1", 0)
    assert_match "OMNI Doctor", shell_output("#{bin}/omni doctor 2>&1", 0)
  end
end
