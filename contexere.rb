class Contexere < Formula
  desc "The context layer for AI coding agents: less noise, fewer tokens, nothing lost"
  homepage "https://github.com/fajarhide/contexere"
  version "0.8.5"
  license "Apache-2.0"

  on_macos do
    on_arm do
      url "https://github.com/fajarhide/omni/releases/download/v#{version}/omni-v#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "bc1af55184f0634dbe57bbb9fd1461674503e8af0c26108ba2f0882cd8b1e9eb"
    end
    on_intel do
      url "https://github.com/fajarhide/omni/releases/download/v#{version}/omni-v#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "88c31d2c2329a9673b7abda9e76c1dad89c495cadb48bb512bd1bd160c23eb22"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/fajarhide/omni/releases/download/v#{version}/omni-v#{version}-aarch64-unknown-linux-musl.tar.gz"
      sha256 "79d54774031b365c8581a87c309458de29d173b3234b1304e099906cd8d9e171"
    end
    on_intel do
      url "https://github.com/fajarhide/omni/releases/download/v#{version}/omni-v#{version}-x86_64-unknown-linux-musl.tar.gz"
      sha256 "f5fb404719f6edff60ade6be52fa1343a4a56408d66f9e1043198e91fddc524f"
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
