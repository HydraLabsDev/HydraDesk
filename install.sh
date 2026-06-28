#!/usr/bin/env bash
# ==========================================================================
# Copyright (c) 2026 HydraCodeLabs
# Owner: HydraCodeLabs
# Project: HydraDesk
# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
# Last updated: 2026-06-27T01:32:17Z
# ==========================================================================

# HydraDesk installer / uninstaller
#
# Install (one line, builds from source if no release is published yet):
#   curl -fsSL https://raw.githubusercontent.com/HydraLabsDev/HydraDesk/main/install.sh | bash
#
# Force build from source:
#   curl -fsSL https://raw.githubusercontent.com/HydraLabsDev/HydraDesk/main/install.sh | bash -s -- --build
#
# From a cloned repo:
#   bash install.sh            # use a local build / release
#   bash install.sh --build    # compile from source here
#
# Uninstall:
#   curl -fsSL https://raw.githubusercontent.com/HydraLabsDev/HydraDesk/main/install.sh | bash -s -- --uninstall
#   # or, if installed:  sudo hydradesk uninstall

set -euo pipefail

INSTALL_BIN="/usr/local/bin/hydradesk"
REPO="HydraLabsDev/HydraDesk"
VERSION="${HYDRADESK_VERSION:-latest}"

# Elevate individual commands when not already root.
SUDO=""
[[ "${EUID:-$(id -u)}" -ne 0 ]] && SUDO="sudo"

# ── Colours ───────────────────────────────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'
CYAN='\033[0;36m'; BOLD='\033[1m'; RESET='\033[0m'

info()  { echo -e "${CYAN}[hydradesk]${RESET} $*"; }
ok()    { echo -e "${GREEN}[hydradesk]${RESET} $*"; }
warn()  { echo -e "${YELLOW}[hydradesk]${RESET} $*"; }
die()   { echo -e "${RED}[hydradesk] ERROR:${RESET} $*" >&2; exit 1; }

# ── Distro abstraction ─────────────────────────────────────────────────────────
# HydraDesk runs on GNOME across Debian-family (apt) and Red Hat-family (dnf)
# distros. Detect by which tools exist rather than parsing /etc/os-release.
pkg_mgr() {
    if command -v apt-get &>/dev/null; then echo apt
    elif command -v dnf &>/dev/null; then echo dnf
    elif command -v yum &>/dev/null; then echo yum
    else echo none
    fi
}

# GDM daemon config: /etc/gdm3 on Debian/Ubuntu, /etc/gdm elsewhere.
gdm_conf_path() {
    if [[ -d /etc/gdm3 ]]; then echo /etc/gdm3/custom.conf; else echo /etc/gdm/custom.conf; fi
}

# ── Version helpers ─────────────────────────────────────────────────────────────
# The version we are about to install, read from the source manifest when running
# in a cloned repo. Prints nothing if it cannot be determined.
local_source_version() {
    local f
    for f in ./cli/Cargo.toml ./Cargo.toml; do
        if [[ -f "$f" ]]; then
            local v
            v=$(grep -E '^[[:space:]]*version[[:space:]]*=' "$f" | head -1 | sed -E 's/.*"([^"]+)".*/\1/')
            [[ -n "$v" ]] && { printf '%s\n' "$v"; return 0; }
        fi
    done
    return 1
}

# The latest published release tag on GitHub (e.g. "1.0.0"), or nothing if the
# API is unreachable or no release exists yet.
latest_release_version() {
    local url="https://api.github.com/repos/${REPO}/releases/latest" json
    if command -v curl &>/dev/null; then
        json=$(curl -fsSL "$url" 2>/dev/null) || return 1
    elif command -v wget &>/dev/null; then
        json=$(wget -qO- "$url" 2>/dev/null) || return 1
    else
        return 1
    fi
    printf '%s' "$json" \
        | grep -o '"tag_name"[[:space:]]*:[[:space:]]*"[^"]*"' \
        | head -1 \
        | sed -E 's/.*"v?([^"]*)".*/\1/'
}

# True when $1 is a strictly higher semantic version than $2.
version_gt() {
    [[ "$1" == "$2" ]] && return 1
    local highest
    highest=$(printf '%s\n%s\n' "$1" "$2" | sort -V | tail -n1)
    [[ "$highest" == "$1" ]]
}

