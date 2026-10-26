#!/usr/bin/env bash
# Universal Hardware Graphics & Xwayland Driver Detector for Infynitae
# Detects AMD, Intel, NVIDIA, and Virtualized GPUs, and installs the required
# Vulkan, 32-bit (multilib), and Xwayland drivers.
set -e

DRY_RUN=false
[ "${1:-}" = "--dry-run" ] && DRY_RUN=true

# the shared, unattended pacman (waits for another package manager, replaces a conflicting
# package instead of stopping); next to this script in the repo, or in ~/infynitae when this is
# the installed ~/.local/bin/infynitae-drivers
for lib in "$(dirname "$0")/install-lib.sh" "$HOME/infynitae/live/install-lib.sh"; do
    if [ -f "$lib" ]; then
        # shellcheck disable=SC1090
        . "$lib"
        break
    fi
done
if ! command -v pacman_run >/dev/null 2>&1; then
    pacman_run() { sudo pacman "$@" --noconfirm --ask 4; }
fi

echo "=========================================================="
echo "    INFYNITAE - UNIVERSAL GRAPHICS & DRIVER SETUP"
echo "=========================================================="

# 1. Ensure pciutils is available for accurate detection
if ! command -v lspci >/dev/null 2>&1; then
    echo "==> Installing pciutils for hardware detection..."
    if [ "$DRY_RUN" = true ]; then
        echo "==> [Dry-run] would install pciutils"
    else
        pacman_run -S --needed pciutils
    fi
fi

# 2. Ensure [multilib] repository is enabled in pacman.conf (required for 32-bit drivers & Steam)
if ! grep -q "^\[multilib\]" /etc/pacman.conf; then
    if [ "$DRY_RUN" = true ]; then
        echo "==> [Dry-run] would enable [multilib] in /etc/pacman.conf"
    else
        echo "==> Enabling [multilib] repository in /etc/pacman.conf..."
        sudo sed -i '/^#\[multilib\]$/,/^#Include/ s/^#//' /etc/pacman.conf
        if ! grep -q "^\[multilib\]" /etc/pacman.conf; then
            echo "==> /etc/pacman.conf has no [multilib] section to enable; 32-bit drivers (Steam) are left out." >&2
        else
            pacman_run -Syu --needed
        fi
    fi
fi
MULTILIB=false
grep -q "^\[multilib\]" /etc/pacman.conf && MULTILIB=true

# 3. Detect graphics hardware
echo "==> Scanning PCI bus and DRM subsystems..."
GPU_OUTPUT=""
if command -v lspci >/dev/null 2>&1; then
    GPU_OUTPUT=$(lspci -nn | grep -iE 'vga|3d controller|display controller' 2>/dev/null || true)
fi

SYS_DRM_VENDORS=""
for v in /sys/class/drm/card*/device/vendor; do
    if [ -f "$v" ]; then
        SYS_DRM_VENDORS="$SYS_DRM_VENDORS $(cat "$v" 2>/dev/null || true)"
    fi
done

DETECTED_AMD=false
DETECTED_INTEL=false
DETECTED_NVIDIA=false
DETECTED_VM=false

if echo "$GPU_OUTPUT" | grep -iqE '\b(amd|ati|radeon)\b|\[1002:' || echo "$SYS_DRM_VENDORS" | grep -iqE '\b0x1002\b'; then
    DETECTED_AMD=true
fi

if echo "$GPU_OUTPUT" | grep -iqE '\bintel\b|\[8086:' || echo "$SYS_DRM_VENDORS" | grep -iqE '\b0x8086\b'; then
    DETECTED_INTEL=true
fi

if echo "$GPU_OUTPUT" | grep -iqE '\bnvidia\b|\[10de:' || echo "$SYS_DRM_VENDORS" | grep -iqE '\b0x10de\b'; then
    DETECTED_NVIDIA=true
fi

if echo "$GPU_OUTPUT" | grep -iqE '\b(virtio|vmware|virtualbox|qemu)\b|\[(1af4|15ad|80ee|1b36):' || \
   echo "$SYS_DRM_VENDORS" | grep -iqE '\b(0x1af4|0x15ad|0x80ee|0x1b36)\b'; then
    DETECTED_VM=true
fi

TARGET_PKGS=()

# Universal Wayland, Xwayland, Mesa, and Vulkan loaders
TARGET_PKGS+=(
    mesa
    lib32-mesa
    vulkan-icd-loader
    lib32-vulkan-icd-loader
    xorg-xwayland
    xwayland-satellite
)

echo "==> Hardware Detection Results:"
if [ "$DETECTED_AMD" = true ]; then
    echo "  [✓] AMD Radeon GPU detected"
    TARGET_PKGS+=(vulkan-radeon lib32-vulkan-radeon)
