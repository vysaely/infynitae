# Roadmap

What comes next, roughly in order. [STATUS.md](STATUS.md) says where things stand today.

## First: make what exists solid

- **Try on real hardware** what has only run in the test suite: touch on the canvas, rotation
  and auto-rotate, VRR from display tools, the laptop lid, monitor hotplug, the virtual screen,
  remote desktop through the portal, crash reports.
- **Fix what that turns up.**
- **Screens turned off from display tools** (`wlr-randr --off`, kanshi profiles): the screen
  leaves the layout and comes back where it was.
- **Pens and drawing tablets** (`tablet-v2`).

## Later: desktops on the canvas

An idea, not a plan yet: a cluster that works like a desktop (a workspace), placed on the
canvas.

- Each such cluster is one screen at 100% zoom: desktop 1, desktop 2, and so on.
- Super+Shift+1..9 jumps to it at 100% zoom, like switching workspaces.
- Inside it, windows tile Hyprland-style (dwindle) to fill the screen.
- What workspaces cannot do: zoom out and every desktop is laid out on the canvas, to pan
  between, arrange side by side, or drag a window from one to another.
- Later perhaps a scrolling desktop (niri, PaperWM): wider than a screen, scrolled sideways.

Open questions before a design:

1. Can a window float on the open canvas, outside any desktop?
2. Does a new window always join the desktop you are on?
3. Moving a window to desktop N: Super+Ctrl+1..9?
4. Two monitors: a desktop per monitor, each its own size?
5. Is every cluster a desktop, or only one you make into one (Super+T)?

Core is a specified state machine, so this starts with its rules in the foundation spec
(placement, closing, size limits, fullscreen, the overview), then keys and tests.

## Not planned yet

HDR, accessibility and screen readers, a second GPU, NVIDIA's explicit sync, packaging beyond
Arch Linux.
