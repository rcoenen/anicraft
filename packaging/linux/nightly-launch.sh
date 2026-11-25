#!/bin/sh
# AniCraft Nightly's launcher: one private profile and the flavor's launch protocol, for every
# Linux nightly format. The real program never starts without
#
#   --launch-profile nightly-v1 --profile-root <this profile> -- <the person's arguments>
#
# so nothing can move Nightly's preferences, Recovery, Logs, plug-ins, Templates, Commands,
# themes, thumbnails and guide cache (src/platform.rs) into stable's folders. The profile is a
# sibling of stable's data folder, never inside it and never containing it (src/profile_root.rs).
#
# The same file is installed as:
#   deb/rpm   /usr/bin/anicraft-nightly   (the real program in /usr/lib/anicraft-nightly/)
#   AppImage  $APPDIR/AppRun              (the real program in $APPDIR/usr/bin/)
#   Arch      bin/anicraft                (the real program in bin/.anicraft-nightly/)
#   Flatpak   /app/bin/anicraft-nightly   (the real program in /app/bin/anicraft-bin)
#   nightly tar  anicraft                 (the real program in anicraft-bin beside it)
set -eu
case "$0" in
  */*) self=$0 ;;
  *) self=$(command -v -- "$0" 2>/dev/null || printf '%s' "$0") ;;
esac
# Physical, so a merged-`/usr` system's `/bin/<launcher>` is still the system install (/usr/bin).
here=$(CDPATH='' cd -P -- "$(dirname -- "$self")" && pwd -P)
self="$here/$(basename -- "$self")"

# Picks the first candidate that is a regular program of the bundle (never this script itself).
find_program() {
  for candidate in "$@"; do
    if [ -f "$candidate" ] && [ ! -L "$candidate" ] && ! [ "$candidate" -ef "$self" ]; then
      printf '%s' "$candidate"
      return 0
    fi
  done
  return 1
}

# Every candidate is either inside the bundle this launcher came with or the one fixed system path
# a package installs into: no path is computed from a parent of `$here`, because an AppImage mount,
# an extracted tar and the Arch handoff can sit under a folder another local user can write.
# A stable program is never a candidate either (`/usr/bin/anicraft` is stable's, and the packages
# are co-installable), so a co-installed Stable copy can never be started in Nightly's place.
real_bin=
if [ "$here" = /usr/bin ]; then
  # The deb/rpm puts the program in its own lib folder beside the system bin directory.
  real_bin=$(find_program /usr/lib/anicraft-nightly/anicraft "$here/anicraft-bin" \
    "$here/.anicraft-nightly/anicraft" "$here/usr/bin/anicraft") || :
else
  real_bin=$(find_program "$here/anicraft-bin" "$here/.anicraft-nightly/anicraft" \
    "$here/usr/bin/anicraft") || :
fi
if [ -z "$real_bin" ]; then
  echo 'anicraft-nightly: the AniCraft Nightly program is not where this launcher was installed' >&2
  exit 2
fi

# A relative XDG_DATA_HOME is ignored, as the XDG spec and the app's own data_home_for do: the
# profile root must be absolute, so a relative one would only make the program refuse to start.
case "${XDG_DATA_HOME:-}" in
  /*) data_home=$XDG_DATA_HOME ;;
  *) data_home="${HOME:?HOME is required to place the AniCraft Nightly profile}/.local/share" ;;
esac
profile_root="$data_home/anicraft-nightly/profile"

# The Launcher already speaks this protocol: pass it through rather than wrap it a second time.
case "${1:-}" in
  --launch-profile | --profile-root) exec "$real_bin" "$@" ;;
esac
# A delimiter the caller supplied is never doubled.
if [ "${1:-}" = "--" ]; then
  shift
  exec "$real_bin" --launch-profile nightly-v1 --profile-root "$profile_root" -- "$@"
fi
# `anicraft-nightly --version` answers the Launcher's own query form, before any profile exists.
if [ "$#" -eq 1 ] && { [ "$1" = "--version" ] || [ "$1" = "-V" ]; }; then
  exec "$real_bin" --launch-profile nightly-v1 --profile-root "$profile_root" "$1"
fi
exec "$real_bin" --launch-profile nightly-v1 --profile-root "$profile_root" -- "$@"
