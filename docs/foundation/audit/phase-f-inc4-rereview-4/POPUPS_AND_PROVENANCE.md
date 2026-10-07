# Fifth Review — Popups, Lifecycle Separation, and Provenance

## 1. Popup Lifetime & `reconcile_popup_chain` (A1 Resolution)

### Architectural Mechanism
The root cause of A1 in Round 3 was that `popup_destroyed` only cleared grab state if the popup chain had already ended or the destroyed surface matched `current_grab()`. On nested chains (depth $\ge 2$), when popups were destroyed in protocol-mandated reverse order (P3, P2, P1), Smithay's internal grab was not cleanly torn down, leaving keyboard focus on a dead surface while Core believed the root was focused. On the next key press, Smithay attempted lazy refocus, issuing `wl_keyboard.enter` with the key already down, triggering a severe protocol violation.

Round 4 introduced `reconcile_popup_chain()` in `crates/infynitae-wayland/src/handlers.rs`:
```rust
fn reconcile_popup_chain(&mut self) {
    let Some(g) = self.popup_grab.clone() else { return; };
    let serial = SERIAL_COUNTER.next_serial();
    let kb = self.keyboard_handle();
    if !g.has_ended() {
        let top = g.current_grab();
        if top.is_some() && kb.current_focus() != top {
            kb.set_focus(self, top, serial);
        }
        return;
    }
    self.popup_grab = None;
    if let Some(ks) = self.popup_keyboard_grab_serial && kb.has_grab(ks) {
        self.keyboard_unset_grab();
    }
    self.popup_keyboard_grab_serial = None;
    if let Some(ps) = self.popup_pointer_grab_serial && self.pointer_has_grab(ps) {
        self.pointer_unset_grab(serial, 0);
    }
    self.popup_pointer_grab_serial = None;
    let target = self.core.focus().target()
        .and_then(|id| self.window(id))
        .and_then(|w| w.toplevel().map(|t| t.wl_surface().clone()));
    kb.set_focus(self, target, serial);
}
```

### Depth-Independence Verification
The review attacked chains at depth 1, 2, 3, 4, and 5:
- **Inner popup destruction:** When P3 dies in a 3-level chain, `g.has_ended()` is false. `current_grab()` returns P2. `reconcile_popup_chain` immediately calls `kb.set_focus(P2)` before any subsequent event. Verified in `attack_popup_inner_dies_outer_keeps_routing`.
- **Full chain reverse close:** When P3, P2, P1 close in order, at P1's death `g.has_ended()` is true. Grabs are unset by serial, and focus returns to `target` (the root window). The client receives exactly one `wl_keyboard.enter` for each surviving level, and finally the root window. When a key is pressed immediately afterward, zero protocol violations occur.
- **Middle popup destruction:** When P2 dies in a 4-level chain (protocol aberration tolerated by Smithay), `current_grab()` remains P4. Routing stays on P4 until P4 dies, transitioning to P3, then P1, then root window. Verified in `attack_popup_middle_dies_tolerated`.
- **Rapid batch destruction:** When all popups are destroyed before a dispatch pump, `reconcile_popup_chain` reconciles directly to the root window with zero residual grabs.

## 2. Tree / Lifecycle Separation (B1 Resolution)

### Architectural Mechanism
In Round 3, `popup_destroyed` invoked `element_committed(parent)`, which called `Core::toplevel_committed`. For an unmapped parent window (which unmaps via null-buffer commit and has its popup dismissed), calling `toplevel_committed` erroneously advanced the parent's lifecycle from `Lifecycle::Unmapped` to `Lifecycle::AwaitingFirstBuffer`, sending an unsolicited configure. When the client later issued its protocol-required initial commit to re-show the window, Core considered it already awaiting buffer and sent zero configures, permanently breaking re-mapping.

Round 4 strictly decoupled scene tree updates from lifecycle transitions:
1. `element_root_committed(index)`: Drives M1 lifecycle (`Core::toplevel_committed`). Only called in `CompositorHandler::commit` when `element_index_by_root(surface)` matches the element's ROOT surface.
2. `element_tree_changed(index)`: Rebuilds hit-test and rendering tree snapshot via `Core::toplevel_tree_changed`. Called on popup death and on child surface (popup/subsurface) commits.
3. `Core::toplevel_tree_changed(id, tree)`:
```rust
pub fn toplevel_tree_changed(
    &mut self,
    id: ElementId,
    tree: SurfaceTree,
) -> Result<Vec<Effect>, ProtocolError> {
    tree.validate().map_err(ProtocolError::Snapshot)?;
    let state = self.stage.state(id).ok_or(ProtocolError::StaleElement)?;
    if state != Lifecycle::Mapped || !tree.root().is_some_and(|r| r.mapped) {
        return Ok(Vec::new());
    }
    self.stage.set_tree(id, Some(tree)).map_err(|_| ProtocolError::StaleElement)?;
    Ok(self.visibility_pass())
}
```
If an element is not `Lifecycle::Mapped` (e.g. `Unmapped` or `AwaitingInitialCommit`), `toplevel_tree_changed` returns `Ok(Vec::new())` without modifying lifecycle state or emitting effects.

### Falsification & Verification
- **Unmapped parent re-show:** Verified in `attack_b1_unmapped_parent_popup_death_does_not_advance_m1` and `b1_unmapped_parent_reshow_gets_exactly_one_configure`:
  1. Window mapped, popup opened.
  2. Null-buffer commit unmaps window; popup dismissed and destroyed.
  3. Parent state remains `Lifecycle::Unmapped`.
  4. Client commits null buffer to re-show window $\rightarrow$ receives EXACTLY ONE configure; advances to `Lifecycle::AwaitingFirstBuffer`.
- **Hit testing in former popup area:** Former popup area immediately delivers clicks to parent window (2 button events).
- **Uncommitted parent:** Child popup commit or destruction on an uncommitted parent never sends configure and leaves state at `Lifecycle::AwaitingInitialCommit`.

## 3. Serial Provenance & Fallback Semantics

### FIFO Store & Fallback
The causing output provenance store tracks `(Serial, OutputId)` tuples capped at 64 entries.
In `crates/infynitae-wayland/src/lib.rs`:
```rust
pub(crate) fn record_serial_output(&mut self, serial: Serial, output: OutputId) {
    if self.serial_outputs.len() >= 64 {
        self.serial_outputs.pop_front();
    }
    self.serial_outputs.push_back((serial, output));
}
```
Resolution policy:
- If `serial` is found in `self.serial_outputs`, return the causing `OutputId`.
- If missing, evicted, or caused by a keyboard event, fall back to `self.core.pointer_output()` (the pointer's current output under Rule 1's "else the seat's last input" clause).

### Verification
In `serial_provenance_fifo_eviction_and_fallback` and `attack_provenance_store_fifo_overflow_and_retention`:
- Evicted serial ($>64$ events ago) falls back to pointer output.
- Retained middle records resolve to their distinct causing output.
- Newest record resolves to its causing output.
- Unknown / unissued serial falls back to pointer output.
- Mutants F15 (evicting newest entry) and F16 (aliasing latest record) are both 100% detected.
