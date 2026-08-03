#!/usr/bin/env sh
# BitVanes installer — macOS / Linux.
# Usage:  curl -fsSL https://bitvanes.com/install.sh | sh
#
# Downloads the latest binary for the detected OS/arch from GitHub Releases
# and installs it to ~/.local/bin (no sudo). Prints PATH guidance.

set -eu

REPO="BitVanes/releases"
API="https://api.github.com/repos/${REPO}/releases/latest"

err() { printf '\033[31m%s\033[0m\n' "$*" >&2; }
info() { printf '\033[1m%s\033[0m\n' "$*"; }

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Darwin) osname="macos" ;;
  Linux)  osname="linux" ;;
  *) err "Unsupported OS: $OS (use the Windows .zip / install.ps1)"; exit 1 ;;
esac

case "$ARCH" in
  arm64|aarch64) archname="aarch64" ;;
  x86_64|amd64)  archname="x86_64" ;;
  *) err "Unsupported arch: $ARCH"; exit 1 ;;
esac

# The release matrix produces bitvanes-{arch}-{os}.tar.gz
asset="bitvanes-${archname}-${osname}.tar.gz"

info "Fetching latest release…"
asset_url="$(curl -fsSL -H 'Accept: application/vnd.github+json' "$API" \
  | grep -oE "\"browser_download_url\":\s*\"[^\"]+/${asset}\"" \
  | head -1 | sed -E 's/.*"([^"]+)".*/\1/')"

if [ -z "$asset_url" ]; then
  err "No asset '$asset' found in the latest release."
  err "Pre-built binaries may not be available yet. Build from source:"
  err "  https://github.com/${REPO}#readme"
  exit 1
fi

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

info "Downloading $asset…"
curl -fsSL -o "${tmpdir}/${asset}" "$asset_url"

info "Installing to ~/.local/bin"
install_dir="${HOME}/.local/bin"
mkdir -p "$install_dir"
tar -xzf "${tmpdir}/${asset}" -C "$install_dir" bitvanes 2>/dev/null || \
  tar -xzf "${tmpdir}/${asset}" -C "$install_dir"

chmod +x "${install_dir}/bitvanes" 2>/dev/null || true

info "✔ Installed: ${install_dir}/bitvanes"

case ":${PATH}:" in
  *":${install_dir}:"*) ;;
  *)
    info "Add ${install_dir} to your PATH:"
    if [ -n "${ZSH_VERSION:-}" ] || [ "$SHELL" = */zsh ]; then
      printf '    echo "export PATH=\$HOME/.local/bin:\$PATH" >> ~/.zshrc\n'
    else
      printf '    echo "export PATH=\$HOME/.local/bin:\$PATH" >> ~/.bashrc\n'
    fi
    ;;
esac

# macOS quarantine note.
if [ "$osname" = "macos" ]; then
  info "macOS: if Gatekeeper blocks the binary, run:"
  printf '    xattr -d com.apple.quarantine %s/bitvanes\n' "$install_dir"
fi

info "Run:  bitvanes --help"
