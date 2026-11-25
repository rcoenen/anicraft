#!/usr/bin/env bash
# Fetches the legacy .fla corpus that crates/fla/tests/corpus.tsv pins: EDAP Tools' public
# sample rigs and templates (CC BY-NC-SA 3.0, never committed). Each file is downloaded from
# its URL, checked against its sha256 (a mismatch stops the script), and the zips are unpacked
# into the folders the env-gated tests read. Files already there with the right sha256 are kept.
#
#   scripts/fetch-fla-corpus.sh <folder>
#   ANICRAFT_FLA_CORPUS=<folder> cargo test -p anicraft-fla
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MANIFEST="$ROOT/crates/fla/tests/corpus.tsv"
DIR="${1:?usage: scripts/fetch-fla-corpus.sh <folder>}"
mkdir -p "$DIR"

sha256() {
  if command -v sha256sum >/dev/null; then sha256sum < "$1" | cut -d' ' -f1; else shasum -a 256 < "$1" | cut -d' ' -f1; fi
}

while IFS=$'\t' read -r sha file url unpack _licence title; do
  [[ -z "$sha" || "$sha" == \#* ]] && continue
  out="$DIR/$file"
  if [[ ! -f "$out" || "$(sha256 "$out")" != "$sha" ]]; then
    echo "fetching $file ($title)"
    curl -fsSL -o "$out.part" "$url"
    got="$(sha256 "$out.part")"
    if [[ "$got" != "$sha" ]]; then
      rm -f "$out.part"
      echo "error: $file from $url has sha256 $got, the manifest pins $sha" >&2
      exit 1
    fi
    mv "$out.part" "$out"
  fi
  if [[ "$unpack" != "-" ]]; then
    # One of the Animate twins (mademoiselle) overstates its zip directory's size, as Animate's
    # own writer often does: unzip warns (status 1 or 2) but extracts every file.
    status=0
    unzip -o -q "$out" -d "$DIR/$unpack" || status=$?
    if (( status > 2 )) || [[ -z "$(ls -A "$DIR/$unpack")" ]]; then
      echo "error: $file didn't unpack into $unpack (unzip status $status)" >&2
      exit 1
    fi
  fi
done < "$MANIFEST"
echo "corpus ready in $DIR"
