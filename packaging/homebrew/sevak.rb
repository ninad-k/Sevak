cask "sevak" do
  version "{{version}}"
  sha256 "{{sha256:dmg}}"

  url "https://github.com/ninad-k/Sevak/releases/download/v#{version}/Sevak_#{version}_universal.dmg"
  name "Sevak"
  desc "Keyboard-first quick launcher"
  homepage "https://github.com/ninad-k/Sevak"

  livecheck do
    url :url
    strategy :github_latest
  end

  # Sevak updates itself (and asks first); Homebrew records the new version.
  auto_updates true
  depends_on macos: ">= :big_sur"

  app "Sevak.app"

  zap trash: [
    "~/Library/Application Support/sevak",
    "~/Library/Caches/com.ninad.sevak",
    "~/Library/Saved Application State/com.ninad.sevak.savedState",
    "~/Library/WebKit/com.ninad.sevak",
  ]

  caveats <<~EOS
    Sevak is not notarized by Apple yet. If macOS blocks the first launch, run:
      xattr -dr com.apple.quarantine "#{appdir}/Sevak.app"
    Sevak lives in the menu bar; press Option+Space to open it.
  EOS
end
