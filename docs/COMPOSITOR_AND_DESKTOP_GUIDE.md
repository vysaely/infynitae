# Infynitae Architecture & Standalone Desktop Blueprint

A complete reference guide for building, customizing, or creating your own standalone, ultra-low-latency Wayland compositor and desktop environment from scratch.

---

## 1. High-Level Architecture Overview

Infynitae is designed around a three-tier architecture:

```
┌────────────────────────────────────────────────────────────────────────┐
│                        1. APPLICATION LAYER                            │
│  Native Wayland Clients  │  Electron (Ozone)  │  Qt5/6  │  X11 (Steam) │
└─────────────────────────────────┬──────────────────────────────────────┘
                                  │ Wayland Protocol / X11 Bridge
┌─────────────────────────────────▼──────────────────────────────────────┐
│                    2. COMPOSITOR & IPC CORE (Rust)                     │
│  ┌───────────────────────┐             ┌────────────────────────────┐  │
│  │     infynitae-core    │◄────────────┤      infynitae-ipc         │  │
│  │ (Spatial Model, Zero  │             │ (Unix Socket JSON-RPC API) │  │
│  │  Alloc Hot Path, Safe)│             └─────────────▲──────────────┘  │
│  └───────────┬───────────┘                           │                 │
│              ▼                                       │                 │
│  ┌───────────────────────┐             ┌─────────────┴──────────────┐  │
│  │   infynitae-wayland   │             │   infynitae-backend-*      │  │
│  │  (Smithay Protocol    │◄────────────┤  (DRM/KMS Console, Nested  │  │
│  │        Adapter)       │             │   Winit, Headless Test)    │  │
│  └───────────────────────┘             └────────────────────────────┘  │
└─────────────────────────────────┬──────────────────────────────────────┘
                                  │ Linux Subsystems
┌─────────────────────────────────▼──────────────────────────────────────┐
│                  3. COMPANION RUNTIME STACK (live/)                    │
│  • Session Supervisor (infynitae-session): TTY lock, crash recovery    │
│  • Environment (env.sh): Decoupling, GPU routing, toolkit hints        │
│  • Universal Action Dispatcher (launch.sh): App shortcuts & OSDs       │
│  • Daemonless Settings Engine (settings.sh): GTK/Qt/Cursor sync        │
│  • Hardware & Driver Setup (setup-drivers.sh): Vulkan, 32-bit Steam    │
│  • Native Portal & Polkit (infynitae-portal, infynitae-polkit)         │
└────────────────────────────────────────────────────────────────────────┘
```

---

## 2. Component Inventory & Directory Layout

Every component in Infynitae has a single, strictly separated responsibility:

| Component / Path | Language | Purpose |
| :--- | :--- | :--- |
| `crates/infynitae-core` | Rust | Pure spatial logic, window clustering, camera transformations, and hit-testing. Zero dependencies (std only), `#![forbid(unsafe_code)]`, and zero allocations on input/render paths. |
| `crates/infynitae-wayland` | Rust | Wayland protocol adapter built on Smithay 0.7.0. Translates compositor effects into Wayland wire protocol events. |
| `crates/infynitae-backend-drm` | Rust | Direct console hardware backend (DRM/KMS, GBM, EGL, Libinput, Libseat). |
| `crates/infynitae-backend-winit` | Rust | Nested window development backend for instant testing inside an existing desktop. |
| `crates/infynitae-backend-headless` | Rust | Virtual offscreen backend used for automated pixel and architecture regression testing. |
| `crates/infynitae-ipc` | Rust | Trusted Unix socket IPC server providing remote command execution and state queries. |
| `crates/infynitae` | Rust | Composition root: parses CLI flags, initializes ports and selected backend. |
| `live/session.sh` | Shell | Main session startup script: restores display, synchronizes PipeWire/portals, and starts user daemons. |
| `live/env.sh` | Shell | Environment isolation: decouples from GNOME/KDE, sets toolkit hints (Qt, GTK, Electron, Java), and configures GPU drivers. |
| `live/launch.sh` | Shell | Central shortcut dispatcher for terminal, launcher, file manager, screenshots, audio, and brightness OSDs. |
| `live/settings.sh` | Shell | Zero-daemon settings engine: writes GTK3/4 `settings.ini`, Qt `kdeglobals`, and cursor themes instantly to disk. |
| `live/setup-drivers.sh` | Bash | Hardware detector: scans PCI/DRM bus, enables `[multilib]`, and installs 32-bit & 64-bit Vulkan/Mesa drivers. |
| `live/portal/infynitae-portal.c` | C / GIO | Standalone XDG Desktop Portal backend implementing `org.freedesktop.impl.portal.GlobalShortcuts`. |
| `live/polkit/infynitae-polkit.c` | C / GTK3 | Minimal native Polkit authentication dialog for administrative prompts. |
| `install.sh` | Bash | Zero-touch automated installer for Arch Linux. |
| `update.sh` | Bash | Live update script using atomic inode installation (`install -m 755`) without process interruptions. |

