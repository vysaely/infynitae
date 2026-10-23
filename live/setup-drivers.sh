#!/usr/bin/env bash
# Universal Hardware Graphics & Xwayland Driver Detector for Infynitae
# Detects AMD, Intel, NVIDIA, and Virtualized GPUs, and installs the required
# Vulkan, 32-bit (multilib), and Xwayland drivers.
set -e

echo "=========================================================="
echo "    INFYNITAE - UNIVERSAL GRAPHICS & DRIVER SETUP"
echo "=========================================================="

# 1. Ensure pciutils is available for accurate detection
if ! command -v lspci >/dev/null 2>&1; then
    echo "==> Installing pciutils for hardware detection..."
    sudo pacman -S --needed --noconfirm pciutils
fi

# 2. Ensure [multilib] repository is enabled in pacman.conf (required for 32-bit drivers & Steam)
if ! grep -q "^\[multilib\]" /etc/pacman.conf; then
    echo "==> Enabling [multilib] repository in /etc/pacman.conf..."
    sudo sed -i '/\[multilib\]/,/Include/ s/^#//' /etc/pacman.conf
    sudo pacman -Sy
fi

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

if echo "$GPU_OUTPUT" | grep -iqE 'amd|ati|radeon|1002:' || echo "$SYS_DRM_VENDORS" | grep -iq '0x1002'; then
    DETECTED_AMD=true
fi

if echo "$GPU_OUTPUT" | grep -iqE 'intel|8086:' || echo "$SYS_DRM_VENDORS" | grep -iq '0x8086'; then
    DETECTED_INTEL=true
fi

if echo "$GPU_OUTPUT" | grep -iqE 'nvidia|10de:' || echo "$SYS_DRM_VENDORS" | grep -iq '0x10de'; then
    DETECTED_NVIDIA=true
fi

if echo "$GPU_OUTPUT" | grep -iqE 'virtio|vmware|virtualbox|qemu|1af4:|15ad:|80ee:|1b36:' || \
   echo "$SYS_DRM_VENDORS" | grep -iqE '0x1af4|0x15ad|0x80ee|0x1b36'; then
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

if [ "$DETECTED_NVIDIA" = true ]; then
    echo "  [✓] NVIDIA GPU detected"
    TARGET_PKGS+=(nvidia-utils lib32-nvidia-utils egl-wayland)
fi

if [ "$DETECTED_VM" = true ]; then
    echo "  [✓] Virtual Machine / VirtIO GPU detected"
    TARGET_PKGS+=(vulkan-virtio lib32-vulkan-virtio vulkan-swrast lib32-vulkan-swrast)
fi

if [ "$DETECTED_AMD" = false ] && [ "$DETECTED_INTEL" = false ] && [ "$DETECTED_NVIDIA" = false ] && [ "$DETECTED_VM" = false ]; then
    echo "  [!] Unknown/Generic GPU detected; adding universal software Vulkan rasterizer"
    TARGET_PKGS+=(vulkan-swrast lib32-vulkan-swrast)
fi

# Filter only uninstalled packages
MISSING=()
for pkg in "${TARGET_PKGS[@]}"; do
    if ! pacman -Qi "$pkg" >/dev/null 2>&1; then
        MISSING+=("$pkg")
    fi
done

if [ ${#MISSING[@]} -gt 0 ]; then
    echo "==> Missing driver packages to install: ${MISSING[*]}"
    if [ "$1" = "--dry-run" ]; then
        echo "==> [Dry-run] Command that would run: sudo pacman -S --needed --noconfirm ${MISSING[*]}"
    else
        sudo pacman -S --needed --noconfirm "${MISSING[@]}"
        echo "==> All required drivers successfully installed!"
    fi
else
    echo "==> All required graphics and Xwayland drivers are already installed."
fi
