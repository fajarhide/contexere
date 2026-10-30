class Contexere < Formula
  desc "The context layer for AI coding agents: less noise, fewer tokens, nothing lost"
  homepage "https://github.com/fajarhide/contexere"
  version "0.8.2"
  license "Apache-2.0"

  on_macos do
    on_arm do
      url "https://github.com/fajarhide/omni/releases/download/v#{version}/omni-v#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "cc5c7877bacb9cda078300af5573b4ff7f3dd806d233e0ea06b3ea75bbdd493a"
    end
    on_intel do
      url "https://github.com/fajarhide/omni/releases/download/v#{version}/omni-v#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "4625d2d0fc65ddeabf651c7a9308a1e5fa4f711cb8d7ecc2526e78ec3e4f08bd"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/fajarhide/omni/releases/download/v#{version}/omni-v#{version}-aarch64-unknown-linux-musl.tar.gz"
      sha256 "6c52f9b4e9b5423ba8f117f49ab669780782cdec4e8fa2fda45b685412336706"
    end
    on_intel do
      url "https://github.com/fajarhide/omni/releases/download/v#{version}/omni-v#{version}-x86_64-unknown-linux-musl.tar.gz"
      sha256 "f28638d9ae31fe98dcd3e5056c50ed02120aa8778e832b2efad65c71871e7f68"
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