# After install, tell the user if a newer release is published. $1 = installed
# version string (may be "hydradesk 1.0.0" or "1.0.0"). Never fails the install.
notify_if_outdated() {
    local installed="${1##* }"   # keep the last token, dropping any "hydradesk " prefix
    installed="${installed#v}"
    [[ -z "$installed" || "$installed" == "unknown" ]] && return 0

    local latest
    latest=$(latest_release_version) || return 0
    [[ -z "$latest" ]] && return 0

    if version_gt "$latest" "$installed"; then
        echo
        warn "A newer HydraDesk is available: v${latest} (you have v${installed})."
        warn "Update:  curl -fsSL https://raw.githubusercontent.com/${REPO}/main/install.sh | bash -s -- --build"
    else
        ok "You are on the latest version (v${installed})."
    fi
}

find_built_binary() {
    local base="$1"
    local candidate
    for candidate in \
        "$base/target/release/hydradesk" \
        "$base/cli/target/release/hydradesk"
    do
        if [[ -f "$candidate" ]]; then
            printf '%s\n' "$candidate"
            return 0
        fi
    done
    return 1
}

print_banner() {
    printf '%b' "${CYAN}${BOLD}"
    cat <<'BANNER'

██╗  ██╗██╗   ██╗██████╗ ██████╗  █████╗    ██████╗ ███████╗███████╗██╗  ██╗
██║  ██║╚██╗ ██╔╝██╔══██╗██╔══██╗██╔══██╗   ██╔══██╗██╔════╝██╔════╝██║ ██╔╝
███████║ ╚████╔╝ ██║  ██║██████╔╝███████║   ██║  ██║█████╗  ███████╗█████╔╝
██╔══██║  ╚██╔╝  ██║  ██║██╔══██╗██╔══██║   ██║  ██║██╔══╝  ╚════██║██╔═██╗
██║  ██║   ██║   ██████╔╝██║  ██║██║  ██║   ██████╔╝███████╗███████║██║  ██╗
╚═╝  ╚═╝   ╚═╝   ╚═════╝ ╚═╝  ╚═╝╚═╝  ╚═╝   ╚═════╝ ╚══════╝╚══════╝╚═╝  ╚═╝
BANNER
    printf '%b\n' "${RESET}"
    echo -e "${CYAN}        Physical Linux desktops over native Windows RDP${RESET}"
    echo
}

# ── Detect architecture ───────────────────────────────────────────────────────
detect_target() {
    local arch
    arch=$(uname -m)
    case "$arch" in
        # Static musl builds so one binary runs on any Linux distro, regardless
        # of glibc version. Must match the targets in .github/workflows/release.yml.
        x86_64)        echo "x86_64-unknown-linux-musl" ;;
        aarch64|arm64) echo "aarch64-unknown-linux-musl" ;;
        armv7l)        echo "armv7-unknown-linux-musleabihf" ;;
        *)             die "Unsupported architecture: $arch" ;;
    esac
}

# ── Write binary to /usr/local/bin ───────────────────────────────────────────
write_binary() {
    local src="$1"
    [[ -f "$src" ]] || die "Binary not found: $src"
    $SUDO install -m 755 "$src" "$INSTALL_BIN"
    ok "Installed → ${INSTALL_BIN}"
}

# ── Verify installation ───────────────────────────────────────────────────────
verify_install() {
    local ver
    if command -v hydradesk &>/dev/null; then
        ver=$(hydradesk --version 2>/dev/null || echo "unknown")
    elif [[ -x "$INSTALL_BIN" ]]; then
        ver=$("$INSTALL_BIN" --version 2>/dev/null || echo "unknown")
        warn "${INSTALL_BIN} is installed but /usr/local/bin may not be on your PATH."
    else
        die "Installation finished but the binary is missing."
    fi
    ok "Installed: ${ver}"
    notify_if_outdated "$ver"
    echo
    echo -e "  ${BOLD}Quick start:${RESET}"
    echo -e "    ${CYAN}hydradesk doctor${RESET}        — check system readiness"
    echo -e "    ${CYAN}sudo hydradesk setup${RESET}    — configure GNOME RDP for mstsc"
    echo -e "    ${CYAN}hydradesk status${RESET}        — show IP, service state, and recent RDP activity"
    echo -e "    ${CYAN}sudo hydradesk uninstall${RESET} — remove HydraDesk"
    echo
}

installed_binary() {
    if command -v hydradesk &>/dev/null; then
        command -v hydradesk
        return 0
    fi
    if [[ -x "$INSTALL_BIN" ]]; then
        printf '%s\n' "$INSTALL_BIN"
        return 0
    fi
    return 1
}

