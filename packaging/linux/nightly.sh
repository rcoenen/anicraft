#!/usr/bin/env bash
# App-owned GUI-only nightly tar: no FUSE/AppImage, CLI, association or install hooks.
#
#   $DIST/anicraft-<version>-linux-x86_64-nightly.tar.gz
#
# Usage: packaging/linux/nightly.sh --arch x86_64
#
# The optional distribution formats stay the stable scripts' (packaging/linux/package.sh,
# flatpak-oci.sh), which take ANICRAFT_BUILD_CHANNEL for a co-installable nightly package.
set -euo pipefail
ARCH=
while [[ $# -gt 0 ]]; do
  case "$1" in
    --arch) [[ $# -ge 2 ]] || { echo 'missing --arch value' >&2; exit 2; }; ARCH="$2"; shift 2 ;;
    -h|--help) sed -n '2,10p' "$0"; exit 0 ;;
    *) echo "unsupported nightly argument: $1" >&2; exit 2 ;;
  esac
done
[[ "$ARCH" == x86_64 && "$(uname -s)" == Linux ]] || { echo 'only native Linux x86_64 nightly packaging is supported' >&2; exit 2; }
. "$(dirname "${BASH_SOURCE[0]}")/../nightly-common.sh"
nightly_init linux "$ARCH" x86_64-unknown-linux-gnu
OUTPUT="$DIST/anicraft-$VERSION-linux-x86_64-nightly.tar.gz"
[[ ! -e "$OUTPUT" && ! -L "$OUTPUT" ]] || nightly_die 'refusing to replace an existing nightly archive'
nightly_refuse_signing_env
nightly_build_gui
STAGE="$NIGHTLY_WORK/stage"
mkdir -p "$STAGE"
# The standard Nightly tar layout: `anicraft` is the launcher (the app's own name) and
# `anicraft-bin` the GUI it starts, so an extracted tar is isolated exactly as the packages are and
# the shared smoke can query the entry point by the app's name.
install -m755 "$NIGHTLY_GUI" "$STAGE/anicraft-bin"
"${CROSS_COMPILE:-}strip" "$STAGE/anicraft-bin" 2>/dev/null || true
# The same launcher the deb/rpm, AppImage, Arch and Flatpak nightly formats install.
install -m755 "$ROOT/packaging/linux/nightly-launch.sh" "$STAGE/anicraft"
cp "$ROOT/assets/app-icon/anicraft-nightly.png" "$STAGE/anicraft-nightly.png"
nightly_copy_notices "$STAGE"
# The staged launcher answers the Launcher's version query from the staged binary, exactly as an
# extracted copy does (no profile is created by a query).
[[ "$("$STAGE/anicraft" --version)" == "AniCraft $VERSION (nightly-v1)" ]] ||
  nightly_die 'the archive launcher does not answer the prepared nightly version'
# The private mktemp cwd is the helper authority root, not a CLI path parent.
(cd "$NIGHTLY_WORK" && python3 "$ROOT/packaging/nightly-archive.py" linux stage archive.tar.gz)
nightly_publish_archive "$NIGHTLY_WORK/archive.tar.gz" "$OUTPUT"
