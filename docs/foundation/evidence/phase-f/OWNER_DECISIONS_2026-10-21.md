# Owner decisions, 2026-10-21

Status: **OWNER DECISIONS, recorded 2026-10-21** from the owner's message of 2026-10-21 ("yes
go"), approving the recommendations of the preceding message. Not normative text by themselves:
each is applied by the change it names.

| # | Decision | Applies to | State |
|---|---|---|---|
| 1 | `main` fast-forwarded to `839f287` (INC-12 integrated, EXPERIMENTAL; INC-11 implemented, not accepted). | repository | done |
| 2 | **OB-40 / PROV-10: activation policy Strict** — an `xdg_activation_v1` request focuses its window only with a validated token (the focused client's, fresh serial, unused, unexpired) and when the window may take focus; every other request marks attention. The default already on `main`. | OB-40, INC-12 exit | recorded; **OB-40 stays OPEN**: closing a DECISION obligation is blocked by `check-foundation-docs` `deferred-closed` (INC-10 change-control record U-8) |
| 3 | **Q-U1 decorations: none.** The compositor offers `xdg-decoration`, answers server-side mode and draws no decoration (no title bars, borders, shadows or rounded corners from the compositor; clients stop drawing CSD). | a later change (xdg-decoration) | recorded; not implemented |
| 4 | **The shell is Quickshell-class**: panels, bars and launchers draw through layer-shell and control the desktop through the INC-11 socket. Layer-shell is the next architecture decision (decision paper `~/infynitae/LAYER_SHELL_DECISIONS_PROPOSAL_2026-10-21.md`). | next increment | decision paper prepared |