handle_existing_install() {
    local installed ver answer

    if [[ "${HYDRADESK_FORCE_REINSTALL:-}" == "1" ]]; then
        return 1
    fi

    installed="$(installed_binary || true)"
    [[ -n "$installed" ]] || return 1

    ver=$("$installed" --version 2>/dev/null || echo "unknown")
    warn "HydraDesk is already installed: ${installed}"
    warn "Installed version: ${ver}"

    if [[ ! -r /dev/tty || ! -w /dev/tty ]]; then
        warn "No interactive terminal found; leaving the existing install in place."
        warn "Set HYDRADESK_FORCE_REINSTALL=1 to reinstall/update non-interactively."
        "$installed" status 2>/dev/null || true
        return 0
    fi

    echo > /dev/tty
    echo "  Choose what to do:" > /dev/tty
    echo "    [s] run setup again (credentials/services)" > /dev/tty
    echo "    [r] reinstall/update HydraDesk" > /dev/tty
    echo "    [t] show status" > /dev/tty
    echo "    [q] quit" > /dev/tty
    printf "  Selection [q]: " > /dev/tty
    read -r answer < /dev/tty || answer=""

    case "$answer" in
        s|S|setup|SETUP)
            run_setup_after_install
            return 0
            ;;
        r|R|reinstall|REINSTALL|update|UPDATE)
            info "Reinstall/update selected."
            return 1
            ;;
        t|T|status|STATUS)
            "$installed" status || true
            return 0
            ;;
        *)
            info "Leaving existing HydraDesk install unchanged."
            return 0
            ;;
    esac
}

# ── Run setup after install ───────────────────────────────────────────────────
run_setup_after_install() {
    if [[ "${HYDRADESK_INSTALL_ONLY:-}" == "1" ]]; then
        warn "Skipping setup because HYDRADESK_INSTALL_ONLY=1."
        echo -e "    Run ${CYAN}sudo hydradesk setup${RESET} later to configure credentials and RDP."
        return
    fi

    echo
    info "Starting HydraDesk setup..."
    info "You will be prompted for the RDP username and password."
    handle_rdp_conflicts

    if [[ -r /dev/tty && -w /dev/tty ]]; then
        $SUDO "$INSTALL_BIN" setup < /dev/tty
    else
        warn "No interactive terminal found; setup will continue non-interactively."
        warn "Pass credentials later with: printf '%s' 'STRONG_PASSWORD' | sudo hydradesk setup --username USER --password-stdin"
        $SUDO "$INSTALL_BIN" setup
    fi
}

# -- RDP conflict detection ---------------------------------------------------
is_pkg_installed() {
    if command -v dpkg-query &>/dev/null; then
        dpkg-query -W -f='${Status}' "$1" 2>/dev/null | grep -q "install ok installed" && return 0
    fi
    if command -v rpm &>/dev/null; then
        rpm -q "$1" &>/dev/null && return 0
    fi
    return 1
}

service_exists() {
    command -v systemctl &>/dev/null || return 1
    systemctl list-unit-files "$1" &>/dev/null
}

rdp_conflict_summary() {
    local found=()
    if is_pkg_installed xrdp || service_exists xrdp.service; then
        found+=("xrdp")
    fi
    if is_pkg_installed xorgxrdp; then
        found+=("xorgxrdp")
    fi
    printf '%s\n' "${found[@]}"
}

port_3389_listener() {
    if command -v ss &>/dev/null; then
        ss -ltnp 2>/dev/null | awk '$4 ~ /:3389$/ {print; found=1} END {exit found ? 0 : 1}'
    elif command -v lsof &>/dev/null; then
        lsof -nP -iTCP:3389 -sTCP:LISTEN 2>/dev/null
    else
        return 1
    fi
}

remove_xrdp_stack() {
    info "Stopping XRDP services..."
    $SUDO systemctl disable --now xrdp xrdp-sesman 2>/dev/null || true

    case "$(pkg_mgr)" in
        apt) info "Removing XRDP packages..."; $SUDO apt-get remove -y -q xrdp xorgxrdp ;;
        dnf) info "Removing XRDP packages..."; $SUDO dnf remove -y xrdp xorgxrdp ;;
        yum) info "Removing XRDP packages..."; $SUDO yum remove -y xrdp xorgxrdp ;;
        *)   warn "No supported package manager; stopped XRDP services but did not remove packages." ;;
    esac
}

