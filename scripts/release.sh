#!/bin/sh
# Publishes a GitHub release with the macOS app built and signed on this Mac, so the signing key never leaves it.
#   scripts/release.sh 0.2.0
# Sets the version in package.json, tauri.conf.json and Cargo.toml, commits, tags v<version>, builds the
# universal app, makes the zip, DMG, updater archive and latest.json, pushes, and uploads them with
# `gh release create`. Publishing the release starts .github/workflows/release.yml, which adds the
# Windows and Linux installers and their entries in latest.json.
# Installed copies find the release through the updater the next time they start.
set -e
cd "$(dirname "$0")/.."

VERSION="${1:?usage: scripts/release.sh <version, e.g. 0.2.0>}"
TAG="v$VERSION"
IDENTITY="${APPLE_SIGNING_IDENTITY:-Dictate Local Signing}"
TARGET="universal-apple-darwin"
REPO="wolfAC/dictate"
KEY="${TAURI_SIGNING_KEY_FILE:-$HOME/.tauri/dictate.key}"

# Releases must be signed with the same certificate every time, or users lose their permissions.
if ! security find-identity -p codesigning 2>/dev/null | grep -q "\"$IDENTITY\""; then
    echo "Signing certificate \"$IDENTITY\" not found in the keychain; refusing to publish an ad-hoc build." >&2
    exit 1
fi
# Updates are only accepted if signed with the key matching the pubkey in tauri.conf.json.
if [ ! -f "$KEY" ]; then
    echo "Updater signing key $KEY not found." >&2
    exit 1
fi
if [ "$(git rev-parse --abbrev-ref HEAD)" != "main" ]; then
    echo "Release from main." >&2
    exit 1
fi
if [ -n "$(git status --porcelain)" ]; then
    echo "Working tree has uncommitted changes; commit or stash them first." >&2
    exit 1
fi
if git rev-parse -q --verify "refs/tags/$TAG" >/dev/null; then
    echo "Tag $TAG already exists." >&2
    exit 1
fi

(cd src-tauri && cargo test --lib)

VERSION="$VERSION" node -e '
const fs = require("fs");
for (const file of ["package.json", "src-tauri/tauri.conf.json"]) {
  const json = JSON.parse(fs.readFileSync(file, "utf8"));
  json.version = process.env.VERSION;
  fs.writeFileSync(file, JSON.stringify(json, null, 2) + "\n");
}'
VERSION="$VERSION" perl -pi -e 's/^version = "[^"]*"/version = "$ENV{VERSION}"/ if !$done && /^version = / && ($done = 1)' src-tauri/Cargo.toml
(cd src-tauri && cargo update --workspace --offline -q)
if [ -n "$(git status --porcelain)" ]; then
    git add package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml src-tauri/Cargo.lock
    git commit -q -m "Release $TAG"
fi
git tag -a "$TAG" -m "Dictate $VERSION"

rustup target add aarch64-apple-darwin x86_64-apple-darwin
APPLE_SIGNING_IDENTITY="$IDENTITY" TAURI_SIGNING_PRIVATE_KEY="$(cat "$KEY")" TAURI_SIGNING_PRIVATE_KEY_PASSWORD="" \
    pnpm tauri build --target "$TARGET" --bundles app

BUNDLE="src-tauri/target/$TARGET/release/bundle/macos"
APP="$BUNDLE/Dictate.app"
mkdir -p build
rm -rf build/dmg build/Dictate.dmg build/Dictate.zip build/Dictate.app.tar.gz build/latest.json
mkdir build/dmg
cp -R "$APP" build/dmg/
ln -s /Applications build/dmg/Applications
hdiutil create -volname "Dictate" -srcfolder build/dmg -ov -format UDZO build/Dictate.dmg
rm -rf build/dmg
ditto -c -k --keepParent "$APP" build/Dictate.zip
cp "$BUNDLE/Dictate.app.tar.gz" build/Dictate.app.tar.gz

VERSION="$VERSION" REPO="$REPO" SIG="$(cat "$BUNDLE/Dictate.app.tar.gz.sig")" node -e '
const { VERSION, REPO, SIG } = process.env;
const mac = { signature: SIG, url: `https://github.com/${REPO}/releases/download/v${VERSION}/Dictate.app.tar.gz` };
const manifest = {
  version: VERSION,
  pub_date: new Date().toISOString(),
  platforms: { "darwin-aarch64": mac, "darwin-x86_64": mac },
};
require("fs").writeFileSync("build/latest.json", JSON.stringify(manifest, null, 2) + "\n");'

git push origin main "$TAG"
gh release create "$TAG" build/Dictate.dmg build/Dictate.zip build/Dictate.app.tar.gz build/latest.json \
    --repo "$REPO" \
    --title "Dictate $VERSION" \
    --generate-notes

echo
echo "Released $TAG. Windows and Linux installers are being added by GitHub Actions:"
echo "  gh run watch \$(gh run list --workflow release.yml --limit 1 --json databaseId -q '.[0].databaseId')"
echo "Homebrew cask: version \"$VERSION\", sha256 \"$(shasum -a 256 build/Dictate.zip | cut -d' ' -f1)\""
