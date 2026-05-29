#!/bin/sh
# mit installer — detects platform, downloads latest release, verifies checksum, installs.
#
# Usage:
#   curl -sSfL https://raw.githubusercontent.com/ttys026/mit/main/install.sh | sh
#
# Or from source:
#   cargo install --git https://github.com/ttys026/mit.git --bin mit

set -eu

REPO="ttys026/mit"
BINARY="mit"
# INSTALL_DIR is resolved after the banner:
#   1. honour an explicit INSTALL_DIR env var if set
#   2. otherwise prefer /usr/local/bin when writable
#   3. otherwise fall back to ~/.local/bin (no sudo required)
DEFAULT_SYSTEM_DIR="/usr/local/bin"
DEFAULT_USER_DIR="${HOME}/.local/bin"
SKIP_VERIFICATION=false

# ── Colors ──────────────────────────────────────────────────────────────────────

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

info()  { printf "${CYAN}${BOLD}==>${NC} %s\n" "$1"; }
ok()    { printf "${GREEN}${BOLD} ✓${NC} %s\n" "$1"; }
warn()  { printf "${YELLOW}${BOLD} !${NC} %s\n" "$1"; }
error() { printf "${RED}${BOLD}Error:${NC} %s\n" "$1" >&2; exit 1; }

# Parse command-line flags (must be after function definitions)
while [ $# -gt 0 ]; do
    case "$1" in
        --skip-verification)
            SKIP_VERIFICATION=true
            shift
            ;;
        *)
            error "Unknown option: $1"
            ;;
    esac
done

# ── Platform detection ──────────────────────────────────────────────────────────

detect_os() {
    case "$(uname -s)" in
        Linux*)  echo "linux" ;;
        Darwin*) echo "darwin" ;;
        MINGW*|MSYS*|CYGWIN*) echo "windows" ;;
        *) error "Unsupported operating system: $(uname -s)" ;;
    esac
}

detect_arch() {
    case "$(uname -m)" in
        x86_64|amd64)  echo "x86_64" ;;
        arm64|aarch64) echo "aarch64" ;;
        *) error "Unsupported architecture: $(uname -m)" ;;
    esac
}

OS="$(detect_os)"
ARCH="$(detect_arch)"

info "Detected platform: ${OS}-${ARCH}"

# ── Fetch latest release ────────────────────────────────────────────────────────

info "Fetching latest release..."

RELEASE_JSON="$(curl -sSfL "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null)" || error "Failed to fetch release info from GitHub."

# Extract version tag
VERSION="$(echo "$RELEASE_JSON" | grep '"tag_name"' | head -1 | sed 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/')"
if [ -z "$VERSION" ]; then
    error "Could not determine latest release version."
fi

# Validate VERSION against semantic versioning pattern to prevent code injection
if ! echo "$VERSION" | grep -qE '^v[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.]+)?$'; then
    error "Invalid version format: ${VERSION}. Expected semantic version (e.g., v1.0.0)."
fi

ok "Latest version: ${VERSION}"

# ── Construct download URL ──────────────────────────────────────────────────────

# Release assets are named with the bare version (no leading "v"),
# matching the release.yml workflow which strips the "v" prefix.
VERSION_BARE="${VERSION#v}"

# Archive naming: mit-{version}-{arch}-{platform}.tar.gz (or .zip for windows)
# Linux binaries ship as musl (statically linked, glibc-free) for portability.
PLATFORM="${OS}"
if [ "$OS" = "darwin" ]; then
    PLATFORM="apple-darwin"
elif [ "$OS" = "linux" ]; then
    PLATFORM="linux-musl"
fi

if [ "$OS" = "windows" ]; then
    ARCHIVE="${BINARY}-${VERSION_BARE}-${ARCH}-pc-windows-msvc.zip"
else
    ARCHIVE="${BINARY}-${VERSION_BARE}-${ARCH}-${PLATFORM}.tar.gz"
fi

DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${VERSION}/${ARCHIVE}"
CHECKSUM_URL="https://github.com/${REPO}/releases/download/${VERSION}/checksums.txt"

info "Downloading ${ARCHIVE}..."

# ── Download and verify ─────────────────────────────────────────────────────────

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

# Download archive
curl -sSfL -o "${TMPDIR}/${ARCHIVE}" "$DOWNLOAD_URL" || \
    error "Download failed. This platform (${ARCH}-${PLATFORM}) may not have a pre-built binary.\n  Try: cargo install --git https://github.com/${REPO}.git --bin mit"

ok "Downloaded ${ARCHIVE}"

# Download and verify checksum (if available)
CHECKSUM_DOWNLOADED=false
if curl -sSfL -o "${TMPDIR}/checksums.txt" "$CHECKSUM_URL" 2>/dev/null; then
    CHECKSUM_DOWNLOADED=true
fi

if [ "$SKIP_VERIFICATION" = true ]; then
    warn "Checksum verification skipped — this is insecure. Only skip if you trust the download source."
    warn "For security, verification is enabled by default. Re-run without --skip-verification for secure installs."