handle_rdp_conflicts() {
    local conflicts listener answer
    conflicts="$(rdp_conflict_summary | paste -sd ', ' -)"

    if [[ -n "$conflicts" ]]; then
        warn "Detected another RDP stack: ${conflicts}."
        warn "HydraDesk uses GNOME Remote Desktop on port 3389, so XRDP can conflict."

        if [[ -r /dev/tty && -w /dev/tty ]]; then
            printf "  Remove XRDP and use HydraDesk instead? [y/N]: " > /dev/tty
            read -r answer < /dev/tty || answer=""
            case "$answer" in
                y|Y|yes|YES)
                    remove_xrdp_stack
                    ;;
                *)
                    warn "Leaving XRDP installed. HydraDesk setup may fail if port 3389 is already in use."
                    ;;
            esac
        else
            warn "No interactive terminal available, so XRDP was not removed."
            case "$(pkg_mgr)" in
                apt) warn "Run: sudo apt remove xrdp xorgxrdp" ;;
                dnf) warn "Run: sudo dnf remove xrdp xorgxrdp" ;;
                yum) warn "Run: sudo yum remove xrdp xorgxrdp" ;;
                *)   warn "Remove the xrdp/xorgxrdp packages with your package manager." ;;
            esac
        fi
    fi

    listener="$(port_3389_listener || true)"
    if [[ -n "$listener" ]] && ! grep -qi "gnome-remote-desktop" <<<"$listener"; then
        warn "Port 3389 is already listening:"
        echo "$listener"
        warn "If this is not HydraDesk/GNOME Remote Desktop, stop it before setup."
    fi
}

# ── Ensure git ────────────────────────────────────────────────────────────────
ensure_git() {
    command -v git &>/dev/null && return
    info "git not found — installing..."
    case "$(pkg_mgr)" in
        apt) $SUDO apt-get update -qq && $SUDO apt-get install -y -q git ;;
        dnf) $SUDO dnf install -y git ;;
        yum) $SUDO yum install -y git ;;
        *)   die "git not found and no supported package manager (apt/dnf). Install git and retry." ;;
    esac
}

# ── Install Rust via rustup ───────────────────────────────────────────────────
install_rust() {
    info "Rust not found — installing via rustup (this may take a minute)..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
        | sh -s -- -y --no-modify-path --profile minimal
    # shellcheck disable=SC1090
    source "${HOME}/.cargo/env" 2>/dev/null || true
    export PATH="${HOME}/.cargo/bin:${PATH}"
}

ensure_cargo() {
    command -v cargo &>/dev/null && return
    install_rust
    command -v cargo &>/dev/null || die "Rust/cargo still not found after install attempt."
}

# ── Build from source in the current directory (cloned repo) ──────────────────
install_from_source() {
    info "Building from source — this may take a few minutes on slow hardware..."
    ensure_cargo
    if [[ -d "./cli" ]]; then
        cargo build --release --manifest-path ./cli/Cargo.toml
        write_binary "$(find_built_binary "." || true)"
    elif [[ -f "./Cargo.toml" ]]; then
        cargo build --release
        write_binary "$(find_built_binary "." || true)"
    else
        die "No Cargo.toml found. Run from the HydraDesk repository root."
    fi
}

# ── Clone the repo to a temp dir and build (for the piped one-liner) ──────────
install_via_clone_build() {
    ensure_git
    ensure_cargo
    local tmp
    tmp=$(mktemp -d)
    # shellcheck disable=SC2064
    trap "rm -rf '$tmp'" EXIT
    info "Cloning ${REPO}..."
    git clone --depth 1 "https://github.com/${REPO}.git" "$tmp/HydraDesk" \
        || die "git clone failed (is the repo public and the name correct?)"
    info "Building (a few minutes on slow hardware)..."
    cargo build --release --manifest-path "$tmp/HydraDesk/cli/Cargo.toml" \
        || die "build failed"
    write_binary "$(find_built_binary "$tmp/HydraDesk" || true)"
}

# ── Try the pre-built release (non-fatal; returns 1 if unavailable) ───────────
try_prebuilt() {
    local target artifact url tmp
    target=$(detect_target) || return 1
    artifact="hydradesk-${target}"

    if [[ "$VERSION" == "latest" ]]; then
        url="https://github.com/${REPO}/releases/latest/download/${artifact}"
    else
        url="https://github.com/${REPO}/releases/download/${VERSION}/${artifact}"
    fi

    info "Looking for a pre-built binary: ${artifact}"
    tmp=$(mktemp)
    if command -v curl &>/dev/null; then
        curl -fsSL "$url" -o "$tmp" 2>/dev/null || { rm -f "$tmp"; return 1; }
    elif command -v wget &>/dev/null; then
        wget -qO "$tmp" "$url" 2>/dev/null || { rm -f "$tmp"; return 1; }
    else
        rm -f "$tmp"; return 1
    fi
    [[ -s "$tmp" ]] || { rm -f "$tmp"; return 1; }
    chmod +x "$tmp"
    write_binary "$tmp"
    rm -f "$tmp"
}

