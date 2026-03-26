#!/bin/sh
# sd2dinit installer
# Installs the sd2dinit binary and optionally the pacman/alpm hook.
#
# Usage:
#   curl -fsSL https://gitlab.cherkaoui.ch/HadiCherkaoui/sd2dinit/-/raw/main/install.sh | sh
#   sh install.sh [--version v0.1.0] [--no-hook] [--global]

set -e

GITLAB_URL="https://gitlab.cherkaoui.ch"
PROJECT_PATH="HadiCherkaoui/sd2dinit"
PROJECT_PATH_ENCODED="HadiCherkaoui%2Fsd2dinit"
BINARY_NAME="sd2dinit-linux-x86_64"
HOOK_SRC="hooks/sd2dinit.hook"
HOOK_DEST="/usr/share/libalpm/hooks/sd2dinit.hook"

# ── Colours ──────────────────────────────────────────────────────────────────

if [ -t 1 ]; then
    GREEN='\033[0;32m'; YELLOW='\033[1;33m'; RED='\033[0;31m'; CYAN='\033[0;36m'; RESET='\033[0m'
else
    GREEN=''; YELLOW=''; RED=''; CYAN=''; RESET=''
fi

info()    { printf "${CYAN}info:${RESET}    %s\n" "$*"; }
success() { printf "${GREEN}ok:${RESET}      %s\n" "$*"; }
warn()    { printf "${YELLOW}warning:${RESET} %s\n" "$*"; }
err()     { printf "${RED}error:${RESET}   %s\n" "$*" >&2; exit 1; }

# ── Parse args ───────────────────────────────────────────────────────────────

VERSION=""
INSTALL_HOOK=true
FORCE_GLOBAL=false

while [ $# -gt 0 ]; do
    case "$1" in
        --version)  VERSION="$2";   shift 2 ;;
        --no-hook)  INSTALL_HOOK=false; shift ;;
        --global)   FORCE_GLOBAL=true;  shift ;;
        *)          err "unknown argument: $1" ;;
    esac
done

# ── Checks ───────────────────────────────────────────────────────────────────

if ! command -v curl >/dev/null 2>&1; then
    err "curl is required but not installed"
fi

ARCH=$(uname -m)
if [ "$ARCH" != "x86_64" ]; then
    err "only x86_64 is supported (got $ARCH)"
fi

# ── Resolve version ──────────────────────────────────────────────────────────

if [ -z "$VERSION" ]; then
    info "Fetching latest release..."
    API="${GITLAB_URL}/api/v4/projects/${PROJECT_PATH_ENCODED}/releases/permalink/latest"
    VERSION=$(curl -fsSL "$API" | grep -o '"tag_name":"[^"]*"' | cut -d'"' -f4)
    if [ -z "$VERSION" ]; then
        err "Could not determine latest version. Try: install.sh --version v0.1.0"
    fi
fi

info "Installing sd2dinit $VERSION"

# ── Privilege escalation ─────────────────────────────────────────────────────

if command -v doas >/dev/null 2>&1; then
    ESCALATE="doas"
elif command -v sudo >/dev/null 2>&1; then
    ESCALATE="sudo"
else
    ESCALATE=""
fi

# ── Choose install location ───────────────────────────────────────────────────

LOCAL_BIN="$HOME/.local/bin"

if [ "$FORCE_GLOBAL" = "false" ] && [ -d "$LOCAL_BIN" ] && \
   echo "$PATH" | tr ':' '\n' | grep -qxF "$LOCAL_BIN"; then
    INSTALL_DIR="$LOCAL_BIN"
    NEEDS_ESCALATION=false
    info "Installing to $INSTALL_DIR (no escalation needed)"
elif [ "$FORCE_GLOBAL" = "false" ]; then
    # ~/.local/bin exists or can be created but not in PATH
    mkdir -p "$LOCAL_BIN"
    INSTALL_DIR="$LOCAL_BIN"
    NEEDS_ESCALATION=false
    warn "$LOCAL_BIN is not in PATH — add this to your shell profile:"
    warn "  export PATH=\"\$HOME/.local/bin:\$PATH\""
else
    INSTALL_DIR="/usr/local/bin"
    NEEDS_ESCALATION=true
    if [ -z "$ESCALATE" ]; then
        err "--global requested but neither doas nor sudo is available"
    fi
    info "Installing to $INSTALL_DIR (requires $ESCALATE)"
fi

# ── Download binary ───────────────────────────────────────────────────────────

DOWNLOAD_URL="${GITLAB_URL}/api/v4/projects/${PROJECT_PATH_ENCODED}/packages/generic/sd2dinit/${VERSION}/${BINARY_NAME}"
TMP_BIN=$(mktemp)

info "Downloading $BINARY_NAME..."
if ! curl -fsSL --progress-bar "$DOWNLOAD_URL" -o "$TMP_BIN"; then
    rm -f "$TMP_BIN"
    err "Download failed. Check that version $VERSION exists and the registry is accessible."
fi

chmod +x "$TMP_BIN"

# ── Install binary ────────────────────────────────────────────────────────────

if [ "$NEEDS_ESCALATION" = "true" ]; then
    $ESCALATE install -Dm755 "$TMP_BIN" "$INSTALL_DIR/sd2dinit"
else
    install -Dm755 "$TMP_BIN" "$INSTALL_DIR/sd2dinit"
fi

rm -f "$TMP_BIN"
success "sd2dinit installed to $INSTALL_DIR/sd2dinit"

# ── Install pacman hook (optional) ────────────────────────────────────────────

if [ "$INSTALL_HOOK" = "true" ]; then
    HOOK_DIR=$(dirname "$HOOK_DEST")

    if [ ! -d "$HOOK_DIR" ]; then
        warn "Hook directory $HOOK_DIR not found — skipping hook install (not Artix/Arch?)"
    elif [ -z "$ESCALATE" ]; then
        warn "Hook install requires privilege escalation but neither doas nor sudo found"
        warn "Manually install: cp $HOOK_SRC $HOOK_DEST"
    else
        info "Installing pacman hook (requires $ESCALATE)..."
        HOOK_URL="${GITLAB_URL}/${PROJECT_PATH}/-/raw/${VERSION}/hooks/sd2dinit.hook"
        TMP_HOOK=$(mktemp)
        if curl -fsSL "$HOOK_URL" -o "$TMP_HOOK"; then
            $ESCALATE install -Dm644 "$TMP_HOOK" "$HOOK_DEST"
            rm -f "$TMP_HOOK"
            success "Hook installed to $HOOK_DEST"
            info "sd2dinit will now auto-convert .service files on pacman install/upgrade"
        else
            rm -f "$TMP_HOOK"
            warn "Could not download hook file — install manually:"
            warn "  $ESCALATE cp hooks/sd2dinit.hook $HOOK_DEST"
        fi
    fi
fi

# ── Done ──────────────────────────────────────────────────────────────────────

echo ""
success "Installation complete!"
echo ""
echo "Quick start:"
echo "  sd2dinit convert /usr/lib/systemd/system/sshd.service --dry-run"
echo "  sd2dinit install /usr/lib/systemd/system/nginx.service --enable --start"
echo ""
echo "See sd2dinit --help for full usage."
