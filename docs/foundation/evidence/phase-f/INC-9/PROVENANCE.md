# INC-9 OB-24 evidence — provenance of these copies (2026-10-17)

The files in this directory are copied **verbatim** from `experimental/inc-9-zoom-contract` at
`829b170` (the branch itself is not merged: it carries INC-9 harness code from before INC-6). They
are the evidence the owner's OB-24 decisions of 2026-10-17 were taken on. Their status lines
("PROPOSAL", "Nothing is adopted") describe them as written and are kept unchanged.

They are **superseded** by layer C12.F14 (FOUNDATION_ARCHITECTURE §C12.F14) and its record,
`../OB-24/DECISION_RECORD_2026-10-17.md`. Where they differ, C12.F14 governs; in particular:
- the A2 exactness bound is 2³¹, not the package's 2⁵⁰ (§C12.F14.4 item 1;
  `anchor_model_sim.py` T3 used k values with small denominators);
- the package's M4 phrase "validates the hardware facts" does not exist in M4 (§C12.F14.4 item 2);
- `ZOOM_CONTRACT.md` is the derivation of the domain; its amendment texts were drafting input.
