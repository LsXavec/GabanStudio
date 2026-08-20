#!/usr/bin/env bash
# THE SHIPPING ROOM's release path (research/PSD-shipping.md).
# Usage: tools/release.sh v0.1.0 [owner/repo]
# Order matters: the installer EMBEDS the release exe at build time.
set -euo pipefail
TAG="${1:?usage: tools/release.sh vX.Y.Z [owner/repo]}"
REPO="${2:-}"
cargo test -p animstudio -p anim-core
cargo build --release -p animstudio
cargo build --release -p animstudio-installer
echo "built: target/release/animstudio.exe + AnimStudio-Setup.exe"
if [ -n "$REPO" ]; then
  # The updater looks for an asset named animstudio*.exe (not *setup*);
  # testers download AnimStudio-Setup.exe.
  gh release create "$TAG" \
    --repo "$REPO" \
    --title "Gaban Studio $TAG" \
    --notes "Draft build for testers." \
    "target/release/animstudio.exe" \
    "target/release/AnimStudio-Setup.exe" \n    "target/release/GabanStudio-portable.zip"
  echo "published $TAG to $REPO"
else
  echo "no repo given — binaries built, nothing published"
fi
