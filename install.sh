#!/bin/sh
# sd2dinit installer
#
# Usage:
#   curl -fsSL https://gitlab.cherkaoui.ch/HadiCherkaoui/sd2dinit/-/raw/main/install.sh | sh
#   sh install.sh [--version v0.1.0] [--no-hook]

set -e

GITLAB_URL="https://gitlab.cherkaoui.ch"
PROJECT_PATH_ENCODED="HadiCherkaoui%2Fsd2dinit"
BINARY_NAME="sd2dinit-linux-x86_64"
INSTALL_BIN="/usr/local/bin/sd2dinit"
HOOK_DEST="/usr/share/libalpm/hooks/sd2dinit.hook"

# ── Colours ──────────────────────────────────────────────────────────────────

if [ -t 1 ]; then
    GREEN='\033[0;32m' YELLOW='\033[1;33m' RED='\033[0;31m' CYAN='\033[0;36m' RESET='\033[0m'
else
    GREEN='' YELLOW='' RED='' CYAN='' RESET=''
fi

info()    { printf "${CYAN}info:${RESET}    %s\n" "$*"; }
success() { printf "${GREEN}ok:${RESET}      %s\n" "$*"; }
warn()    { printf "${YELLOW}warning:${RESET} %s\n" "$*"; }
err()     { printf "${RED}error:${RESET}   %s\n" "$*" >&2; exit 1; }

# ── Parse args ───────────────────────────────────────────────────────────────

VERSION=""
INSTALL_HOOK=true

while [ $# -gt 0 ]; do
    case "$1" in
        --version) VERSION="$2"; shift 2 ;;
        --no-hook) INSTALL_HOOK=false; shift ;;
        *) err "unknown argument: $1" ;;
    esac
done

# ── Requirements ─────────────────────────────────────────────────────────────

command -v curl >/dev/null 2>&1 || err "curl is required but not installed"

[ "$(uname -m)" = "x86_64" ] || err "only x86_64 is supported (got $(uname -m))"

# Privilege escalation: doas preferred, sudo as fallback
if command -v doas >/dev/null 2>&1; then
    ESCALATE="doas"
elif command -v sudo >/dev/null 2>&1; then
    ESCALATE="sudo"
else
    err "neither doas nor sudo found — cannot install system-wide"
fi

info "Using $ESCALATE for privilege escalation"

# ── Resolve version ───────────────────────────────────────────────────────────

if [ -z "$VERSION" ]; then
    info "Fetching latest release..."
    API="${GITLAB_URL}/api/v4/projects/${PROJECT_PATH_ENCODED}/releases/permalink/latest"
    VERSION=$(curl -fsSL "$API" | grep -o '"tag_name":"[^"]*"' | cut -d'"' -f4)
    [ -n "$VERSION" ] || err "Could not determine latest version. Try: install.sh --version v0.1.0"
fi

# Show currently installed version if any
if command -v sd2dinit >/dev/null 2>&1; then
    CURRENT=$(sd2dinit --version 2>/dev/null | awk '{print $2}' || true)
    if [ -n "$CURRENT" ]; then
        info "Upgrading sd2dinit $CURRENT → $VERSION"
    else
        info "Installing sd2dinit $VERSION"
    fi
else
    info "Installing sd2dinit $VERSION"
fi

# ── Download binary ───────────────────────────────────────────────────────────

DOWNLOAD_URL="${GITLAB_URL}/api/v4/projects/${PROJECT_PATH_ENCODED}/packages/generic/sd2dinit/${VERSION}/${BINARY_NAME}"
TMP_BIN=$(mktemp)

info "Downloading $BINARY_NAME..."
curl -fsSL --progress-bar "$DOWNLOAD_URL" -o "$TMP_BIN" \
    || { rm -f "$TMP_BIN"; err "Download failed. Check that version $VERSION exists."; }

chmod +x "$TMP_BIN"

# ── Install binary to /usr/local/bin ─────────────────────────────────────────

info "Installing to $INSTALL_BIN (requires $ESCALATE)..."
$ESCALATE install -Dm755 "$TMP_BIN" "$INSTALL_BIN"
rm -f "$TMP_BIN"
success "sd2dinit installed to $INSTALL_BIN"

# ── Install pacman hook ───────────────────────────────────────────────────────

if [ "$INSTALL_HOOK" = "true" ]; then
    HOOK_DIR=$(dirname "$HOOK_DEST")
    if [ ! -d "$HOOK_DIR" ]; then
        warn "Hook directory $HOOK_DIR not found — skipping (not Artix/Arch?)"
    else
        info "Installing pacman hook to $HOOK_DEST (requires $ESCALATE)..."
        HOOK_URL="${GITLAB_URL}/api/v4/projects/${PROJECT_PATH_ENCODED}/packages/generic/sd2dinit/${VERSION}/sd2dinit.hook"
        # Fall back to raw repo URL if hook not in package registry
        HOOK_URL_RAW="${GITLAB_URL}/HadiCherkaoui/sd2dinit/-/raw/${VERSION}/hooks/sd2dinit.hook"
        TMP_HOOK=$(mktemp)
        if curl -fsSL "$HOOK_URL" -o "$TMP_HOOK" 2>/dev/null \
            || curl -fsSL "$HOOK_URL_RAW" -o "$TMP_HOOK" 2>/dev/null; then
            $ESCALATE install -Dm644 "$TMP_HOOK" "$HOOK_DEST"
            rm -f "$TMP_HOOK"
            success "Hook installed to $HOOK_DEST"
            info "sd2dinit will now auto-convert .service files on pacman install/upgrade"
        else
            rm -f "$TMP_HOOK"
            warn "Could not download hook file. Install manually:"
            warn "  $ESCALATE install -Dm644 hooks/sd2dinit.hook $HOOK_DEST"
        fi
    fi
fi

# ── Done ──────────────────────────────────────────────────────────────────────

printf "\n"
success "Installation complete!"
printf "\n"
printf "Quick start:\n"
printf "  sd2dinit convert /usr/lib/systemd/system/sshd.service --dry-run\n"
printf "  %s sd2dinit install /usr/lib/systemd/system/nginx.service --enable --start\n" "$ESCALATE"
printf "\n"
printf "See sd2dinit --help for full usage.\n"
