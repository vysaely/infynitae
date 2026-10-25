# Layer surfaces — owner decision record (2026-10-21)

Status: **OWNER DECISION, 2026-10-21**, from the owner's message of 2026-10-21 ("go with the
recommendation"), answering `DECISIONS_PROPOSAL_2026-10-21.md` (this directory, a verbatim copy).
Each decision is that proposal's recommendation [R] for its row.

| # | Decision |
|---|---|
| L1 | A layer surface is a new Core object (OBJ-35): per output, screen-fixed in output-logical coordinates, outside the Canvas, never moved by a camera, never in membership's camera views. |
| L2 | `background` and `bottom` are drawn below the Canvas elements, `top` and `overlay` above them, per output. |
| L3 | Hit testing per output: overlay → top → Canvas (ProjectionFrame) → bottom → background, input regions respected; F-HIT-1 unchanged for the Canvas part. |
| L4 | Exclusive zones shrink the output's usable area, used by F-PLACE-1 and F-FIT-1; the camera's view, membership and scale are unchanged. |
| L5 | Keyboard: `none` never focused; `on_demand` focused by a click; `exclusive` takes the keyboard while mapped in `top`/`overlay` and gives it back to the window focus when it goes. |
| L6 | During a protocol-fullscreen presentation only `overlay` surfaces are shown. |
| L7 | A pan that starts on a layer surface still pans; pointer constraints apply to layer surfaces as to element surfaces. |
| L8 | Any client may create layer surfaces. |

Normative consequences are prepared as layer C12.F19 (`C12F19_PACKAGE_2026-10-21.md`), **not
adopted**; implementation and evidence: `IMPLEMENTATION_RECORD_2026-10-21.md`.
