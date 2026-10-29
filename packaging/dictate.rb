# Homebrew cask for your own tap (e.g. github.com/<you>/homebrew-tap, file Casks/dictate.rb).
# Update version + sha256 for each release (scripts/release.sh prints both):
#   shasum -a 256 build/Dictate.zip
cask "dictate" do
  version "0.1.0"
  sha256 :no_check

  url "https://github.com/wolfAC/dictate/releases/download/v#{version}/Dictate.zip"
  name "Dictate"
  desc "Offline push-to-talk dictation"
  homepage "https://github.com/wolfAC/dictate"

  depends_on macos: ">= :catalina"

  app "Dictate.app"

  # Not notarized: remove the quarantine flag so Gatekeeper doesn't block it.
  postflight do
    system_command "/usr/bin/xattr", args: ["-dr", "com.apple.quarantine", "#{appdir}/Dictate.app"]
  end

  zap trash: "~/Library/Application Support/dev.dictate.desktop"
end
