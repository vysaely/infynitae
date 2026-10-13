# `configure_bounds` amendment (registry layer C12.F3) — record

Status: **adopted 2026-10-14 by owner approval; local main, not pushed.** Evidence and
provenance; the authority is FOUNDATION_ARCHITECTURE §C12.F3.

## Authorized meaning

Smithay may emit `configure_bounds(0, 0)` during the initial configure for supported xdg-shell
versions. Zero dimensions are nonrestrictive and semantically equivalent to no bounds being
specified. Non-zero bounds and broader protocol changes are not authorized.

## Findings behind it

| Question | Finding |
|---|---|
| Who emits it | Smithay 0.7.0, `send_toplevel_configure`, reached from `ToplevelSurface::send_configure` |
| When | with a toplevel's first configure (`!initial_configure_sent`), and whenever the pending bounds differ from the current ones; only to a client that bound `xdg_wm_base` ≥ 4 |
| Value | `bounds.unwrap_or_default()`: `(0, 0)` while no bounds are set. Infynitae never sets any |
| Suppressible | not through an API. Capping the advertised version below 4 would also remove `wm_capabilities` (version 5); a Smithay patch is not justified under D-B12-PIN (no defect) |
| Protocol meaning | xdg-shell: "If width and height are 0, it means bounds is unknown and equivalent to as if no configure_bounds event was ever sent for this surface." |

## Adopted wording against the proposal (P8)

The proposal read "`configure_bounds` is either not sent or carries `(0, 0)`". The authorized
meaning is narrower: the initial configure. The adopted sentence says so, and adds that a
non-zero bound is never sent:

> No bounds are proposed in Foundation 01. `configure_bounds` is sent only as `(0, 0)` and only
> with a toplevel's initial configure, where Smithay emits it for a client that binds
> `xdg_wm_base` version 4 or later (W-SM-17); xdg-shell defines zero dimensions as bounds
> unknown, equivalent to the event never having been sent. A non-zero bound is never sent.

Nothing else in INV-PROTO-1 changes. INV-CAMERA-4 is unchanged.

## Test

T1 `configure_bounds_is_zero_and_only_with_an_initial_configure` (a real client over the
protocol): through focus, a maximize request, an output reconfiguration to another size and
scale, a second output, an unmap and a re-show, the client sees `(0, 0)` once per initial
configure and nothing between. Seeded control: a bound set in a later configure.

Observed with separate-process clients as well (KCalc, the fullscreen validation client): one
`configure_bounds(0, 0)` before the first configure and none afterwards.