---

## 3. Essential System Dependencies (Arch Linux)

To run a fully functional, out-of-the-box Wayland desktop environment without inheriting desktop bloat, the following packages provide all core capabilities:

### Core Display & Input
* `base-devel`: Essential compilation tools (GCC, Make, pkg-config).
* `libinput`: Multi-touch, touchpad gestures, and raw mouse input.
* `seatd` / `systemd-logind`: Secure unprivileged access to DRM and input devices.
* `xorg-xwayland`: X11 client compatibility server.
* `xwayland-satellite`: Dedicated Xwayland supervisor ensuring zero idle RAM and clean isolation.

### Graphics, Mesa & Vulkan
* `mesa`, `lib32-mesa`: OpenGL and Gallium driver stack.
* `vulkan-icd-loader`, `lib32-vulkan-icd-loader`: Vulkan hardware dispatchers.
* `vulkan-radeon` / `lib32-vulkan-radeon`: AMD Vulkan driver (RADV).
* `vulkan-intel` / `lib32-vulkan-intel`: Intel Vulkan driver (ANV).
* `nvidia-utils` / `lib32-nvidia-utils` / `egl-wayland`: NVIDIA proprietary Wayland drivers.

### Audio & Multimedia Stack
* `pipewire`: Next-generation low-latency multimedia routing daemon.
* `wireplumber`: Session manager for PipeWire.
* `pipewire-pulse`: PulseAudio emulation layer (required for Steam, Discord, Spotify, Chrome, and games).
* `playerctl`: Hardware media keys (`Play/Pause`, `Next`, `Previous`).

### Portals, Polkit & System Integration
* `xdg-desktop-portal`: Front-end portal dispatch service.
* `xdg-desktop-portal-wlr`: wlroots-compatible screencopy and screenshot backend.
* `xdg-desktop-portal-gtk`: Standard file chooser and app chooser dialog backend.
* `polkit`: Privilege authorization daemon.

### Desktop Utilities & Typography
* `alacritty` / `kitty` / `foot`: GPU-accelerated Wayland terminal.
* `fuzzel`: Minimal, instant C-based Wayland application launcher.
* `swaylock`: Screen locker.
* `libnotify`: Sends desktop notifications (`notify-send`) for volume and brightness OSDs.
* `brightnessctl`: Laptop screen brightness adjustments.
* `swaybg`: High-performance, low-memory static wallpaper daemon.
* `grim` & `slurp`: Screen and interactive region capture tools.
* `wl-clipboard`: Wayland clipboard utilities (`wl-copy`, `wl-paste`).
* `wl-clip-persist`: Keeps clipboard buffers alive when parent applications exit.
* `ttf-dejavu`: Universal clean monospace and sans-serif fonts to prevent missing glyphs.
* `qt5-wayland` & `qt6-wayland`: Wayland platform plugins for Qt applications.

---

## 4. Toolkit Compatibility Matrix