# ── Uninstall ─────────────────────────────────────────────────────────────────
uninstall_all() {
    info "Uninstalling HydraDesk..."

    # Prefer the binary's own teardown if available (cleanest).
    if command -v hydradesk &>/dev/null; then
        $SUDO hydradesk uninstall || warn "binary uninstall returned an error; continuing with manual cleanup"
    fi

    local target_user target_home uid
    target_user="${SUDO_USER:-$USER}"
    target_home=$(getent passwd "$target_user" 2>/dev/null | cut -d: -f6)
    target_home="${target_home:-$HOME}"
    uid=$(id -u "$target_user" 2>/dev/null || echo "")

    # Stop + disable the virtual display service.
    $SUDO systemctl stop hydradesk-display.service 2>/dev/null || true
    $SUDO systemctl disable hydradesk-display.service 2>/dev/null || true

    # Best-effort: turn off RDP in the user session.
    if [[ -n "$uid" ]]; then
        $SUDO -u "$target_user" \
            env XDG_RUNTIME_DIR="/run/user/$uid" \
                DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$uid/bus" \
            grdctl rdp disable 2>/dev/null || true
    fi

    # Remove everything HydraDesk installed.
    $SUDO rm -f /usr/local/bin/hydradesk-display-init
    $SUDO rm -f /etc/systemd/system/hydradesk-display.service
    $SUDO rm -rf /usr/local/share/hydradesk
    for f in \
        "$target_home/.local/bin/hydradesk-rdp-init" \
        "$target_home/.config/autostart/hydradesk-rdp.desktop" \
        "$target_home/.local/bin/hydradesk-lock-watch" \
        "$target_home/.config/autostart/hydradesk-lock.desktop"
    do
        rm -f "$f" 2>/dev/null || $SUDO rm -f "$f"
    done
    $SUDO systemctl daemon-reload 2>/dev/null || true
    $SUDO rm -f "$INSTALL_BIN"

    ok "HydraDesk removed."
    echo
    local rm_hint
    case "$(pkg_mgr)" in
        apt) rm_hint="apt remove gnome-remote-desktop" ;;
        dnf) rm_hint="dnf remove gnome-remote-desktop" ;;
        yum) rm_hint="yum remove gnome-remote-desktop" ;;
        *)   rm_hint="remove gnome-remote-desktop with your package manager" ;;
    esac
    echo -e "  ${BOLD}Left in place on purpose:${RESET}"
    echo -e "    • GDM auto-login (edit $(gdm_conf_path) → AutomaticLoginEnable=false to disable)"
    echo -e "    • gnome-remote-desktop package (${rm_hint} to remove)"
    echo
}

# ── Main ──────────────────────────────────────────────────────────────────────
main() {
    local mode="${1:-auto}"
    print_banner
    info "HydraDesk Installer"
    local src_ver
    src_ver=$(local_source_version || echo "")
    [[ -n "$src_ver" ]] && info "Version: v${src_ver}"
    info "Architecture: $(uname -m)"

    if [[ "$mode" != "--uninstall" && "$mode" != "uninstall" ]]; then
        if handle_existing_install; then
            return
        fi
    fi

    case "$mode" in
        --uninstall|uninstall)
            uninstall_all
            return
            ;;
        --build)
            if [[ -d ./cli || -f ./Cargo.toml ]]; then
                install_from_source
            else
                install_via_clone_build
            fi
            ;;
        auto)
            if built="$(find_built_binary "." || true)" && [[ -n "$built" ]]; then
                info "Found local build: ${built}"
                write_binary "$built"
            elif [[ -d ./cli || -f ./Cargo.toml ]]; then
                install_from_source
            else
                # Piped via curl with no local repo: prefer a release, else build.
                if try_prebuilt; then
                    :
                else
                    warn "No pre-built release found — building from source instead."
                    install_via_clone_build
                fi
            fi
            ;;
        *)
            die "Unknown argument: $mode. Use --build, --uninstall, or no argument."
            ;;
    esac
    verify_install
    run_setup_after_install
}

main "$@"
