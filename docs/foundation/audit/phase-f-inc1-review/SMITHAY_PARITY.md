# Pinned Smithay 0.7.0 Analysis and Parity Proofs

## 1. Pinned Source Verification

Source location: Cargo registry cache `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/smithay-0.7.0`.

### 1.1 `with_surface_tree_downward` Traversal Order
In `src/wayland/compositor/tree.rs`:
- Children vector maintains relative stacking: subsurfaces placed above parent are appended; subsurfaces placed below parent (`place_below`) are inserted before the parent.
- Traversal with `reverse: true` (`with_surface_tree_downward`) iterates `data_guard.children.iter().rev()`.
- Children above parent are visited first; parent surface is visited when `c.id() == surface.id()`; children below parent are visited last.
- This produces a strictly top-to-bottom (nearest to screen first, front-to-back) ordering.
- `infynitae-wayland::snapshot::tree` uses `with_surface_tree_downward`, faithfully reflecting Smithay's rendering traversal.

### 1.2 `Window::render_elements` Analysis
In `src/desktop/space/wayland/window.rs`:
- Popups from `PopupManager::popups_for_surface(&root)` are evaluated first.
- For each popup, its offset relative to root is `self.geometry().loc + popup_offset - popup.geometry().loc`.
- Popup trees are rendered downward before the root tree.
- Root surface tree is rendered downward via `render_elements_from_surface_tree`.
- Notice: `render_elements_from_surface_tree` takes `location: impl Into<Point<i32, Physical>>`, truncating the root location to integer pixels (`W-SM-5`).
- Infynitae avoids `W-SM-5` by passing the exact f64 `RenderLocation` ($L(x)$) directly to `WaylandSurfaceRenderElement::from_surface`.

### 1.3 `surface_under` Failure Modes (W-SM-11 & W-SM-12)
In `src/desktop/wayland/utils.rs`:
- **W-SM-11:** `RendererSurfaceState::contains_point` tests input regions with `point.to_i32_round()` (round-to-nearest) but tests surface bounds with exact half-open containment, shifting input regions by $-\frac{1}{2}$ px.
- **W-SM-12:** `under_from_surface_tree` overwrites its `found` slot for every match during downward traversal and only early-stops after an entire node's subtree. A subsurface placed **below** its parent (`place_below`) is traversed after the parent and overwrites the parent's match, stealing input clicks even where the parent is rendered on top.
- Independent test `tst09_xsnap_x15_and_surface_under_control` proved that 100% of disagreements between `Window::surface_under` and presented surfaces are accounted for by W-SM-11 and W-SM-12 (0 unexplained differences).

### 1.4 Mapping State (`view()` vs `buffer()`)
In `src/backend/renderer/utils/wayland.rs`:
- When a client commits a buffer, `on_commit_buffer_handler` stores the buffer and computes `surface_view`.
- When a null buffer is attached (`wl_surface.attach(nil)`), `BufferAssignment::Removed` triggers `self.reset()`, clearing both `self.buffer = None` and `self.surface_view = None`.
- `d.buffer().is_some()` accurately and reliably tracks whether a surface is mapped.

## 2. Mathematical Parity Proofs

### 2.1 Formula Equivalence: `edge(x)` vs `WaylandSurfaceRenderElement::geometry`
In Smithay's `WaylandSurfaceRenderElement<R>` (`src/backend/renderer/element/surface.rs`):
```rust
fn geometry(&self, scale: Scale<f64>) -> Rectangle<i32, Physical> {
    Rectangle::new(self.location.to_i32_round(), self.size(scale))
}
fn size(&self, scale: impl Into<Scale<f64>>) -> Size<i32, Physical> {
    ((self.view.dst.to_f64().to_physical(scale).to_point() + self.location).to_i32_round()
        - self.location.to_i32_round())
    .to_size()
}
```
Smithay computes:
- Left edge: $\text{round}(\text{location}.x)$
- Right edge: $\text{location}.x.\text{to\_i32\_round}() + \text{size}.w = \text{round}(\text{location}.x + \text{width} \cdot k)$

Under Infynitae:
- Location is $L(x) = (x - R) \cdot k + \delta - C$ (F-RENDER-1)
- Left edge: $\text{round}(L(x)) = \text{round}((x - R) \cdot k + \delta - C)$. Since $C \in \mathbb{Z}$:
  $$\text{round}(L(x)) = \text{round}((x - R) \cdot k + \delta) - C = \text{edge}(x)$$
- Right edge: $\text{round}(L(x) + \text{width} \cdot k) = \text{round}((x + \text{width} - R) \cdot k + \delta - C)$:
  $$\text{round}(L(x) + \text{width} \cdot k) = \text{edge}(x + \text{width})$$

**Proof:** Because Infynitae defines $\text{physical\_rect}(x_0, w) = [\text{edge}(x_0), \text{edge}(x_0 + w))$, the edges computed by Core match Smithay's presented render element edges identically for all $x_0, w, k$.

### 2.2 Empirical Parity Verification
- `tst04_x6_render_edges_equal_lattice_edges`: Tested $n \in [120, 360]$ ($k \in [1.0, 3.0]$) across 3 magnitudes up to $2^{36}$ (63,016 rectangles). Mismatches: 0.
- `tst10_xhit_selection_equals_presented`: Tested $k \in \{1.0, 1.25, 1.5, 1.75\}$ across fractional cameras (>100,000 pixels). Mismatches: 0.
- `tst18_d5hit_zoom1_configurations`: Tested 3840×2160, $k = 1.5$, D5 camera (>100,000 pixels). Mismatches: 0.
- `adversarial_fractional_scales_and_aspect_ratios`: Tested scales 1.1, 1.25, 1.333, 1.5, 1.667, 1.75, 2.0, 2.25 across 5 display resolutions. Mismatches: 0.
