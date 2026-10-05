# Prototype 01 Decision Ledger

Status: **Phase A classification** (master prompt A2). Revisited in Phase D (D7). Evidence IDs refer to
[FOUNDATION_EVIDENCE.md](FOUNDATION_EVIDENCE.md). Source identity: the hashes in
[`evidence/phase-a/prototype-source-hashes.txt`](evidence/phase-a/prototype-source-hashes.txt).
The prototype repository has no commits.

Legend: **REUSE** (take as-is) · **ADAPT** (keep idea, change form) · **REIMPLEMENT** (requirement
stands, prototype code unsuitable) · **DISCARD** (remove) · **DEFER** (not Foundation 01).

Overall: nothing in Prototype 01 is classified REUSE as code. What it contributes is **feasibility
evidence** (Smithay hosts a Stage-owned model without Space, nested GLES works, satellite connects) and a
small set of *ideas*.

| Area | Prototype decision | Class | Reason | Evidence |
|---|---|---|---|---|
| Stage representation | `HashMap<ElementId, StageElement>` + `Vec` z-order; `StageElement` owns `smithay::desktop::Window`; `canvas_pos` is a `pub` field | **ADAPT** | Stage-as-sole-authority is right (I1). Owning a Smithay type makes the domain Smithay-dependent (contradicts C1), and the `pub` field allows unencapsulated mutation. | EV-P05, EV-P16, EV-S03 |
| Element identity | Monotonic `u64`, never reused, no generation; lookup by surface is a linear scan | **ADAPT** | Never-reused IDs prevent ABA within a process. Whether monotonic or generational is chosen is B4. The surface→element mapping needs an O(1) association (e.g. surface user data), not a scan. | EV-P16 |
| Canvas numeric type | `i32` everywhere (not f64) | **REIMPLEMENT** | No numeric contract, no defined rounding points. Sub-pixel input is lost (EV-P10). B1 decides from first principles. | EV-P01, EV-P10 |
| Camera representation | `i32 {x, y}`, one global instance | **REIMPLEMENT** | One camera only. No output association, zoom or precision contract. | EV-P07 |
| Rendering projection | `screen = canvas_pos - camera`, done at draw time, surface origin as anchor, scale hard-coded 1.0 | **ADAPT** | Draw-time projection is the right shape (I8). Anchor, scale and output transform must be explicit (B2). | EV-P09 |
| Inverse hit testing | Rounds pointer, uses geometry-origin anchor, returns window-relative surface offset to Smithay | **REIMPLEMENT** | Disagrees with rendering (EV-P09) and violates Smithay's focus-location contract (EV-P08). I9 not met. | EV-P08, EV-P09 |
| Input / pan | Booleans `is_panning`, `super_pressed`, `pan_start_*`; per-event rounded delta; pointer focus cleared during pan | **REIMPLEMENT** | Explicit interaction state machine required (B10). Rounding drops small deltas. Relative-motion isolation untested. | EV-P10, EV-P15 |
| Pan instrumentation | `AtomicUsize` counters in compositor state; verify suite reports literals | **DISCARD** | Hard-coded results (EV-P02). Internal counters cannot prove client-observable behavior (F5). Replace with a client-side harness (X-1). | EV-P02, EV-P03 |
| Protocol-state organization | One `PrototypeState` holding Stage, Camera, all protocol states, seat, input booleans, metrics | **REIMPLEMENT** | Fine for a spike, but it is the "god struct" that layering (C1) must split. | — |
| Smithay `Window` ownership | Stage stores `Window` | **ADAPT** | Keep `Window` for surface-tree helpers (`bbox`, `send_frame`, `on_commit`). It belongs in the Smithay-facing layer, associated with an `ElementId`, not inside the pure Stage. | EV-S03 |
| Smithay `Space` | Not used | **REUSE (decision)** | Confirms Stage can own placement. Consequence: enter/leave must be driven by hand (EV-S04). | EV-P21, EV-S04 |
| Toplevel lifecycle | Create on `new_toplevel`, initial configure on first commit, **no destroy handling** | **REIMPLEMENT** | Stale elements after destroy (EV-P11). B5 lifecycle needed. | EV-P11 |
| Popups / subsurfaces | Popups ignored; subsurfaces rendered via `Window` | **REIMPLEMENT** | B6. | EV-P12 |
| Output enter/leave, scale | None | **REIMPLEMENT** | B8. | EV-P13, EV-S04, EV-S07 |
| Frame callbacks | Every element, every frame, `throttle = ZERO`, continuous redraw | **DISCARD** | Off-screen clients unthrottled; idle never idle. Opposite of the product requirement. | EV-P14, EV-S05 |
| Pointer constraints / relative pointer | Globals advertised, never serviced | **REIMPLEMENT** | Advertising unsupported behavior misleads clients (games). | EV-P15 |
| Winit coupling | Render loop inside winit closure; 1280×720 constant; `Flipped180` output transform | **REIMPLEMENT** behind backend boundary | B12. | EV-P17 |
| xwayland-satellite | Spawned as child, hard-coded personal path, fixed `:25`, killed on drop, no supervision | **ADAPT** | Strategy is sound (AS-2) and the connection was observed (EV-P18). Production needs discovery, display allocation and failure handling (C6/C8). | EV-P18 |
| Error handling | 17 `unwrap`/`expect`, 1 documented-by-convention `unsafe`; `assert!` self-check at startup | **REIMPLEMENT** | C8/C9. The `unsafe` `Generic::get_mut` dispatch pattern is the standard Smithay idiom and may be retained with a written safety note. | EV-P22 |
| Test architecture | 2 unit tests + startup self-check | **DISCARD** (keep the two property ideas) | Projection symmetry and pan-invariance become T0 property tests. Everything else starts from C11. | EV-P23 |
| Placement policy | Diagonal stagger by element count | **DEFER** | Level D UX. | — |
| Focus | Click → raise + keyboard focus; no `set_activated` | **REIMPLEMENT** | C4 focus model. Note `set_activated` would emit configures (activation state), legitimately not pan-induced. | — |
| Data device | `set_data_device_focus` on keyboard focus | **ADAPT** | Minimal correct clipboard plumbing. DnD is part of B10. | — |

