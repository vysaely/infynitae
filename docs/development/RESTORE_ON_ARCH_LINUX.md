# Infynitae Native — Workspace Restoration Guide (Arch Linux / CachyOS)

**Non-normative guide**: This document provides operational setup and restoration instructions for developers and coding agents running on Arch Linux or CachyOS. It is not an architectural authority. Canonical architectural definitions live under `docs/foundation/`.

---

## 1. Prerequisites (Arch Linux / CachyOS)

Install required base development tools, libraries, and runtime dependencies:

```sh
sudo pacman -S --needed \
    base-devel \
    git \
    github-cli \
    zstd \
    python \
    pkgconf \
    rustup \
    wayland \
    libxkbcommon \
    libinput \
    libdrm \
    mesa
```

`rustup` conflicts with the distro `rust` package; pacman offers to remove `rust`. Use rustup: on CachyOS the distro `rust` (from `[extra]`) can break against CachyOS's rebuilt `llvm-libs`.

Not needed until later phases (the verifier reports them but does not require them): `wayland-protocols` (system protocol XML, reference only) and `seatd` (provides `libseat`; Phase G DRM session backend).

Configure Rust stable toolchain:

```sh
rustup default stable
```

---

## 2. Workspace Restoration

Choose either **Pathway A** (from GitHub remote) or **Pathway B** (from offline portable archive).

### Pathway A: From GitHub Remote

```sh
# Clone repository to desired location
git clone https://github.com/vysaely/infynitae.git ~/infynitae-native
cd ~/infynitae-native
```

### Pathway B: From Portable Archive

If using the offline portable backup bundle:

```sh
# Extract archive into home directory
tar --zstd -xvf ~/Infynitae-Portable.tar.zst -C ~

# Move or symlink project directory to working location
mv ~/Infynitae-Portable/project/infynitae ~/infynitae-native
cd ~/infynitae-native
```

---

## 3. Environment & Integrity Verification

Run the non-destructive environment verifier and test suites:

```sh
# 1. Verify complete toolchain and environment availability
./tools/verify-development-environment

# 2. Run canonical foundation architecture checker
./tools/check-foundation-docs

# 3. Run checker negative-control self-test
./tools/check-foundation-docs --self-test

# 4. Run C1 compile-time authority spike verification
./experiments/c1-authority-spike/verify.sh
```

Expected verification outcomes:
- `verify-development-environment`: `VERIFICATION RESULT: ALL CHECKS PASSED`
- `check-foundation-docs`: `RESULT: PASS` (57 invariants, 20 formulas, 51 obligations intact)
- `check-foundation-docs --self-test`: `SELF-TEST: PASS (every seeded defect detected)`
- `experiments/c1-authority-spike/verify.sh`: `== result: PASS`

---

## 4. Agent Onboarding & Resumption Workflow

When launching Claude or AGY on a fresh CachyOS / Arch Linux boot:

```sh
cd ~/infynitae-native
# Launch agent with workspace access (e.g. AGY CLI or Claude Code)
agy --dangerously-skip-permissions
```

Direct the agent to:
1. Read `docs/development/START_HERE_FOR_AGENT.md`.
2. Inspect `docs/archive/CHECKPOINT.md` to confirm the active gate.
3. Confirm that the current gate is:
   - **Phase E: PASSED** (`audit/phase-e-final/VERDICT.md`).
   - **Phase F: begun**; INC-0 implemented, pending independent review.
4. **Do not begin any increment** until the project owner explicitly gives the go-ahead in the session.

---

## 5. Hard Safety Rules

- **Execution Class**: All initial testing is HEADLESS. Do not launch live compositors on DRM/KMS or against active desktop sessions.
- **System Integrity**: Do not touch live compositor sessions, DRM nodes, VT state, system packages, or Rust toolchain settings.
- **Implementation Gate**: Each increment needs explicit owner authorization; `tools/verify-architecture` must pass before and after every change.
