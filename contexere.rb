class Contexere < Formula
  desc "The context layer for AI coding agents: less noise, fewer tokens, nothing lost"
  homepage "https://github.com/fajarhide/contexere"
  version "0.8.4"
  license "Apache-2.0"

  on_macos do
    on_arm do
      url "https://github.com/fajarhide/omni/releases/download/v#{version}/omni-v#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "e9bc00c6db741ffcbbd83366107483f2c9bd4a94e93299f0c6f850451ecd7183"
    end
    on_intel do
      url "https://github.com/fajarhide/omni/releases/download/v#{version}/omni-v#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "ddb0d0760bb09f9e4e89cc0ae55cc2ccca8db6d29f66519941cd4485f569e5ae"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/fajarhide/omni/releases/download/v#{version}/omni-v#{version}-aarch64-unknown-linux-musl.tar.gz"
      sha256 "3cc372ee8f2eac829cc311bc60c1ea46a6cc2ee291afa17a10dc3762777e0308"
    end
    on_intel do
      url "https://github.com/fajarhide/omni/releases/download/v#{version}/omni-v#{version}-x86_64-unknown-linux-musl.tar.gz"
      sha256 "51f323abecd0f3eed2e6b2d3a0aa3eb6f826855132a417d33165310c494dad62"
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