---

## D7 effective projection (living; canonical `PL-*`)

Each row carries forward one row of the Phase A table above, which stays unchanged as history. The current class is unchanged from Phase A, and its justification cites the effective invariants and ADRs.

**No new decision is introduced here.** Where the Phase A reason named a Phase B/C section ("B4", "C4"), the effective rule is the cited invariant.

<!-- living:PL -->
### PL-01 — Stage representation
- **Class (Phase A, unchanged):** ADAPT
- **Effective consequence:** Stage is the sole spatial authority, owned by core without Smithay types
- **Justified by:** INV-STAGE-1, INV-STAGE-6, INV-ARCH-1
- **ADR:** ADR-01

### PL-02 — Element identity
- **Class (Phase A, unchanged):** ADAPT
- **Effective consequence:** `ElementId` NonZeroU64, never reused; `(InstanceId, ElementId)` externally; O(1) surface association
- **Justified by:** INV-STAGE-3
- **ADR:** —

### PL-03 — Canvas numeric type
- **Class (Phase A, unchanged):** REIMPLEMENT
- **Effective consequence:** Q16 i64 with rounding points
- **Justified by:** INV-COORD-1, INV-COORD-2, INV-COORD-3
- **ADR:** ADR-02

### PL-04 — Camera representation
- **Class (Phase A, unchanged):** REIMPLEMENT
- **Effective consequence:** centre-anchored cameras with zoom; created and bound only through the output-lifecycle path
- **Justified by:** INV-CAMERA-1, INV-CAMERA-3, INV-OUTPUT-2
- **ADR:** ADR-04, ADR-08

### PL-05 — Rendering projection
- **Class (Phase A, unchanged):** ADAPT
- **Effective consequence:** draw-time projection kept; lattice edges, f64 location
- **Justified by:** INV-RENDER-1, INV-STAGE-6
- **ADR:** ADR-02, ADR-03

### PL-06 — Inverse hit testing
- **Class (Phase A, unchanged):** REIMPLEMENT
- **Effective consequence:** physical-pixel hit selection in render order
- **Justified by:** INV-HIT-1, INV-COORD-6
- **ADR:** ADR-03

### PL-07 — Input / pan
- **Class (Phase A, unchanged):** REIMPLEMENT
- **Effective consequence:** ownership table, interaction state machine, direct-manipulation pan
- **Justified by:** INV-INPUT-1, INV-INPUT-3, INV-COORD-3
- **ADR:** ADR-07

