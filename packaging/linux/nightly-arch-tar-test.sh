#!/usr/bin/env bash
# Validate the staged Arch handoff without building or opening AniCraft.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
stage="$work/source"
dst="$work/dist"
base=anicraft-0.0.1-nightly.1-linux-x86_64
mkdir -p "$stage/bin" "$dst" "$work/extract" "$stage/share/applications" "$stage/share/metainfo"

# A local argv recorder stands in for the compiled binaries. It never opens a UI.
cat >"$stage/bin/anicraft" <<'RECORDER'
#!/bin/sh
for arg in "$@"; do
  printf '<%s>\n' "$arg"
done
RECORDER
chmod 755 "$stage/bin/anicraft"
cp "$stage/bin/anicraft" "$stage/bin/anicraft-cli"
# The stage package.sh builds, as far as the handoff reads it.
cp "$HERE/ai.storyteller.anicraft.desktop" "$stage/share/applications/ai.storyteller.anicraft.desktop"
sed -e 's/@VERSION@/0.0.1/' -e 's/@DATE@/2026-11-24/' \
  "$HERE/ai.storyteller.anicraft.metainfo.xml.in" >"$stage/share/metainfo/ai.storyteller.anicraft.metainfo.xml"
mkdir -p "$stage/share/mime/packages"
cp "$HERE/ai.storyteller.anicraft.mime.xml" "$stage/share/mime/packages/ai.storyteller.anicraft.xml"

bash "$HERE/nightly-arch-tar.sh" "$stage" "$work/stage" "$dst" "$base"
tar -xzf "$dst/$base.tar.gz" -C "$work/extract"
pack="$work/extract/$base"
# The launcher is the shared one, byte for byte, and the real program hides beside it.
cmp "$HERE/nightly-launch.sh" "$pack/bin/anicraft"
bash -n "$pack/bin/anicraft"
# Simulate the existing Arch bin-tree copy and public launcher rename.
mkdir -p "$work/installed"
cp -R "$pack/bin" "$work/installed/bin"
bin="$work/installed/bin"
mv "$bin/anicraft" "$bin/anicraft-nightly"
mv "$bin/anicraft-cli" "$bin/anicraft-nightly-cli"
cmp "$stage/bin/anicraft" "$bin/.anicraft-nightly/anicraft"
cmp "$stage/bin/anicraft-cli" "$bin/anicraft-nightly-cli"

# No stable desktop entry, AppStream id, icon or MIME package can reach an Arch Nightly install.
apps="$pack/share/applications"
nightly_id=ai.storyteller.anicraft.nightly-v1
[[ -f "$apps/$nightly_id.desktop" && ! -e "$apps/ai.storyteller.anicraft.desktop" ]]
for line in 'Name=AniCraft Nightly' 'Exec=anicraft-nightly %f' 'TryExec=anicraft-nightly' \
  "Icon=$nightly_id" "StartupWMClass=$nightly_id"; do
  grep -qxF "$line" "$apps/$nightly_id.desktop" || { echo "missing desktop line: $line" >&2; exit 1; }
done
# `! grep` is not a condition: errexit skips it, so a match would pass. Fail explicitly.
if grep -q '^MimeType=' "$apps/$nightly_id.desktop"; then
  echo 'FAIL: the Nightly desktop entry still declares MimeType' >&2
  exit 1
fi
meta="$pack/share/metainfo/$nightly_id.metainfo.xml"
[[ -f "$meta" && ! -e "$pack/share/metainfo/ai.storyteller.anicraft.metainfo.xml" ]]
grep -qF "<id>$nightly_id</id>" "$meta"
grep -qF '<name>AniCraft Nightly</name>' "$meta"
grep -qF '<binary>anicraft-nightly</binary>' "$meta"
if grep -qF '<mediatype>' "$meta"; then
  echo 'FAIL: the Nightly AppStream file still declares a mediatype' >&2
  exit 1
fi
[[ ! -e "$pack/share/mime" ]]
[[ -f "$pack/share/icons/hicolor/256x256/apps/$nightly_id.png" ]]

export HOME="$work/home with space"
unset XDG_DATA_HOME
expect() {
  expected="$1"
  shift
  actual=$(PATH="$bin:$PATH" anicraft-nightly "$@")
  if [[ "$actual" != "$expected" ]]; then
    printf 'expected:\n%s\nactual:\n%s\n' "$expected" "$actual" >&2
    exit 1
  fi
}
profile="$HOME/.local/share/anicraft-nightly/profile"
expect "$(printf '<%s>\n' --launch-profile nightly-v1 --profile-root "$profile" --)"  # no arguments
expect "$(printf '<%s>\n' --launch-profile nightly-v1 --profile-root "$profile" -- 'scene with spaces.anicraft' '--other-name.anicraft')" \
  'scene with spaces.anicraft' '--other-name.anicraft'
expect "$(printf '<%s>\n' --launch-profile nightly-v1 --profile-root "$profile" --version)" --version
expect "$(printf '<%s>\n' --launch-profile nightly-v1 --profile-root "$profile" -V)" -V
export XDG_DATA_HOME="$work/xdg with space"
profile="$XDG_DATA_HOME/anicraft-nightly/profile"
expect "$(printf '<%s>\n' --launch-profile nightly-v1 --profile-root "$profile" -- 'scene with spaces.anicraft')" 'scene with spaces.anicraft'
# The Launcher's own command line is passed through, not wrapped a second time.
expect "$(printf '<%s>\n' --launch-profile nightly-v1 --profile-root "$work/launcher-profile" -- 'scene.anicraft')" \
  --launch-profile nightly-v1 --profile-root "$work/launcher-profile" -- 'scene.anicraft'

# The handoff can be unpacked under a folder another local user may write, so a program planted in
# a sibling lib folder is never run in place of the packaged one.
planted="$work/planted"
mkdir -p "$planted/lib/anicraft-nightly" "$planted/bin/.anicraft-nightly"
cp "$pack/bin/anicraft" "$planted/bin/anicraft"
cp "$stage/bin/anicraft" "$planted/bin/.anicraft-nightly/anicraft"
printf '#!/bin/sh\necho PLANTED-BY-ANOTHER-USER\n' >"$planted/lib/anicraft-nightly/anicraft"
chmod 755 "$planted/lib/anicraft-nightly/anicraft"
[[ "$("$planted/bin/anicraft" walk.anicraft)" != PLANTED* ]]

echo "PASS: Arch Nightly handoff starts through the shared launcher in its own profile"