else
    if [ "$CHECKSUM_DOWNLOADED" = false ]; then
        error "Checksums not available for this release. Cannot verify binary integrity.\n  To bypass verification (not recommended), re-run with --skip-verification"
    fi

    EXPECTED="$(awk -v f="$ARCHIVE" '$2 == f { print $1 }' "${TMPDIR}/checksums.txt")"
    if [ -z "$EXPECTED" ]; then
        error "Checksum entry not found for ${ARCHIVE}. Binary file name mismatch or corrupted checksums.txt.\n  To bypass verification (not recommended), re-run with --skip-verification"
    fi

    if command -v sha256sum > /dev/null 2>&1; then
        ACTUAL="$(sha256sum "${TMPDIR}/${ARCHIVE}" | awk '{print $1}')"
    elif command -v shasum > /dev/null 2>&1; then
        ACTUAL="$(shasum -a 256 "${TMPDIR}/${ARCHIVE}" | awk '{print $1}')"
    else
        error "No SHA256 tool found (neither sha256sum nor shasum available). Cannot verify checksum.\n  Install sha256sum or shasum, or re-run with --skip-verification (not recommended)"
    fi

    if [ "$ACTUAL" != "$EXPECTED" ]; then
        error "Checksum verification failed!\n  Expected: ${EXPECTED}\n  Got:      ${ACTUAL}\n  The downloaded file may be corrupted. Please try again."
    fi
    ok "Checksum verified"
fi

# ── Extract ─────────────────────────────────────────────────────────────────────

info "Extracting..."

if [ "$OS" = "windows" ]; then
    unzip -qo "${TMPDIR}/${ARCHIVE}" -d "${TMPDIR}" || error "Failed to extract archive."
else
    tar -xzf "${TMPDIR}/${ARCHIVE}" -C "${TMPDIR}" || error "Failed to extract archive."
fi

# Find the binary (may be in a subdirectory)
EXTRACTED_BIN="$(find "${TMPDIR}" -name "${BINARY}" -type f | head -1)"
if [ -z "$EXTRACTED_BIN" ]; then
    # Try with .exe for Windows
    EXTRACTED_BIN="$(find "${TMPDIR}" -name "${BINARY}.exe" -type f | head -1)"
fi
if [ -z "$EXTRACTED_BIN" ]; then
    error "Could not find ${BINARY} binary in extracted archive."
fi

chmod +x "$EXTRACTED_BIN"
ok "Extracted ${BINARY}"

# ── Install ─────────────────────────────────────────────────────────────────────

# Resolve install location. Honour an explicit INSTALL_DIR; otherwise prefer the
# system dir when writable, fall back to ~/.local/bin when not. Never prompt for
# sudo automatically — scripts piped to `sh` can't read a TTY, and many
# environments (CI, corporate laptops, rootless containers) lack sudo entirely.
if [ -n "${INSTALL_DIR:-}" ]; then
    :  # user supplied INSTALL_DIR; respect it verbatim
elif [ -w "$DEFAULT_SYSTEM_DIR" ] 2>/dev/null; then
    INSTALL_DIR="$DEFAULT_SYSTEM_DIR"
else
    INSTALL_DIR="$DEFAULT_USER_DIR"
    info "${DEFAULT_SYSTEM_DIR} is not writable — installing to ${INSTALL_DIR} (no sudo needed)"
fi

mkdir -p "$INSTALL_DIR" || error "Could not create ${INSTALL_DIR}"

info "Installing to ${INSTALL_DIR}/${BINARY}..."

if [ -w "$INSTALL_DIR" ]; then
    mv "$EXTRACTED_BIN" "${INSTALL_DIR}/${BINARY}" || \
        error "Failed to move binary into ${INSTALL_DIR}. Set INSTALL_DIR to a writable directory and re-run."
else
    error "${INSTALL_DIR} is not writable. Set INSTALL_DIR=\$HOME/.local/bin (or another writable path) and re-run."
fi

ok "Installed ${BINARY} to ${INSTALL_DIR}/${BINARY}"

# ── Verify ──────────────────────────────────────────────────────────────────────

if command -v "$BINARY" > /dev/null 2>&1; then
    INSTALLED_VERSION="$("$BINARY" --version 2>/dev/null || echo "mit")"
    printf "\n"
    printf "${GREEN}${BOLD} ✓ ${INSTALLED_VERSION} installed successfully!${NC}\n"
    printf "\n"
    info "Binary:  ${INSTALL_DIR}/${BINARY}"
    printf "\n"
    printf "  ${CYAN}Get started:${NC}\n"
    printf "    ${BOLD}mit auth login${NC}\n"
    printf "\n"
else
    warn "${BINARY} was installed but is not in your PATH."
    printf "  Add this line to ${BOLD}~/.zshrc${NC} or ${BOLD}~/.bashrc${NC}:\n"
    printf "    ${BOLD}export PATH=\"${INSTALL_DIR}:\$PATH\"${NC}\n"
    printf "  Then reload: ${BOLD}source ~/.zshrc${NC} (or open a new shell).\n"
    printf "  Or run directly without modifying PATH: ${BOLD}${INSTALL_DIR}/${BINARY}${NC}\n"
fi