### PL-08 — Pan instrumentation
- **Class (Phase A, unchanged):** DISCARD
- **Effective consequence:** client-observed measurement replaces internal counters
- **Justified by:** INV-CAMERA-4, INV-OBS-1
- **ADR:** —

### PL-09 — Protocol-state organization
- **Class (Phase A, unchanged):** REIMPLEMENT
- **Effective consequence:** five-crate split; core without Smithay
- **Justified by:** INV-ARCH-1
- **ADR:** ADR-08, ADR-12

### PL-10 — Smithay `Window` ownership
- **Class (Phase A, unchanged):** ADAPT
- **Effective consequence:** `Window` in the adapter, associated with `ElementId`
- **Justified by:** INV-ARCH-1, INV-STAGE-1
- **ADR:** ADR-01, ADR-13

### PL-11 — Smithay `Space`
- **Class (Phase A, unchanged):** REUSE (decision)
- **Effective consequence:** not used
- **Justified by:** INV-STAGE-1, INV-RENDER-3
- **ADR:** ADR-01

### PL-12 — Toplevel lifecycle
- **Class (Phase A, unchanged):** REIMPLEMENT
- **Effective consequence:** B5 lifecycle; idempotent destroy
- **Justified by:** INV-STAGE-4, INV-STAGE-5
- **ADR:** —

### PL-13 — Popups / subsurfaces
- **Class (Phase A, unchanged):** REIMPLEMENT
- **Effective consequence:** single origin function; own membership; ordered popup preemption
- **Justified by:** INV-COORD-6, INV-RENDER-2, INV-INPUT-10
- **ADR:** ADR-03, ADR-14

### PL-14 — Output enter/leave, scale
- **Class (Phase A, unchanged):** REIMPLEMENT
- **Effective consequence:** own per-surface membership; unified scale with epochs
- **Justified by:** INV-RENDER-3, INV-SCALE-1, INV-SCALE-3
- **ADR:** ADR-05

### PL-15 — Frame callbacks
- **Class (Phase A, unchanged):** DISCARD
- **Effective consequence:** withhold / kick / fallback; no continuous redraw
- **Justified by:** INV-FRAME-1, INV-FRAME-3, INV-IDLE-1
- **ADR:** ADR-06, ADR-12

### PL-16 — Pointer constraints / relative pointer
- **Class (Phase A, unchanged):** REIMPLEMENT
- **Effective consequence:** compositor activation policy; reconciliation; confinement clamp
- **Justified by:** INV-INPUT-4, INV-INPUT-5
- **ADR:** ADR-07

### PL-17 — Winit coupling
- **Class (Phase A, unchanged):** REIMPLEMENT
- **Effective consequence:** winit backend behind the backend boundary (T2)
- **Justified by:** INV-ARCH-1
- **ADR:** ADR-12

### PL-18 — xwayland-satellite
- **Class (Phase A, unchanged):** ADAPT
- **Effective consequence:** satellite as an ordinary client under a supervisor; restart on the same display (OB-33)
- **Justified by:** INV-XWAYLAND-1
- **ADR:** ADR-10

### PL-19 — Error handling
- **Class (Phase A, unchanged):** REIMPLEMENT
- **Effective consequence:** failure model; `unsafe` policy
- **Justified by:** INV-FAIL-1, INV-ARCH-2
- **ADR:** —

### PL-20 — Test architecture
- **Class (Phase A, unchanged):** DISCARD
- **Effective consequence:** T0–T5 strategy (FOUNDATION_TEST_PLAN)
- **Justified by:** INV-ARCH-1
- **ADR:** ADR-12

### PL-21 — Placement policy
- **Class (Phase A, unchanged):** DEFER
- **Effective consequence:** Level D UX (camera default placement is also Level D, INV-CAMERA-3)
- **Justified by:** INV-STAGE-1
- **ADR:** —

### PL-22 — Focus
- **Class (Phase A, unchanged):** REIMPLEMENT
- **Effective consequence:** ActiveElementFocus; `activated` per INV-PROTO-1
- **Justified by:** INV-INPUT-8, INV-PROTO-1
- **ADR:** ADR-07, ADR-14

### PL-23 — Data device
- **Class (Phase A, unchanged):** ADAPT
- **Effective consequence:** clipboard plumbing follows ActiveElementFocus; DnD is a client-owned grab sequence
- **Justified by:** INV-INPUT-1, INV-INPUT-8
- **ADR:** ADR-07

<!-- /living -->
