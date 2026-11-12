cask "platypusdb" do
  arch arm: "aarch64", intel: "x64"

  version "@VERSION@"
  sha256 arm:   "@SHA256_MACOS_ARM@",
         intel: "@SHA256_MACOS_INTEL@"

  url "https://github.com/platypus-db/platypus-db/releases/download/v#{version}/PlatypusDB_#{version}_#{arch}.dmg"
  name "PlatypusDB"
  desc "Fast, lightweight SQL client"
  homepage "https://github.com/platypus-db/platypus-db"

  livecheck do
    url :url
    strategy :github_latest
  end

  auto_updates true
  depends_on macos: ">= :catalina"

  app "PlatypusDB.app"

  zap trash: [
    "~/Library/Application Support/dev.platypusdb.desktop",
    "~/Library/Caches/dev.platypusdb.desktop",
    "~/Library/Logs/dev.platypusdb.desktop",
    "~/Library/WebKit/dev.platypusdb.desktop",
  ]
end
