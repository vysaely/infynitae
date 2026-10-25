# Erratum — commit hashes rewritten for e-mail privacy (2026-10-20)

Status: **ERRATUM, 2026-10-20**, by owner decision B (2026-10-20): before the first push of the commits after
`origin/main` `23752f1`, a personal e-mail address recorded as author and committer of 64 local commits
(20 of them on `main`, `f2bf0da` … `1d5931d`, 2026-10-16/17) was replaced, as both author and committer,
by `vysaely <vysaely@users.noreply.github.com>`. The address appears in no tracked file, commit message or
trailer, and this record does not repeat it.

## What changed and what did not

- **Changed:** commit metadata only — the identity line of the 64 commits; therefore the hash of those commits and of
  every descendant: **121 commits**, **75 of them on `main`**. Abbreviated hashes of rewritten commits
  inside commit messages were replaced by the new hash of the same length; inside ref, directory and file names
  (`rehearsal/inc8-clipboard-shutdown-on-619187f`, `gates-main-56b3f12/`, `perf_frame_times_56b3f12_run{1,2}.out`)
  the old text is kept, because those names did not change.
- **Not changed:** every **tree** — file contents are byte-identical (verified for all 428 commits: old tree = new tree);
  parents (mapped), author and committer names and dates, message text other than those hash tokens, and the order of
  history. `origin/main` `23752f1` and its 153 commits keep their hashes; the nine historical seals of
  `tools/verify-development-environment` keep theirs.
- **Text in this repository is not edited.** Commit hashes recorded in the foundation documents, the evidence records,
  the layer headers and `tools/check-foundation-docs` (`F_LAYERS`) are the **pre-rewrite** names; read them through the
  table below. That includes the layer bases `ca2e468` (C12.F14; now `da062bf`), `dda4d87` (C12.F15; now
  `16977bb`), `da538d6` (C12.F16; now `e109e68`) and `16f13c9` (C12.F17; now `dfdbc77`); the INC-6
  ratified commit `ca2e468` (now `da062bf`) and the INC-10 ratified commit `faf6dda` (now `4d5be84`). Tree
  hashes recorded in the evidence (for example "tree `5f80065`") stay valid as written.

## Verification

Run over the composed `commit-map` of `git filter-repo` (mailmap pass, then a message pass for the name tokens and two
forward references), old repository against new: commit count 428 = 428; for every commit, tree equal, parents equal
under the map, author and committer equal after the address substitution, header fields equal, message equal up to
hash tokens; zero occurrences of the address in any identity, message or object of the new repository; one identity
left (`vysaely <vysaely@users.noreply.github.com>`); 99 refs, each the image of its old value; `origin/main` =
`23752f17651887426e89e15dd80b40f73111aa8e`. Result: ALL CHECKS PASSED.

## Map — commits on `main` (oldest first; 75)

