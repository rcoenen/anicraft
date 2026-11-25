#!/usr/bin/env bash
# Stage the AniCraft Nightly-only handoff for the Arch packager.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
source_tree="$1"
work="$2"
destination="$3"
basename="$4"
stage="$work/$basename"
app_id=ai.storyteller.anicraft
nightly_id=$app_id.nightly-v1

mkdir -p "$work"
cp -R "$source_tree" "$stage"
mkdir "$stage/bin/.anicraft-nightly"
mv "$stage/bin/anicraft" "$stage/bin/.anicraft-nightly/anicraft"
# The launcher every Nightly format installs: it starts the real program with
# --launch-profile nightly-v1 --profile-root <its own profile> -- <arguments>
# (packaging/linux/nightly-launch.sh).
cp "$here/nightly-launch.sh" "$stage/bin/anicraft"
chmod 755 "$stage/bin/anicraft"

# The packager ships bin/ and share/ as they are, so Arch Nightly must not land on stable's
# desktop entry, AppStream id, icons or MIME package: ship the Nightly ones. Their
# StartupWMClass is the window class the Nightly build sets (src/platform.rs APP_ID_NIGHTLY).
if [[ -d "$stage/share" ]]; then
  if [[ -f "$stage/share/applications/$app_id.desktop" ]]; then
    sed -e "s#^Exec=anicraft\([[:space:]]\|$\)#Exec=anicraft-nightly\1#" \
        -e "s|^TryExec=anicraft$|TryExec=anicraft-nightly|" \
        -e "s|^Icon=$app_id$|Icon=$nightly_id|" \
        -e "s|^Name=AniCraft$|Name=AniCraft Nightly|" \
        -e "s|^StartupWMClass=.*|StartupWMClass=$nightly_id|" \
        -e '/^MimeType=/d' \
        "$stage/share/applications/$app_id.desktop" >"$stage/share/applications/$nightly_id.desktop"
    rm -f "$stage/share/applications/$app_id.desktop"
  fi
  if [[ -f "$stage/share/metainfo/$app_id.metainfo.xml" ]]; then
    sed -e "s|<id>$app_id</id>|<id>$nightly_id</id>|g" \
        -e "s|<name>AniCraft</name>|<name>AniCraft Nightly</name>|g" \
        -e "s|<launchable type=\"desktop-id\">$app_id.desktop</launchable>|<launchable type=\"desktop-id\">$nightly_id.desktop</launchable>|g" \
        -e "s|<icon type=\"stock\">$app_id</icon>|<icon type=\"stock\">$nightly_id</icon>|g" \
        -e "s|<binary>anicraft</binary>|<binary>anicraft-nightly</binary>|g" \
        -e "s|<binary>anicraft-cli</binary>|<binary>anicraft-nightly-cli</binary>|g" \
        -e '/<mediatype>/d' \
        "$stage/share/metainfo/$app_id.metainfo.xml" >"$stage/share/metainfo/$nightly_id.metainfo.xml"
    rm -f "$stage/share/metainfo/$app_id.metainfo.xml"
  fi
  rm -rf "$stage/share/mime"
  if [[ -f "$here/../../assets/app-icon/anicraft-nightly.png" ]]; then
    rm -rf "$stage/share/icons/hicolor"
    install -Dm644 "$here/../../assets/app-icon/anicraft-nightly.png" \
      "$stage/share/icons/hicolor/256x256/apps/$nightly_id.png"
  fi
fi

tar -C "$work" -czf "$destination/$basename.tar.gz" "$basename"
echo "wrote $destination/$basename.tar.gz"