To prevent crashes, fuzzy fonts, and scaling artifacts, [`live/env.sh`](file:///home/vysaely/infynitae/live/env.sh) configures every major toolkit as follows:

| Toolkit / Runtime | Environment Configuration | Why It Is Necessary |
| :--- | :--- | :--- |
| **Qt 5 & Qt 6** | `QT_QPA_PLATFORM="wayland;xcb"`<br>`QT_WAYLAND_DISABLE_WINDOWDECORATION="1"`<br>`QT_AUTO_SCREEN_SCALE_FACTOR="1"` | Ensures Qt runs natively on Wayland, but cleanly falls back to X11 (`xcb`) if an app lacks Wayland plugins instead of crashing. Disables redundant titlebars. |
| **GTK 3 & GTK 4** | `GDK_BACKEND="wayland,x11,*"`<br>`unset GTK_MODULES`<br>`unset GTK_RC_FILES` | Forces native Wayland rendering with X11 fallback. Unsets legacy host desktop modules that can cause crashes. |
| **Electron & Chromium** | `ELECTRON_OZONE_PLATFORM_HINT="auto"` | Forces Discord, VS Code, Slack, Obsidian, Chrome, and Brave to use native Wayland, eliminating Xwayland scaling blur and input lag. |
| **Java GUI (AWT/Swing)**| `_JAVA_AWT_WM_NONREPARENTING="1"` | Fixes the infamous "gray blank window" bug in non-reparenting tiling/canvas compositors for IntelliJ, PyCharm, Minecraft, and Ghidra. |
| **Mozilla Firefox** | `MOZ_ENABLE_WAYLAND="1"` | Enables native Wayland rendering, smooth kinetic touchpad scrolling, and hardware WebGL/VAAPI acceleration. |
| **SDL 2 & Games** | `SDL_VIDEODRIVER="wayland,x11"` | Native Wayland rendering for SDL2 games with seamless fallback to X11 for legacy titles. |
| **NVIDIA GPU** | `LIBVA_DRIVER_NAME="nvidia"`<br>`__GLX_VENDOR_LIBRARY_NAME="nvidia"` | Automatically routes video decode and OpenGL/Vulkan to the NVIDIA proprietary driver when `/dev/nvidia0` is detected. |

---

## 5. How to Build Your Own Compositor & Desktop Environment

Follow these steps to customize Infynitae or build your own standalone Wayland environment:

### Step 1: Design the Spatial & Camera Model
In `crates/infynitae-core`, maintain a clear separation between:
1. **Canvas Coordinates**: Fixed-point 64-bit coordinates representing where windows live in world space.
2. **Camera Transformations**: Scale (zoom) and translation `(x, y)` mapping canvas coordinates to physical output pixels.
3. **Event Proofs**: Restrict authority mutations (like moving the camera or changing focus) to validated input events via `authority::Authorized` tokens.

### Step 2: Implement the Session Supervisor
Never run a compositor directly from a TTY without a supervisor. In `live/session.sh` and `infynitae-session`:
* Trap signals (`EXIT`, `INT`, `TERM`, `HUP`) to run `stty sane` and `tput cnorm`.
* Monitor process exit times: if the compositor crashes within 3 seconds multiple times in a row, break the loop and drop to a rescue shell rather than locking the user out.

### Step 3: Integrate with Display Managers & Login
* Create `/usr/share/wayland-sessions/<your-compositor>.desktop`:
  ```ini
  [Desktop Entry]
  Name=MyCompositor
  Comment=Custom Wayland Compositor
  Exec=mycompositor-session
  Type=Application
  DesktopNames=mycompositor;wlroots
  ```
* Symlink your session binary into both `/usr/local/bin` and `/usr/bin` so display managers like GDM, SDDM, Ly, and greetd find it regardless of how their systemd PATH is restricted.

### Step 4: Zero-Daemon Settings Management
Avoid running heavy background configuration daemons (like `xfconfd` or `dconf-service`):
* Directly generate and write static configuration files to:
  * GTK 3/4: `~/.config/gtk-3.0/settings.ini` and `~/.config/gtk-4.0/settings.ini`
  * Qt: `~/.config/kdeglobals`
  * X11/Xwayland Cursor: `~/.icons/default/index.theme`
* Update D-Bus / systemd environments on reload using:
  ```bash
  dbus-update-activation-environment --systemd XCURSOR_THEME XCURSOR_SIZE
  ```

### Step 5: Implement Safe Live Updating
When compiling and installing binaries on a running desktop:
* Never use `cp -f target/release/binary /destination`: Linux returns `ETXTBUSY` ("Text file busy") when truncating an open binary file descriptor.
* Always use `install -m 755 target/release/binary /destination`: `install` unlinks the destination inode first, allowing running processes to maintain their existing file handle while new executions immediately use the updated binary.

---

## 6. Testing & Development Workflows

### Nested Window Testing
Test your changes in a window inside your current desktop without switching TTYs:
```bash
cargo build
./target/debug/infynitae --nested --width 1600 --height 900
```

### Headless Verification & Architecture Probes
Run the negative-control probe suite to ensure no architectural rules or unsafe policies were broken:
```bash
./tools/verify-architecture
cargo test --workspace
```

### Live Reloading
* Press `Super + Shift + Backspace` inside the compositor to reload configuration, helper scripts, and themes instantly without restarting client windows.
* Use `infynitae-update` anytime to pull upstream commits, rebuild optimized release binaries, and install them live.
