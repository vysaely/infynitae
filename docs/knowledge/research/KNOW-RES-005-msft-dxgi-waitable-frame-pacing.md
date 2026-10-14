# KNOW-RES-005: DXGI Waitable Swapchains, Queue Depth, and Frame Pacing

| Field | Value |
| :--- | :--- |
| **id** | KNOW-RES-005 |
| **title** | DXGI Waitable Swapchains, Queue Depth, and Frame Pacing |
| **subsystem** | Research/MicrosoftGraphics |
| **source_rev** | DXGI 1.3 / Windows SDK |
| **date** | 2026-10-14 |
| **evidence_class** | SOURCE-DERIVED |
| **applicability** | Frame scheduling and client pacing |
| **affected_branches** | N/A (Comparative Architecture) |
| **related_invariants** | INV-FRAME-1, INV-PRES-1 |
| **adoption_status** | RESEARCH |
| **next_action** | Prototype wp_presentation_time feedback in adapter |

---

## Verified Facts
> **Reconciliation 2026-10-14 (main `f323c2d`):** the Microsoft statements below were not checked against Microsoft documentation in any session, and some are interpretations ("exact parity", magnitudes). Read them with the MS-DOC / INTERP / APPLY / WIN-ONLY classes of [KNOW-RES-008](KNOW-RES-008-msft-research-claim-classification.md).

1. **Queue Bloat:** Unconstrained GPU rendering queues can buffer 3–4 frames of execution ahead of scanout, adding 50–100ms of input lag.
2. **Waitable Swapchains:** DXGI 1.3 introduced `IDXGISwapChain2::GetFrameLatencyWaitableObject()`. Applications wait on an OS event handle before executing CPU simulation for the next frame, pinning queue depth strictly to 1 frame.
3. **Linux/Wayland Parity:** The Wayland `wl_surface.frame` callback combined with `wp_presentation_time` provides the Linux equivalent, providing high-resolution hardware latch timestamps and throttling client render loops.

## Unverified Hypotheses
- **Claim Requiring Validation:** Strict 1-frame queue equivalence between DXGI waitables and Wayland frame callbacks must be benchmarked under fluctuating render workloads (e.g. 5ms to 20ms frame times) to test for queue starvation vs judder.

## Performance & Correctness Implications
- Reduces click-to-photon latency by up to 2 full frame intervals.
- Protects against thermal throttling and excessive GPU energy consumption.

## Limitations & Counterexamples
- Off-screen clients receiving zero frame callbacks can starve their swapchains (e.g. EGL swapchain hangs), requiring a gated heartbeat.

## Reproduction Instructions
Review Microsoft Learn:
- *"Reduce latency with DXGI 1.3 swap chains"* (Microsoft Learn)
- Verify with Wayland presentation-time protocol specification.

## Links & Citations
- Source Documentation: Microsoft Learn DirectX Developer Guide
- Related Research: [KNOW-RES-004: DirectFlip, Independent Flip, and MPO](KNOW-RES-004-msft-directflip-independent-flip-mpo.md)
- Related Pitfall: [KNOW-PITFALL-006: Unbounded Off-Screen Callback Heartbeats](../pitfalls/KNOW-PITFALL-006-unbounded-offscreen-callback-heartbeats.md)