fi

if [ "$DETECTED_INTEL" = true ]; then
    echo "  [✓] Intel Graphics detected"
    TARGET_PKGS+=(vulkan-intel lib32-vulkan-intel intel-media-driver)
fi

NVIDIA_NOTE=""
if [ "$DETECTED_NVIDIA" = true ]; then
    echo "  [✓] NVIDIA GPU detected"
    TARGET_PKGS+=(nvidia-utils lib32-nvidia-utils egl-wayland libva-nvidia-driver)
    # the kernel driver: nvidia-utils is only its user-space half. The open modules (the only
    # ones in Arch's repositories now) need a Turing card or newer (RTX, GTX 16xx); an older
    # card needs a legacy driver from the AUR, and nouveau drives it until then.
    if ! pacman -Qq 2>/dev/null | grep -qE '^nvidia(-open)?(-dkms|-lts)?$|^nvidia-[0-9]+xx-dkms$'; then
        if echo "$GPU_OUTPUT" | grep -qE '\b(TU|GA|GH|AD|GB)[0-9]{3}'; then
            TARGET_PKGS+=(nvidia-open-dkms)
            # dkms builds it for every installed kernel: each needs its headers
            for k in $(pacman -Qq 2>/dev/null | grep -E '^linux(-lts|-zen|-hardened|-rt|-rt-lts)?$'); do
                TARGET_PKGS+=("$k-headers")
            done
        else
            # nouveau needs none of the NVIDIA driver's packages (exact names, not substrings)
            KEEP=()
            for pkg in "${TARGET_PKGS[@]}"; do
                case "$pkg" in
                    nvidia-utils | lib32-nvidia-utils | libva-nvidia-driver) ;;
                    *) KEEP+=("$pkg") ;;
                esac
            done
            TARGET_PKGS=("${KEEP[@]}")
            NVIDIA_NOTE="This NVIDIA card is older than Turing: Arch's driver no longer supports it. It runs on nouveau (slow, no reclocking); for full speed install a legacy driver from the AUR (e.g. nvidia-580xx-dkms)."
        fi
    fi
fi

if [ "$DETECTED_VM" = true ]; then
    echo "  [✓] Virtual Machine / VirtIO GPU detected"
    TARGET_PKGS+=(vulkan-virtio lib32-vulkan-virtio vulkan-swrast lib32-vulkan-swrast)
fi

if [ "$DETECTED_AMD" = false ] && [ "$DETECTED_INTEL" = false ] && [ "$DETECTED_NVIDIA" = false ] && [ "$DETECTED_VM" = false ]; then
    echo "  [!] Unknown/Generic GPU detected; adding universal software Vulkan rasterizer"
    TARGET_PKGS+=(vulkan-swrast lib32-vulkan-swrast)
fi

# Filter only uninstalled packages (32-bit ones only with multilib)
MISSING=()
for pkg in "${TARGET_PKGS[@]}"; do
    [ -z "$pkg" ] && continue
    case "$pkg" in lib32-*) [ "$MULTILIB" = true ] || continue ;; esac
    if ! pacman -Qi "$pkg" >/dev/null 2>&1; then
        MISSING+=("$pkg")
    fi
done

if [ ${#MISSING[@]} -gt 0 ]; then
    echo "==> Missing driver packages to install: ${MISSING[*]}"
    if [ "$DRY_RUN" = true ]; then
        echo "==> [Dry-run] Command that would run: sudo pacman -S --needed --noconfirm ${MISSING[*]}"
    else
        pacman_run -S --needed "${MISSING[@]}"
        echo "==> All required drivers successfully installed!"
    fi
else
    echo "==> All required graphics and Xwayland drivers are already installed."
fi

# NVIDIA: the compositor needs the driver's kernel modesetting (nvidia_drm modeset=1); current
# drivers turn it on, an older setup may not
if [ "$DETECTED_NVIDIA" = true ] && [ -z "$NVIDIA_NOTE" ] \
    && ! grep -rqs 'nvidia.drm.*modeset=1' /etc/modprobe.d /usr/lib/modprobe.d \
    && ! grep -qs 'nvidia.drm.modeset=1' /proc/cmdline; then
    if [ "$DRY_RUN" = true ]; then
        echo "==> [Dry-run] would turn on NVIDIA kernel modesetting (/etc/modprobe.d/infynitae-nvidia.conf)"
    else
        echo "options nvidia_drm modeset=1 fbdev=1" | sudo tee /etc/modprobe.d/infynitae-nvidia.conf >/dev/null
        echo "==> NVIDIA kernel modesetting turned on (after the reboot)"
    fi
fi
if [ -n "$NVIDIA_NOTE" ]; then
    echo "==> Note: $NVIDIA_NOTE"
fi
