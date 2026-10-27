class Omni < Formula
  desc "Semantic Signal Engine: less noise, more signal, the right signal"
  homepage "https://github.com/fajarhide/omni"
  version "0.7.12"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/fajarhide/omni/releases/download/v#{version}/veng-v#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "e7d716ed2327685a892aa4eaab983f14cbdf33b273cbab6d80e35eb5c19e7942"
    end
    on_intel do
      url "https://github.com/fajarhide/omni/releases/download/v#{version}/veng-v#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "9b9c753a7e4b75d5337dcdd6792cd29176c74a146dedacfaf494355074b37d54"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/fajarhide/omni/releases/download/v#{version}/veng-v#{version}-aarch64-unknown-linux-musl.tar.gz"
      sha256 "8596f8eeb648c1bf0d15eed9408f6d88ac94ba7f2cadc1f816b034d427f7fe04"
    end
    on_intel do
      url "https://github.com/fajarhide/omni/releases/download/v#{version}/veng-v#{version}-x86_64-unknown-linux-musl.tar.gz"
      sha256 "1bc45045331507cca9d3b740446c077cf36eca9309c1ddc9f3e76226c5a990fd"
    end
  end

  def install
    # The formula keeps its name through the rename so `brew upgrade` crosses it,
    # and `omni` stays as a link so existing hooks keep running (#876).
    bin.install "veng"
    bin.install_symlink "veng" => "omni"
  end

  def caveats
    <<~EOS
      Quick start:
        veng init              # Initialize VENG setup (interactive)
        veng doctor            # Verify installation
        veng stats             # View token savings

      OMNI is now VENG. `omni` still works and runs the same binary.
      Hooks intercept Claude Code tool outputs and distill them in real-time.
    EOS
  end

  test do
    assert_match "VENG", shell_output("#{bin}/veng version")
    assert_match "VENG", shell_output("#{bin}/omni version")
    assert_match "Signal Report", shell_output("#{bin}/veng stats 2>&1", 0)
    assert_match "VENG Doctor", shell_output("#{bin}/veng doctor 2>&1", 0)
  end
end
