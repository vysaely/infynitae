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
