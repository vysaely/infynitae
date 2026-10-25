# Independent Adversarial Probes and Mutation Testing

## 1. Adversarial Attack Suite Methodology

To verify the implementation independently of author-provided test harnesses, an isolated git worktree was established in scratch (`scratch/worktree`) and 8 targeted adversarial attack test suites were executed against the production codebase.

### Attack 1: Boundary Pixels and Half-Open Interval Containment
- **Objective:** Falsify interval handling at physical edges ($x_0, x_1$).
- **Vectors:**
  - $(x_0, y_0)$: Top-left boundary pixel. Asserted to hit when within surface input region.
  - $(x_1 - 1, y_1 - 1)$: Bottom-right inner pixel. Asserted to hit when within surface input region.
  - $(x_1, y_0)$: Right outer boundary pixel. Strictly excluded by half-open interval $[x_0, x_1)$.
  - $(x_0, y_1)$: Bottom outer boundary pixel. Strictly excluded by half-open interval $[y_0, y_1)$.
  - Sub-pixel probes: $x_0 \pm 0.0001, x_1 \pm 0.0001$.
- **Result:** PASS. Half-open intervals are strictly enforced.

### Attack 2: Non-Standard Fractional Scales and Display Modes
- **Objective:** Falsify edge parity beyond the 4 standard scales $\{1.0, 1.25, 1.5, 1.75\}$.
- **Vectors:** Scales $k \in \{1.1, 1.25, 1.333, 1.5, 1.667, 1.75, 2.0, 2.25\}$ across resolutions $1366\times 768$, $1920\times 1080$, $2560\times 1440$, $3840\times 2160$, and $1024\times 768$.
- **Result:** PASS. 100% agreement between Infynitae's `edge(x)` and Smithay's presented geometry. 0 mismatches.

### Attack 3: Complex Multi-Operation Input Regions
- **Objective:** Falsify nested islands, holes within islands, and negative coordinate bounds.
- **Vectors:** Ops: Add $(0,0,100,100)$ → Subtract $(20,20,60,60)$ → Add $(40,40,20,20)$ → Subtract $(45,45,10,10)$. Also negative offset regions $(-20,-20,40,40)$.
- **Result:** PASS. Request-order evaluation correctly resolved all nested boundaries.

### Attack 4: Large Coordinates ($2^{36}$ Boundary)
- **Objective:** Detect floating-point degradation near the $2^{36}$ Canvas limit.
- **Vectors:** Center at $2^{36} - 10000$ Canvas px ($\approx 6.87 \times 10^{10}$). Surface origins offset by 25 px.
- **Result:** PASS. Because $x - R$ is bounded near the viewport ($< 120$ px), $(x - R) \cdot k$ maintains full 53-bit IEEE 754 precision. Unproject/project roundtrip error was $< 10^{-4}$ px.

### Attack 5: Delivered Coordinate Clamping
- **Objective:** Detect coordinate overflow or clamping leakage into $[w, \infty)$.
- **Vectors:** Probed grid over surface boundaries at fractional scale $k = 1.75$.
- **Result:** PASS. Delivered coordinates strictly satisfy $0 \le \text{local}.x < w \cdot 2^{16}$ and $0 \le \text{local}.y < h \cdot 2^{16}$.

### Attack 6: Protocol M1 State Machine and Core Entry Refusal
- **Objective:** Force illegal transitions and verify Core refusal.
- **Vectors:**
  - Commit buffer before initial configure → Refused with `BufferBeforeConfigure`.
  - Connect second output → Refused with `EntryError::Unsupported`.
  - Reconfigure output → Refused with `EntryError::Unsupported`.
  - Disconnect output → Refused with `EntryError::Unsupported`.
  - Unmap → Snapshot dropped, anchor and identity preserved.
  - Re-map with immediate buffer → Refused with `BufferBeforeConfigure`.
  - Destroy → Purged from stage, frames, z-order; idempotent.
- **Result:** PASS. Core fails closed without mutating state.

### Attack 7: Multi-Surface Stacking and Hole Fall-Through
- **Objective:** Verify z-order priority, hole fall-through, and shadow exclusion.
- **Vectors:**
  - Root with CSD shadow (20 px) and hole $(20, 55, 20, 30)$ over subsurface S2 below.
  - Point in S1 above root → Hits S1.
  - Point in hole over S2 → Falls through to S2 below.
  - Point outside hole where W overlaps S2 → Hits W (W wins over S2 below).
  - Point in S2 outside W boundary → Hits S2.
- **Result:** PASS. Stacking and transparency fall-through behave exactly as specified.

### Attack 8: Popup Target Inward Rounding (M3, R8)
- **Objective:** Verify inward rounding across fractional scales.
- **Vectors:** Scales $k \in \{1.0, 1.167, 1.25, 1.333, 1.5, 1.75, 2.0\}$.
- **Result:** PASS. Target min edges ceiling-rounded; target max edges floor-rounded. Strictly inside viewport.

---

## 2. Expanded Mutation Testing Evaluation

The existing 18 mutation controls in `tools/verify-test-efficacy` were evaluated and passed (18/18 detected).
5 additional adversarial mutations (`ADV-MUT-1` through `ADV-MUT-5`) were authored and executed in the isolated worktree:

| Mutation ID | Mutation Description | Target Module | Test Filter | Expected | Actual | Finding |
|---|---|---|---|---|---|---|
| `ADV-MUT-1` | Popup geometry y offset added instead of subtracted | `snapshot.rs` | `t1_parity tst09_xsnap` | DETECTED | DETECTED | Verified |
| `ADV-MUT-2` | Unshown surfaces included in render list (omit `!shown continue`) | `render.rs` | `t1_parity tst09_xsnap` | DETECTED | **MISSED** | **INC1-F07** |
| `ADV-MUT-3` | Unproject swaps x and y coordinates | `projection.rs` | `t1_parity tst10_xhit` | DETECTED | **MISSED** | **INC1-F06** |
| `ADV-MUT-4` | Root tree extracted before popups | `wayland/snapshot.rs` | `t1_parity tst09_xsnap` | DETECTED | DETECTED | Verified |
| `ADV-MUT-5` | Initial anchor displaced from camera center | `protocol.rs` | `core m1_map_unmap` | DETECTED | DETECTED | Verified |

### Mutation Failure Analysis

1. **`ADV-MUT-2` Analysis:**
   In all existing T0 and T1 tests, all surfaces in test trees have `mapped: true`. Although `SurfaceTree::shown()` is unit-tested in isolation, no test runs `render_list` or `entries_for` on an unmapped surface. Removing `if !shown { continue; }` went undetected. (Logged as `INC1-F07`).

2. **`ADV-MUT-3` Analysis:**
   In `t1_parity.rs`, `hit_parity` constructs its oracle by calling `view.unproject(p)` to determine `q` for `eligible(e, q)`. The production `select(p)` also calls `view.unproject(p)`. Because both sides share the same helper, a coordinate transposition in `unproject` went undetected by `tst10_xhit`. (Logged as `INC1-F06`).