| old | new | date | subject (pre-rewrite text) |
|---|---|---|---|
| `f2bf0da39572f8e5299f10bb8458832297a4d64a` | `ee92e0642dc74672e211d391e9bf69670cb32857` | 2026-10-16 | governance(inc-6): record adoption of the INC-6 exit package (C12.F11–F13), prepare ratif… |
| `35159b929d3177e4652bd1bdd05b911b9eea5ff1` | `ec54d08baefa08a6571fdb4f453407a94f353c44` | 2026-10-16 | feat(clipboard): wl_data_device_manager and primary selection, authorized by the keyboard… |
| `bf040cab9b0778d4a71a2703e464288c5b498eed` | `9044ade20daa9ea443155248cc426e5f778883b7` | 2026-10-16 | feat(runtime): orderly shutdown on SIGTERM and SIGINT through a self-pipe [PROPOSAL, NEED… |
| `7aa2070bb6ce507e133307dfbcf2122d695715ec` | `e55383e954070ea5074332d3663a0b8833b71522` | 2026-10-16 | governance(inc-6): ratification review — layers restored to the approved package text; P-… |
| `f1e0d309deff347e36f36230d590158a649b0e1a` | `443765904f75a1458821dce4895aee0df4046a9a` | 2026-10-16 | fix(clipboard): drag-and-drop is refused — a client's drag no longer takes the pointer fr… |
| `b4c2e6262d232ab77db45d7c3e77f7106e6b70fb` | `e9ea7a9dd111fd7da64d5c579fc45733639e6825` | 2026-10-16 | feat(runtime): session process ownership — the compositor's process subtree ends with the… |
| `18173a95aa3251f09ff2310f3c0af0520301671c` | `5dc4721d62a22307acf4db3f0a3f82c9bc65f042` | 2026-10-16 | docs(runtime): ownership contract — in-process spawners register their children [PROPOSAL] |
| `ca2e4689dbe3e28f617f56e1a492edb03d9810d6` | `da062bfb27e2b9ee7443961cbe3061cdbc960eb8` | 2026-10-16 | docs(foundation): ratify Phase F INC-6 — scale policy; OB-13, OB-28, OB-42 closed [RATIFI… |
| `c28a1211640718de2371a00357df99135c7eb373` | `7527b33c32eaba799305ca468837f5484149aac9` | 2026-10-17 | fix(runtime): session members are the session's own children and adopted orphans, not eve… |
| `8322bb6212b7f1c27a1c3dabb5d18e120309f57e` | `24b48b6f847705030556deb4a4fd47741a04ce13` | 2026-10-17 | fix(runtime): spawn_owned — spawning and registering an owned child is atomic with respec… |
| `a63fca2a5a07546b430491dd4abb40c9f1b7593a` | `c58142ced6a0ffdd1496780069c8c3967d1dbc59` | 2026-10-17 | fix(runtime): members() lists and filters the direct children under the registry lock [PR… |
| `e5e482302427eb26b885db5d1781ffac54c7c7ea` | `6c24837183ab91f68623455a474592179d55bd83` | 2026-10-17 | fix(nested): submit a frame only when the host is ready for it — a hidden window no longe… |
| `d655f83ccb5d4e6bdf884edb5b31cd8160dff43c` | `a8638db013ff58584c910bb9aea7ef5d513ba722` | 2026-10-17 | evidence(nested): NB-1 investigation — harness, field stalls (2/15 per build), determinis… |
| `df35fba3e30615bd19e2e3cc240db93f489c57b0` | `da0f4f648bbc3b34e22e269f60e6db67fa0573df` | 2026-10-17 | test(runtime): a failing shutdown test kills and reaps its compositor (seeded-defect runs… |
| `7d7ac73abab5d14ee2c209bda5a937a62525b6f9` | `3178ff7d4696c8b448a1f229651a0f2b8e7778a1` | 2026-10-17 | test(nested): NB-1 regression test (ignored runtime check) — fails on unfixed main, passe… |
| `b251a43452c951de40466127db745a9850262846` | `6488c37b8af89189c3e41a54133228ff792353fb` | 2026-10-17 | governance(ob-24): record the owner's decisions D1–D6; layer C12.F14 — representable zoom… |
| `1f2b246a71ae8ea3cc6039ad9a166b11a39bb908` | `ee61443f98c3d7438ac6a13b7aa25f4ff4ebd08e` | 2026-10-17 | merge: NB-1 host-frame gating fix (fix/nested-host-frame-gating-2026-10-17, 7d7ac73) into… |
| `7ddde8f091325404324aec78765b567eeddd9db9` | `5f2a134212ca1255f96e955b063905667e10e8cc` | 2026-10-17 | docs(foundation): record the NB-1 integration and INC-6's local integration in CHECKPOINT… |
| `387888e64fbd80bbd19511fa0862ebf1d0bb4b97` | `85fc4deecba051eaa36d8928f2fd87026e9e0b60` | 2026-10-17 | merge: main (NB-1 integration, 7ddde8f) into the OB-24 governance branch |
| `1d5931d7f9869046a0bb1c44487cf42b00859bf7` | `f0015e5b9bfcea44c118b4983566ddb43289c892` | 2026-10-17 | chore(verify): re-anchor the F-LATTICE-1 floor control on camera_on_grid's refactored ref… |
| `270128f802de6d8973ad2a35c0eae77221f21beb` | `ee52b34610579317284be70d6f497f92442cdade` | 2026-10-17 | governance(ob-24): C12.F14 corrections after review of 1d5931d — precision argument, rend… |
| `dda4d87ee760136a9c0de009a144aaa50491c08e` | `16977bb193e2a4ece1159f11c73e207fc92c4a53` | 2026-10-17 | docs(foundation): record C12.F14's integration into local main (270128f); G-OB24 met, OB-… |
| `da538d6523d98f891ff5b6dda930fa6a50debcc9` | `e109e68873dca51fbdad95cbaee388fa29d7c44a` | 2026-10-17 | governance(c12.f15): one canonical rect for rendering and hit testing (option B); F-FIT-1… |
| `e75ea72d9c79d8767ec6387b38a3c04a381e0e0d` | `c842e7c638d818535423afda87b21bf8208e5e92` | 2026-10-18 | governance(c12.f16): bounded fit/restore history — 256 entries per output, oldest evicted… |
| `b46021fb17d90f4af71cf646ded7e2f204433948` | `8b7602f07e7deb1527367f3de64b5bb0632afca6` | 2026-10-18 | feat(inc-10): the output's kept lattice reference and zoomed pan (F-LATTICE-1 as amended … |
| `2884287e99994d9eb9bd17c36bf7352d4f43189a` | `e1a80ebe1c9ed034eef0c9fff8fe17dc84215738` | 2026-10-18 | feat(inc-10): one canonical rect for drawing and hit testing — the CanonicalRect wrapper … |
| `cdf3a096bdf06a6298a78ee0d8f04bc72d3b5ac0` | `35b50f173feb78dda882504e7af6d9664065e42c` | 2026-10-18 | feat(inc-10): one-shot viewport fit and the bounded restore history (F-FIT-1 as amended b… |
| `73c0aca4b737242b68edefd78f7d1c9e318928dc` | `22b3a669d3c8552195ad541056de33d40a972263` | 2026-10-18 | test(inc-10): surface and output lifecycle while zoomed and panning; INV-ALLOC-1 at zoom … |
| `c4a72630bb2b39b835490a305538da61549eaee3` | `210426f3e30df23b0fd729ccd3ae6337451e38b9` | 2026-10-18 | docs(inc-10): implementation record and exit evidence; fix stale seeded builds in verify-… |
| `07edaff42b728ba904d99cb08bc5816c49bf8d65` | `9f7236feda543d9edcf9fc18b979b5afc40b2713` | 2026-10-18 | fix(inc-10): reconcile the pointer after a fit or restore under a kept confinement (exit … |
| `071364e4fd6134a313c2cdfa5630e715bc935314` | `22edb10b655c7d4db98e049838337fbdbb48c530` | 2026-10-18 | feat(inc-10): Logo+F / Logo+Shift+F match their modifiers exactly; T1 binding and target … |
| `2dbdf8d2226f77085bc3a11173a06e6769ff2bcc` | `607ec72225ff95e14371c40be67ff2d85b0480f1` | 2026-10-18 | test(inc-10): maintained pixel sweep, crop sampling with viewport source and buffer scale… |
| `abe64b78096930e433be61a8bafc9b93045105b7` | `fcf324f5ee5da214fbf9562bec504981f67ea7f0` | 2026-10-18 | perf(wayland), fix(tools): no per-frame OutputModel clone in render_output; featured effi… |
| `58ba428b9a6f5c73343cda76cb33e059d1520d59` | `13d0ec3b794b5dbe11fead1c370909e9bce1b907` | 2026-10-18 | docs(inc-10): exit-review M1 resolution record; follow-up obligations OB-54 … OB-58 [INC-… |
| `619187fe18f8fdbcbf529089732642b3959b72ab` | `89295b7581e7e4766173d54fbe9c9828f2f2810b` | 2026-10-18 | governance(c12.f17): amendment package for change control — INC-10 exit clarifications [P… |
| `f5f36d3f029842504e141246955e7730b67ab8be` | `dfde84ce7f7046d38f63b7e4ce12fd2dbda7f94e` | 2026-10-18 | fix(input): an Escape cancel ends a pan as its release does — epoch settled, reconciliati… |
| `d27de97c5bf1802675f8fd011b4ef78bb04b6255` | `bc7ac054a9b1db8c039fa5dce200741fbd150160` | 2026-10-18 | fix(wayland): a oneshot constraint Smithay ended stays ended in Core (OB-54, audit Q-B3) |
| `46e76d4c7471545345bd74225d83a93c1bda7eee` | `a091d0b58f8fb0946808089271f42dc5bfcd8d11` | 2026-10-18 | perf(input): client button press and release allocate nothing (INV-ALLOC-1 input intake, … |
| `0d4a39a622e7de1490e36304069a6eea36551de6` | `7d3446790f46e185dff84c726282be67faea4fb6` | 2026-10-18 | test(inc-10): TST-18 d5hit zoomed configurations on the production path (OB-14 zoom != 1 … |
| `3c8b81d8ade25acf4a80e0a3b7f509681ac84817` | `884645e595f64d1a6d32f1aa1dd156a42dc4de4b` | 2026-10-18 | test(inc-10): OB-53 INC-10 part — an adversarial client under a viewport fit (EC-22 loop) |
| `1e216ad4a8c8b8a41ee55b4213f672985c825d61` | `251a6965f569682a6a1fe1339a0c368cf49d3095` | 2026-10-18 | test(inc-10): zoomed d5hit checks OB-14's sub-pixel clamp at the far corner of every pixel |
| `b00ecd7d0f9d43618f9c16ce4337d26c9d0d41e2` | `ecab52b13eb9ba5aab3b86a28a8ef4024208c080` | 2026-10-18 | docs(inc-10): exit readiness — OB-14 / OB-53 evidence map, C12.F17 Q1 framing corrected a… |
| `ca7b5d53c97da302eef5de354843ac8d07579388` | `c4906115bb656511f424ad0c0c06f844c7d82718` | 2026-10-18 | REHEARSAL ONLY — merge governance/inc-8-ratification-2026-10-16 (c003657, INC-8 ratified … |
| `a8d17e752e526e8ba7bc430cba9911c39d5d62ff` | `15c059b9e96106b77db86b2d0c427fefc7e3c384` | 2026-10-18 | fix(tools): re-anchor two mutation seeds on the allocation-free button handlers (Q-P1) |
| `6c2ccf111505003d0fa19cb3e5041fff3847c211` | `444a23d8fe155d2da86b524b96e9422a6710ba4a` | 2026-10-18 | fix(output): no wl_pointer.motion to a kept lock at an output reconfiguration (OB-55, aud… |
| `fc4636c93831d93e508676bee64d52a6674c5774` | `73203c6fb9f6822a103e7a743f0abc3dd516b989` | 2026-10-18 | test(inc-10): t1_d5hit_zoomed module doc without a lazy list continuation (clippy doc_laz… |
| `4c122abf3d86e735b8230919a3b0b9ecc5b8b748` | `8b98b92e63d4054cec1b047d9a348146f25bee60` | 2026-10-19 | feat(runtime): the session's signal self-pipe on libc, not signal-hook; shutdown tests us… |
| `b322a370d0ae9f15fcac0290e60333c2983c8ee3` | `d3cca2e72fa8a7c8e95ae2f89c478ab557cbd7d8` | 2026-10-19 | tools: verify-test-efficacy --check-seeds proves every seed compiles |
| `155bf212e4c3d2256e717da85c8821ca049bae89` | `98981e0b7296831fe997c9ce9ff81c452a732637` | 2026-10-19 | Merge fix/defect-verification-2026-10-18 (owner decision 2026-10-19: merge after independ… |
| `25751584fd2f4258a892dd179c331e6ec257152d` | `b230dc4df259bc0ab1a330d68c50f9b5f4d4e5c4` | 2026-10-19 | Merge prep/ob55-q2-gated-2026-10-18 (owner decision 2026-10-19: OB-55 fix, Q2 widened) |
| `17486df983ebef2a34f2d7540ce61cd4a1689476` | `3d341c4f94dd2d5f8484095ffc3e37bd7e7f6909` | 2026-10-19 | Merge rehearsal/inc8-clipboard-shutdown-on-619187f (owner decision 2026-10-19: integrate … |
| `69314b68640d8101ce645c2958688ca7df24996f` | `d676b1d2b65d328134163d986fdf0aa623588b4c` | 2026-10-19 | Merge prep/inc10-exit-readiness-2026-10-18 (C12.F17 package, INC-10 exit evidence; gated … |
| `ab0e53deaad834afd418ce8c2a1e0b88d34bb638` | `18c2455aec635867007a487dfa37232594d2a33d` | 2026-10-19 | core: C12.F17 Q1 (B, scope 2) and Q3 ("exactly one") per owner decisions 2026-10-19 |
| `b27d345a38177d2031bacd84e0127f6aa791f072` | `8f17b0a6caaa2e5934ebf0d6b2945c4039f4323a` | 2026-10-19 | docs(c12.f17): record owner decisions Q1–Q5 (2026-10-19) and apply the §8 edits; Q4 forwa… |
| `187e55149c3b1e0b14bcfa6ddcdfa2173e5018fb` | `3b47acef14cd00dc495c52aca8c7ba5e07222489` | 2026-10-19 | docs(c12.f17): OB-59 states how a drag is refused (cancelled, no offer), citing its T1 te… |
| `122e83c74e33a6dcf450c7e19bf2ceb479810b22` | `383aaec9a4b7f46be0417b719170bb85eb295466` | 2026-10-20 | Merge work/c12f17-2026-10-19 (C12.F17 package with owner decisions 2026-10-19; Q1 B scope… |
| `0d9cbcc6725bde9baa9260cdacb115857055e672` | `03cc7211abfc84215c9fd70b428afd7235bca9f9` | 2026-10-20 | Merge feature/clipboard-2026-10-16 (owner decision 2026-10-19: clipboard yes; drag-and-dr… |
| `c5c8953db815d06f5dfcac8da218b0aa9052efc7` | `f2e40e7b30889f79e702bdae3dc32bb19cd47544` | 2026-10-20 | Merge feature/orderly-shutdown-self-pipe-2026-10-19 (owner decision 2026-10-19: orderly s… |
| `1dcff758fdc124070e9664ba13bb3e731482c6c4` | `6bb399327628c97551234909cade6ec8a347a669` | 2026-10-20 | docs(shutdown): record the owner decision of 2026-10-19 (design approved, signal-hook rej… |
| `65d4347dc452dfbc8c965aabb9b223644e1ee140` | `beb5466322b69a86f3083fb505022fbc2d2e4342` | 2026-10-20 | docs(checkpoint): the INC-8 rehearsal section becomes the record of the owner-authorized … |
| `85eac66064c4fb0d93d2bd6c3e602e78671f12da` | `f3e047ccc6ddc2005ddd94dbb42cbcd174c1da1d` | 2026-10-20 | docs(c12.f17): OB-59's Destination uses the registry vocabulary (DECISION) |
| `72fe35f31dfa1455b064a61542b8465984e4ffe5` | `56e3bd064fd6c7336a8361d06d1853cc1e8bae1c` | 2026-10-20 | core: no absolute motion to a kept lock when a presentation starts or ends (independent r… |
| `05a3069e5bf00bbbef720a02084f5e6a23e34c5a` | `59e537360bffe703b668e1ae76759544d03345e3` | 2026-10-20 | fix(runtime, docs): independent review 2026-10-20, findings 2, 4, 5 (package), 6 |
| `9a21dab82b9449574a518b04443320ea79098a86` | `91155171ec5e5c73a74a92101082de6905f3e3f4` | 2026-10-20 | fix(runtime): review re-check loose ends — Drop never takes the registry lock while unwin… |
| `0d97e119a830e5763912dca07ecf9af56ba3306a` | `e1ef62d4e64910f6c55889e7d727c228990a28f2` | 2026-10-20 | test(shutdown): the SIGTERM-ignoring branch is three levels deep, so the subtree-walk con… |
| `16f13c9e0dd8b5fc432d3d3a3afa4a55fae4f839` | `dfdbc778ab7049c9342f72b268dfd96bac2e4a2a` | 2026-10-20 | fix(runtime): Session's Drop ends leftover processes through shutdown(), so the shutdown … |
| `56b3f123acaef9a066b8a78810d295df5a66b3b0` | `0ac948fbbe20c7ec9e5c441dd34f4ad8736750d1` | 2026-10-20 | governance(c12.f17): INC-10 exit clarifications with the owner decisions of 2026-10-19 [P… |
| `b594900c62e2bd7129d1e3a0e3c0fc56d3a0ecfb` | `5a27d16e315f28edd560f17dbc67d502b7aed393` | 2026-10-20 | test(runtime): the session-outlive test kills and reaps a leftover command; U-006 teardow… |
| `a56cc3c9659de28cbb37d99fdcac95854aab8498` | `a7432dc3d7886cbb164ac8f764fb22d8fcb3c64b` | 2026-10-20 | docs(inc-10): change-control record — exit criteria and blocking obligations mapped to ev… |
| `632873c507595603c2c76af0a3df0ad0c307c5bc` | `c9fdcbdd72a4fdeff14b91d746a5c73c4b5063e1` | 2026-10-20 | test(inc-10): change-control review — C12.F16.6 items 3, 6, 7, 10 as worded; OB-55 confin… |
| `c3fa23b645612da1ecc079dc5f21a3160f62b5f1` | `28e7d27dce7448ab2af313e85cc6ee8348edd0b6` | 2026-10-20 | docs(inc-10): change-control record corrected after the independent review — OB-24 does n… |
| `eca016bf27ff78387a32892ea9a9f0e809449c40` | `abfda9965b88b2f5c3692b7b19895fb620f7c806` | 2026-10-20 | docs(inc-10), test: change-control record after the review's re-check — closure path (A–G… |
| `d678d328b15ea84cc2c9b8fec4734dbff976a05d` | `cead872e2a2ee726604f91d972bd5db5aa84d382` | 2026-10-20 | docs(inc-10): gates on 56b3f12 all PASS (581/0/2, 431 seeds, 429/429 DETECTED, envverify,… |
| `faf6ddac1e2bce6a680d5149315a87b4154fea4b` | `4d5be84bb07a49b105db331c554dc404b2ff0839` | 2026-10-20 | docs(inc-10): change-control record — gates on the branch tip d678d32 all PASS (586/0/2, … |
| `bbad1cd2d359c035fffa47be431fdefa7d8dc78b` | `941b6787cfdc57855b41ea92ff392ab9b9eafa30` | 2026-10-20 | docs(foundation): ratify Phase F INC-10 — zoomed navigation [ratified commit faf6dda; pre… |

## Map — commits on other local branches only (46)

| old | new | date | subject (pre-rewrite text) |
|---|---|---|---|
| `4ea174b12ddc5422494c9f320c197e28e0667f3f` | `a42ba285283ccd6623d15477d3e8fdeff153e003` | 2026-10-16 | merge: feature/clipboard-2026-10-16 (35159b9) [INTEGRATION TRIAL, NOT MAIN] |
| `3e80f3b50fe0e8d2d98291a64df3420054dee05f` | `e3c7c10246821d64fed0a4e6dc4f261dec6faa57` | 2026-10-16 | merge: feature/orderly-shutdown-2026-10-16 (bf040ca) [INTEGRATION TRIAL, NOT MAIN] |
| `300c876896c92a66c9a75d92805710718fd0ce35` | `6382649a6cfc0cd7624c26c4a755bd621f1f5490` | 2026-10-16 | merge: feature/clipboard-2026-10-16 (35159b9) onto the INC-6/8/12 rehearsal [REHEARSAL, N… |
| `4567070d8bc246335f256a87877e8cb8b903409e` | `c49279e1899820d0372eb8799ec9649a440402f3` | 2026-10-16 | merge: feature/orderly-shutdown-2026-10-16 (bf040ca) onto the INC-6/8/12 rehearsal [REHEA… |
| `1ef3e5da89e8450b15921d212c6c7e8ecd0fe95f` | `72d6fd263bd1e3af7b795c222aac4bc23926cfc1` | 2026-10-16 | fix(inc-12): the satellite runs in a private runtime directory, removed after it is reape… |
| `86b309701d503b9227796a242aa77367224cfd69` | `cf7051f56bb0e78af0e13f16749356d2e1f412d1` | 2026-10-16 | merge: fix/inc-12-satellite-runtime-dir-2026-10-16 (1ef3e5d) onto the runtime line [REHEA… |
| `be5ec805e22235d3e553b8b62a23d6b75be60539` | `a9dd5b42155f26ebb2f4d59acd5a98717150618e` | 2026-10-16 | evidence(runtime): nested runtime validation runs 1 and 2 — scripts, logs, results [REHEA… |
| `2dac05a9be1d8d08ed62575bc91b985135a4488b` | `27a45bd46fd798ae9eebfbbc10886ca478efec6e` | 2026-10-16 | docs(readiness): runtime validation plan rev. 2 — executed recipe, nested run 1/2 results… |
| `7b1e64d3d3e2e6e5e907b4498c4bf26bea550d24` | `9cdc8a17bf1496b55d758ed752172d133bbfe4b9` | 2026-10-16 | docs(inc-9): G-OB24 final decision proposal — domain, invalid entries, per-output anchor,… |
| `8f8a83447125ea9e457c138542a4c85c97b9d805` | `493967cec20445f9400063233254a0999f569350` | 2026-10-16 | docs(readiness): operator-guided nested session — runbook, run/driver scripts, interactiv… |
| `28e8efcc98c55aa6ca42be3d1336551533a9b733` | `22ee251a60c1d2c474d8356a88efd3fc97878c74` | 2026-10-16 | review(inc-12): round 2 — activation tokens bounded, a dead session's cookie and satellit… |
| `70447bfaa57a04be9b8079cafb5d49ae32330cc5` | `6b252cf98a194635a4aebf86e6538ca8e712800c` | 2026-10-16 | merge: feature/clipboard-2026-10-16 (f1e0d30) onto the runtime line r2 [REHEARSAL, NEVER … |
| `a3e2a24566549b0b75a64891f407d6eac0b8c4ed` | `00cbdf32429fa4b79ca95e6ae7e061b09e0a733d` | 2026-10-16 | merge: fix/inc-12-satellite-runtime-dir-2026-10-16 (28e8efc) onto the runtime line r2 [RE… |
| `c55bb707ec1187f1be0209ecf1cd95cc7916e0b3` | `655ccc9a551812f20f77a73f717ab6864086e927` | 2026-10-16 | merge: feature/session-process-ownership-2026-10-16 (b4c2e62) onto the runtime line r2; t… |
| `49652c72e1ffe4c083f4a3693e72e951377553bf` | `fc166ff6827688ff2271ccdd21272581b175f398` | 2026-10-16 | test(runtime line): the real-satellite T1 test registers the clients it spawns (session p… |
| `6e17ecc4151d2844554f1098601781d1bf20bda5` | `c51430acf31a16023f6c0bcf62d0183753a2df13` | 2026-10-16 | merge: feature/session-process-ownership-2026-10-16 (18173a9) onto the runtime line r2 [R… |
| `9e15cdbbf42bf569796b78d1d1ea5c77ed165811` | `08acfd78a2b87ee58125b8ec6a57fbbf39cc45e1` | 2026-10-16 | evidence(runtime): runtime line r2 — scripted sessions, satellite failure, finding NB-1 (… |
| `55536556bf7c553c520e59723976116a22da7f4a` | `6900498cf536046d6d2c05071816f3ff91f0aa63` | 2026-10-16 | docs(readiness): runbook — abort with Ctrl+\ when the nested window stalls (finding NB-1) |
| `1609f7de6e077ed3028e4209e0f86b0eedbe9beb` | `de484ec71c5e41d37d1950c4aefdff998dee7606` | 2026-10-16 | docs(readiness): runbook abort via operator-abort.sh (SIGKILL to the run's namespace init… |
| `1e3e43756064241a995ff1c6beee0c8ef80c330f` | `0d4001d7d0f9588eb4e47f055ae56d919f958aab` | 2026-10-16 | docs(readiness): operator-abort.sh quiet on processes that exit during its scan |
| `c6873e9520dd68b694e99b6fcdb12158e1047518` | `117771db0de4245a6358b3a82c2511eab287b18c` | 2026-10-16 | merge: feature/session-process-ownership-2026-10-16 (18173a9) [INTEGRATION TRIAL, NOT MAI… |
| `1585b6623e1e5802e001bb4e0dfef6a41ed265cb` | `57ca42ec2a8a279ba0886564e5d7ae87c0db9ca5` | 2026-10-16 | merge: feature/clipboard-2026-10-16 (f1e0d30) [INTEGRATION TRIAL, NOT MAIN] |
| `bf8568c8a0d6a7867dc79139d19cb25be90e80ef` | `7fe774bb7cb81a3f86b1a39c3bbd43a566056361` | 2026-10-17 | evidence(runtime): r2 note — the tested abort is operator-abort.sh, not Ctrl+\ [REHEARSAL… |
| `c4968254eb843e2f1e34f31bfbdcf182302e9c29` | `66bf9aad0165feaf68b9afd28079d14bbf5430a2` | 2026-10-17 | merge: feature/session-process-ownership-2026-10-16 (c28a121) onto the runtime line r2 [R… |
| `df90bed84920ced7e53c4d6afb291f88c22b5cfb` | `db62f8fa1b0a4b450b27d6692dec5384ccf15629` | 2026-10-17 | runtime line r2: SIGCHLD also for a session with an X display; the INC-12 test child is o… |
| `e5bf31469e9d657f6c6878f50bf3c8dbd957bac5` | `29f0687a3013e4051e2671efdee24e79379074d4` | 2026-10-17 | runtime line r2: resolve the 8322bb6 merge — client_command with spawn_owned; the satelli… |
| `d9e3ec21d7f035f97f34517630757528e2a7cc3b` | `00259ce1bae48bbc09ce3b135a61a73eebb5f976` | 2026-10-17 | merge: feature/session-process-ownership-2026-10-16 (a63fca2) onto the runtime line r2 [R… |
| `1a67c4bda0621c7ae52e14a2b1cf0a7e345512c2` | `04d1066158c1ca6f316c6481d5c41bc3a7b8fbc6` | 2026-10-17 | merge: feature/session-process-ownership-2026-10-16 (a63fca2) [INTEGRATION TRIAL, NOT MAI… |
| `df4f87b125a4950d35a181c376b34e9e2649f8de` | `3e4e695fc42659e3e9b47d6eb29890a7ac081a1a` | 2026-10-17 | evidence(runtime): runtime line r2 final build — gates, real satellite, satellite failure… |
| `ee3721b7262177d9321bafde93229c2c68a9994d` | `e9a5c0578534e94bc42fac7986e0e718166c54fb` | 2026-10-17 | docs(inc-9): OB-24 decision table for approval; animated-zoom measurement withdraws the Z… |
| `dacc0cf22d222fcfcb20a3b448454680e0b6b608` | `20fd75cd2b7ec4ec0f74fb8f874ada1ffe6f2eae` | 2026-10-17 | experiment(nb1): INFYNITAE_NB1_MINIMIZE_MS minimises the nested window to make the host w… |
| `bfbe98f957b4e9b0e94086c7f2b61dfaf9fd1e6c` | `24b9c581161d0b496fb14de14735f9a40e23ab4a` | 2026-10-17 | experiment(nb1): INFYNITAE_NB1_MINIMIZE_MS minimises the nested window to make the host w… |
| `dafc5ed8723ec6171be9b5d766263ff81b6849f3` | `9bee4e35e3474e5a237166cbce0f749c9939c432` | 2026-10-17 | experiment(nb1): INFYNITAE_NB1_MINIMIZE_MS minimises the nested window to make the host w… |
| `83415764cef9cf5b17fc3f3a9bb6704c5f7165f7` | `e0f57cd5a5fda62d85fbf8624bb94fcd075d1b4b` | 2026-10-17 | experiment(nb1): INFYNITAE_NB1_MINIMIZE_MS minimises the nested window to make the host w… |
| `d7c6c03c6df249620b35a18ca65eb74fb155a0a1` | `8081f36bc96ede2a2a1d08a6076281e6ff352dec` | 2026-10-17 | fix(nested): submit a frame only when the host is ready for it — a hidden window no longe… |
| `2f234e2a332de6839fd54b36ce0fd6a68583bb53` | `325883217f4d9e25a1844217761f0149a3247122` | 2026-10-17 | evidence(inc-9): anchor model checks — multi-output pans, re-anchor on scale change, f64 … |
| `a416efbb8f14517c568b61d2af910e212e7fb574` | `f59a3254d3f94f58ed728dcb8987ce60d97e2cbf` | 2026-10-17 | docs(inc-9): G-OB24 decision package for owner review — D1–D6 traced to ratified text; A1… |
| `7428e09e2a689e11034b241832a1366e4d7c0348` | `02ec7b6ba60701869ade7ac69d1b515d0f337329` | 2026-10-17 | experiment(nb1): the NB-1 regression test on unfixed main (must fail) [EXPERIMENT, NEVER … |
| `1727efecdae60685d125c2c2182669a0fa020376` | `a05dd1a82976d7b92f1aca6094c7147bdf3a4aaf` | 2026-10-17 | docs(inc-10): implementation brief — scope, constraints, prerequisites, planned files, te… |
| `58e1b40071d8ffb3f8c8c459564222ee48757b0f` | `c721258cd0134689db9a840644030b385fe30395` | 2026-10-16 | experiment(inc-9): A1 domain code (a156e93) re-based on main ca2e468 — INC-6 cull test's … |
| `829b1703c834956a765590c1e15fd9e48fe31697` | `9218ad8f5a2ddc927d2a387b45a8fb74f6dad654` | 2026-10-17 | docs(inc-9): decision package — A1 re-based on main: one ratified INC-6 test must move it… |
| `a5e0fce4e00097b7a6eb4659eaa8fe322e3a425f` | `f7620177f61a8d358738a59657e0e1b386823712` | 2026-10-17 | docs(session): overnight report 2026-10-17 — NB-1 verified and merge-ready (not merged), … |
| `487112fd480896da880a3fb5b01a3f419755e45e` | `5a9628f7843dc2f0314ab8a4e5cf26edb732eb47` | 2026-10-17 | docs(inc-10): remaining owner decisions with recommended defaults (fit target, bindings, … |
| `7a85689812ded3ec6dd2682df268ac37e7148841` | `77826edd5851f9e9343f72b083493330af7ca846` | 2026-10-17 | docs(session): brief 7 report — NB-1 integrated into local main (7ddde8f), OB-24 layer C1… |
| `d1c2effff5169d024667bf00bab8ce9e205d38d8` | `7f7fb7721453c4f91acd1a398616e5b8261af538` | 2026-10-18 | REHEARSAL ONLY — merge feature/clipboard-2026-10-16 (f1e0d30) onto the INC-8 rehearsal [P… |
| `a5bd01bd3d7f57306dae7d0a4d5716da5e05bf87` | `b6ec25b56cd1f22ec2e7b50bd40b87f68ebafeed` | 2026-10-18 | REHEARSAL ONLY — merge feature/session-process-ownership-2026-10-16 (df35fba: orderly SIG… |
