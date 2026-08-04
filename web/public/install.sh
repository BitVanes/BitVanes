#!/usr/bin/env sh
# BitVanes installer — macOS / Linux.
#
#   curl -fsSL https://bitvanes.com/install.sh | sh
#
# Downloads the latest release for your OS/arch from BitVanes/releases into
# ~/.bitvanes and adds it to PATH. The bundle is self-contained (NER model +
# libonnxruntime + libpdfium included), so no extra setup. For Tier-2 name
# redaction, also run `bitvanes-nerd` (and set BITVANES_LICENSE_KEY for paid
# features).

set -eu

PREFIX="${BITVANES_INSTALL_PREFIX:-$HOME/.bitvanes}"

err() { printf 'bitvanes install: %s\n' "$*" >&2; exit 1; }

# --- detect OS + arch → asset name -------------------------------------------
OS=$(uname -s)
ARCH=$(uname -m)
case "$OS/$ARCH" in
  Linux/x86_64|Linux/amd64)            ASSET="bitvanes-x86_64-linux.tar.gz" ;;
  Darwin/arm64)                        ASSET="bitvanes-aarch64-macos.tar.gz" ;;
  Darwin/x86_64)                       ASSET="bitvanes-x86_64-macos.tar.gz" ;;
  *) err "unsupported OS/arch: $OS/$ARCH (see bitvanes.com for manual download)" ;;
esac

# --- find the latest release download URL ------------------------------------
API="https://api.github.com/repos/BitVanes/releases/releases/latest"
printf 'bitvanes: resolving latest release... '
URL=$(curl -fsSL "$API" \
  | grep -o "\"browser_download_url\": *\"[^\"]*/$ASSET\"" \
  | head -1 \
  | grep -o 'https://[^"]*')
[ -n "$URL" ] || err "could not find $ASSET in the latest BitVanes release"
printf 'ok\n'

# --- download + extract ------------------------------------------------------
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
printf 'bitvanes: downloading %s... ' "$ASSET"
curl -fsSL "$URL" -o "$TMP/$ASSET"
printf 'ok\n'

mkdir -p "$PREFIX"
# tarball extracts to `./` (binaries + lib/ + models/ at the archive root).
tar xzf "$TMP/$ASSET" -C "$PREFIX"
printf 'bitvanes: installed to %s\n' "$PREFIX"

# --- macOS: clear Gatekeeper quarantine so the unsigned binaries run ---------
if [ "$OS" = "Darwin" ]; then
  xattr -dr com.apple.quarantine "$PREFIX" 2>/dev/null || true
fi
chmod +x "$PREFIX"/bitvanes "$PREFIX"/bitvanes-nerd 2>/dev/null || true

# --- PATH hint ---------------------------------------------------------------
add_to_path() {
  cat <<EOF

bitvanes: add to PATH (restart shell after):
  export PATH="$PREFIX:\$PATH"

then:
  bitvanes --help            # the CLI (scrub / filter / daemon / tui)
  bitvanes-nerd &            # the Tier-2 NER sidecar (run for name redaction)

Tier-2 NER (personal names, orgs, locations) needs the sidecar running AND a
paid license key in BITVANES_LICENSE_KEY. Without it, BitVanes still fully
redacts email, SSN, phone, credit card, routing numbers, API keys, JWTs.
EOF
}
add_to_path
