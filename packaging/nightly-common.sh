# shellcheck shell=bash
# Shared setup for AniCraft's own native nightly archives; stable packaging does not source this
# file. PhotoCraft's `packaging/nightly-common.sh`, with this app's names and its GUI build.

nightly_die() { printf 'error: %s\n' "$*" >&2; exit 2; }

nightly_init() {
  local os="$1" arch="$2" target="$3" date_digits
  [[ "${ANICRAFT_BUILD_CHANNEL:-}" == nightly-v1 ]] || nightly_die 'ANICRAFT_BUILD_CHANNEL must be nightly-v1'
  [[ "${ANICRAFT_VERSION:-}" =~ ^[0-9]+\.[0-9]+\.[0-9]+-nightly\.[0-9]{8}\.[0-9]+\.[0-9]+\.g[0-9a-f]{12}$ ]] || nightly_die 'an exact prepared nightly version is required'
  [[ "${ANICRAFT_BUILD_SHA:-}" =~ ^[0-9a-f]{40}$ ]] || nightly_die 'an exact source commit is required'
  [[ "${ANICRAFT_VERSION##*.g}" == "${ANICRAFT_BUILD_SHA:0:12}" ]] || nightly_die 'version/source commit mismatch'
  [[ "${ANICRAFT_BUILD_DATE:-}" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]] || nightly_die 'the original run-created UTC date is required'
  date_digits="${ANICRAFT_VERSION#*-nightly.}"
  date_digits="${date_digits%%.*}"
  [[ "${ANICRAFT_BUILD_DATE//-/}" == "$date_digits" ]] || nightly_die 'version/original-run date mismatch'
  [[ "${CARGO_TARGET_DIR:-}" == /* && "$CARGO_TARGET_DIR" != / ]] || nightly_die 'an absolute owned CARGO_TARGET_DIR base is required'
  [[ "${DIST:-}" == /* && "$DIST" != / ]] || nightly_die 'an absolute DIST output directory is required'
  [[ "/$CARGO_TARGET_DIR/" != *'/../'* && "/$DIST/" != *'/../'* ]] || nightly_die 'output paths must not contain parent traversal'
  ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
  [[ "$(git -C "$ROOT" rev-parse HEAD)" == "$ANICRAFT_BUILD_SHA" ]] || nightly_die 'prepared source is not the checked-out commit'
  [[ -z "$(git -C "$ROOT" status --porcelain)" ]] || nightly_die 'nightly packaging requires the frozen clean source'
  if [[ -n "${CRAFT_FONTS_DIR:-}" && ! -d "$CRAFT_FONTS_DIR/fonts" ]]; then
    nightly_die 'declared CRAFT_FONTS_DIR has no fonts directory'
  fi
  # env.sh keeps the established notice copier and reads ANICRAFT_VERSION into VERSION; the values
  # required above prohibit its stable fallbacks. build.rs (src/build_channel.rs) compiles the
  # channel and the prepared feed identity in, so the binary prints that exact version and never
  # Cargo's 0.3.0.
  export ANICRAFT_BUILD_CHANNEL ANICRAFT_VERSION ANICRAFT_BUILD_SHA ANICRAFT_BUILD_DATE
  # shellcheck source=env.sh
  . "$ROOT/packaging/env.sh"
  [[ "$VERSION" == "$ANICRAFT_VERSION" ]] || nightly_die 'the prepared version and env.sh disagree'
  export CARGO_TARGET_DIR="$CARGO_TARGET_DIR/nightly-v1/$os-$arch"
  NIGHTLY_RUST_TARGET="$target"
  mkdir -p "$CARGO_TARGET_DIR"
  NIGHTLY_WORK="$(mktemp -d "$CARGO_TARGET_DIR/nightly-package.XXXXXX")"
  trap 'rm -rf -- "$NIGHTLY_WORK"' EXIT
}

# Compile only: macOS runs the version query after signing (packaging/macos/nightly-verify.sh).
# The GUI alone, with the default features of a stable build (sound output and the GPU stage): the
# nightly archive has no anicraft-cli.
nightly_compile_gui() {
  (cd "$ROOT" && cargo build --release --locked -p anicraft --target "$NIGHTLY_RUST_TARGET")
  # Not `$(... && printf)`: its status 1 on a non-Windows target would end the build under set -e.
  NIGHTLY_GUI="$CARGO_TARGET_DIR/$NIGHTLY_RUST_TARGET/release/anicraft"
  if [[ "$NIGHTLY_RUST_TARGET" == *windows* ]]; then
    NIGHTLY_GUI+=".exe"
  fi
  [[ -f "$NIGHTLY_GUI" && ! -L "$NIGHTLY_GUI" ]] || nightly_die 'the owned GUI build did not produce a regular executable'
  [[ "$(git -C "$ROOT" rev-parse HEAD)" == "$ANICRAFT_BUILD_SHA" && -z "$(git -C "$ROOT" status --porcelain)" ]] || nightly_die 'source changed during the build'
}

nightly_build_gui() {
  nightly_compile_gui
  local actual
  actual="$("$NIGHTLY_GUI" --version)" || nightly_die 'side-effect-free nightly version verification failed'
  # The line the flavor prints: the contract's `<App> <version> (nightly-v1)` (`src/main.rs`,
  # `src/build_flavor.rs`). The Launcher and the shared producer parse this same line.
  [[ "$actual" == "AniCraft $VERSION (nightly-v1)" ]] || nightly_die 'the built version/flavor is not the prepared identity'
  [[ "$(git -C "$ROOT" rev-parse HEAD)" == "$ANICRAFT_BUILD_SHA" && -z "$(git -C "$ROOT" status --porcelain)" ]] || nightly_die 'source changed during the version query'
}

# Signing material never reaches compilation (the Windows -CheckBuildEnvironment rule).
nightly_refuse_signing_env() {
  local name
  # The signing names release.yml passes, for every platform, and import-cert.sh's outputs.
  for name in APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD KEYCHAIN_PASSWORD APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID \
    MACOS_SIGN_IDENTITY MACOS_KEYCHAIN WINDOWS_CERTIFICATE WINDOWS_CERTIFICATE_PASSWORD AZURE_TENANT_ID AZURE_CLIENT_ID \
    AZURE_CLIENT_SECRET AZURE_SIGNING_ENDPOINT AZURE_SIGNING_ACCOUNT AZURE_CERT_PROFILE LINUX_SIGNING_KEY; do
    [[ -z "${!name:-}" ]] || nightly_die "compilation must not receive signing configuration ($name); sign in the separate trusted step"
  done
}

nightly_copy_notices() {
  local destination="$1" required
  for required in README.md LICENSE-MIT LICENSE-APACHE NOTICE ATTRIBUTION.md; do
    [[ -f "$ROOT/$required" && ! -L "$ROOT/$required" ]] || nightly_die "missing regular app notice: $required"
  done
  copy_docs "$destination"
}

nightly_publish_archive() {
  # Exclusive creation: never replace an existing artifact, even if another writer raced the preflight.
  python3 -I - "$1" "$2" <<'PY'
import os, shutil, sys
source, destination = sys.argv[1:]
created = False
try:
    # Open the input first: a missing source must not leave an empty final artifact.
    with open(source, 'rb') as incoming:
        with open(destination, 'xb') as output:
            created = True
            shutil.copyfileobj(incoming, output, length=1024 * 1024)
            output.flush()
            os.fsync(output.fileno())
except BaseException:
    if created:
        os.unlink(destination)
    raise
PY
  printf 'Nightly artifact: %s\n' "$2"
}
