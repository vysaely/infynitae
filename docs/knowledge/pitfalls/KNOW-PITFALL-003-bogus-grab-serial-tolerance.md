# KNOW-PITFALL-003: Permissive / Invalid Popup Grab Serial Tolerance

| Field | Value |
| :--- | :--- |
| **id** | KNOW-PITFALL-003 |
| **title** | Permissive / Invalid Popup Grab Serial Tolerance |
| **subsystem** | Pitfalls/Security |
| **source_rev** | DriftWM / Infynitae INC-7 Review |
| **date** | 2026-10-14 |
| **evidence_class** | CONTRADICTED |
| **applicability** | XDG Shell popup grab security |
| **affected_branches** | N/A (Rejected Anti-Pattern) |
| **related_invariants** | W-SM-7 |
| **adoption_status** | REJECTED |
| **next_action** | Strictly reject invalid popup grab serials |

---

## Verified Facts
1. Upstream DriftWM intentionally installs popup grabs with unrecognized serials ([`src/tests/popups.rs:107`](file:///home/vysaely/.gemini/antigravity/scratch/driftwm/src/tests/popups.rs#L107)), choosing client compatibility over specification compliance.
2. The XDG Shell specification explicitly requires that popup grabs match a valid user interaction serial.
3. Permissive tolerance allows an unprivileged background client to monopolize seat pointer and keyboard input without user interaction, creating an input hijacking and denial-of-service vulnerability.

4. **Reconciliation 2026-10-14 (main `f323c2d`):** **Correction (X-16).** "Monopolize seat pointer and keyboard input" overstates the risk: a popup grab is ended when the user clicks outside the client's surfaces. The real effect of accepting an invalid serial is a grab (keyboard focus to the popup) without user interaction until dismissed — focus stealing. No adversarial client script exists; "mathematically obvious" below is not evidence. Infynitae rejects such grabs (W-SM-7, ratified since INC-4).

## Unverified Hypotheses
- None; the security attack vector is mathematically obvious and demonstrable via an adversarial client script.

## Performance & Correctness Implications
- Tolerating invalid serials compromises session security and violates protocol guarantees.

## Limitations & Counterexamples
- Reconsideration would only be justified for a dedicated compatibility shim running legacy non-conformant toolkits in an isolated, sandboxed security boundary.

## Reproduction Instructions
Inspect DriftWM's permissive popup grab test:
```bash
view_file /home/vysaely/.gemini/antigravity/scratch/driftwm/src/tests/popups.rs (lines 107-142)
```

## Links & Citations
- Source Test: `driftwm/src/tests/popups.rs`
- Related Research: [KNOW-RES-003: DriftWM Popups and Lock Reentrancy](../research/KNOW-RES-003-driftwm-popups-and-locks.md)
