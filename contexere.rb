class Contexere < Formula
  desc "The context layer for AI coding agents: less noise, fewer tokens, nothing lost"
  homepage "https://github.com/fajarhide/contexere"
  version "0.8.7"
  license "Apache-2.0"

  on_macos do
    on_arm do
      url "https://github.com/fajarhide/contexere/releases/download/v#{version}/contexere-v#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "7f6a527ac82e9e9f5b2d406c808d0fcaa9c0c311fb7b2291b30a5aa4595e104d"
    end
    on_intel do
      url "https://github.com/fajarhide/contexere/releases/download/v#{version}/contexere-v#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "1121405d315b9396956f9215bda6f9313336d232c2eaefc2c4caf7ba30bc4983"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/fajarhide/contexere/releases/download/v#{version}/contexere-v#{version}-aarch64-unknown-linux-musl.tar.gz"
      sha256 "8e63b527b194dc0317a9cc0770dcbc7b6be0b3dbd6f380dbc73123db82d72ec8"
    end
    on_intel do
      url "https://github.com/fajarhide/contexere/releases/download/v#{version}/contexere-v#{version}-x86_64-unknown-linux-musl.tar.gz"
      sha256 "419ae909fd086df7c94367a5473d22eaf260c7282c53d99767181882448090d1"
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
