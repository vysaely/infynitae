//! B1 numeric-model experiment for the Infynitae Native foundation program.
//!
//! Pure arithmetic. No compositor, no Wayland, no Smithay. Deterministic
//! (fixed-seed LCG). Output is recorded verbatim under
//! docs/archive/evidence/phase-b/b1-numeric-output.txt.
//!
//! Candidate representations compared:
//!   F64      : Canvas/camera as f64 logical pixels
//!   F32      : included only to rule it out
//!   I32      : Prototype 01 (integer logical pixels, no subpixel)
//!   Q16      : i64 fixed point, 16 fractional bits (1/65536 logical px)
//!   Q16-G    : Q16 camera, but a gesture's deltas are summed in f64 relative
//!              to the gesture start and quantized once per update.

const FRAC_BITS: u32 = 16;
const ONE: i64 = 1 << FRAC_BITS;

fn q16_from_f64(v: f64) -> i64 {
    // Rounding point. Ties-to-even keeps quantization unbiased in aggregate.
    (v * ONE as f64).round_ties_even() as i64
}
fn q16_to_f64(q: i64) -> f64 {
    q as f64 / ONE as f64
}

/// Deterministic LCG (Numerical Recipes constants); no external crates.
struct Lcg(u64);
impl Lcg {
    fn next_u32(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 32) as u32
    }
    /// A trackpad-like delta: magnitude 0.05..4.0 px, random sign, quantized to
    /// 2^-20 so the exact sum is computable in i128 (2^-20 is finer than Q16,
    /// so Q16 quantization error is exercised).
    fn delta_2p20(&mut self) -> i64 {
        let mag = 0.05 + (self.next_u32() as f64 / u32::MAX as f64) * 3.95;
        let k = (mag * (1u64 << 20) as f64) as i64;
        if self.next_u32() & 1 == 0 { k } else { -k }
    }
}
const P20: f64 = (1u64 << 20) as f64;

fn ulp(x: f64) -> f64 {
    let b = x.abs().to_bits();
    f64::from_bits(b + 1) - f64::from_bits(b)
}
fn ulp32(x: f32) -> f32 {
    let b = x.abs().to_bits();
    f32::from_bits(b + 1) - f32::from_bits(b)
}

fn e1_ulp_table() {
    println!("== E1: representable spacing (ulp) by magnitude, logical px ==");
    println!("{:>22} {:>14} {:>14} {:>14}", "magnitude", "f64 ulp", "f32 ulp", "Q16 step");
    for &(name, m) in &[
        ("1e3", 1e3),
        ("1e6", 1e6),
        ("2^31 (i32 max)", 2147483648.0),
        ("1e9 (Hyprland bound)", 1e9),
        ("2^36", 68719476736.0),
        ("1e12", 1e12),
        ("2^47 (Q16 i64 max)", 140737488355328.0),
    ] {
        println!(
            "{:>22} {:>14.3e} {:>14.3e} {:>14.3e}",
            name,
            ulp(m),
            ulp32(m as f32) as f64,
            1.0 / ONE as f64
        );
    }
    println!();
}

/// Camera accumulation under N trackpad deltas, starting at `start` (integer px).
fn e2_accumulation(start_px: i64, n: usize, gesture_len: usize) {
    let mut rng = Lcg(0x1f2e3d4c5b6a7988);
    let exact_start_2p20 = (start_px as i128) << 20;
    let mut exact = exact_start_2p20; // units of 2^-20 px

    let mut cam_f64 = start_px as f64;
    let mut cam_q16_event = start_px << FRAC_BITS;
    let mut cam_q16_g_start = start_px << FRAC_BITS;
    let mut gesture_sum = 0.0f64;
    let mut cam_q16_g;
    let mut cam_i32_round = start_px; // prototype: round each delta to integer

    let (mut err_f64, mut err_qe, mut err_qg, mut err_i) = (0f64, 0f64, 0f64, 0f64);
    for i in 0..n {
        let k = rng.delta_2p20();
        let d = k as f64 / P20;
        exact += k as i128;

        cam_f64 += d;
        cam_q16_event += q16_from_f64(d);
        gesture_sum += d;
        cam_q16_g = cam_q16_g_start + q16_from_f64(gesture_sum);
        cam_i32_round += d.round() as i64;
        if (i + 1) % gesture_len == 0 {
            cam_q16_g_start = cam_q16_g;
            gesture_sum = 0.0;
        }

        // Errors relative to the exact sum (exact is in 2^-20 units).
        let exact_rel = (exact - exact_start_2p20) as f64 / P20; // offset from start, exact in f64 here
        err_f64 = err_f64.max(((cam_f64 - start_px as f64) - exact_rel).abs());
        err_qe = err_qe.max((q16_to_f64(cam_q16_event - (start_px << FRAC_BITS)) - exact_rel).abs());
        err_qg = err_qg.max((q16_to_f64(cam_q16_g - (start_px << FRAC_BITS)) - exact_rel).abs());
        err_i = err_i.max(((cam_i32_round - start_px) as f64 - exact_rel).abs());
    }
    println!(
        "start={:>14} n={:>9} gesture_len={:>4} | max|err| px: f64={:.3e}  Q16/event={:.3e}  Q16-G={:.3e}  I32-round/event={:.3e}",
        start_px, n, gesture_len, err_f64, err_qe, err_qg, err_i
    );
}

/// Constant non-representable delta: worst case for systematic bias.
fn e2b_constant_delta(start_px: i64, d: f64, n: usize) {
    let mut f = start_px as f64;
    let mut qe = start_px << FRAC_BITS;
    let mut ir = start_px;
    for _ in 0..n {
        f += d;
        qe += q16_from_f64(d);
        ir += d.round() as i64;
    }
    let ideal = d * n as f64; // computed once; error of this reference itself ~1e-16 relative
    let qg = q16_to_f64(q16_from_f64(d * n as f64)); // Q16-G with one gesture
    println!(
        "start={:>14} delta={} n={:>9} | drift px: f64={:+.3e}  Q16/event={:+.3e}  Q16-G(1 gesture)={:+.3e}  I32-round/event={:+.3e}",
        start_px,
        d,
        n,
        (f - start_px as f64) - ideal,
        q16_to_f64(qe - (start_px << FRAC_BITS)) - ideal,
        qg - ideal,
        (ir - start_px) as f64 - ideal
    );
}

/// Non-associativity: same deltas, two batchings (per event vs coalesced pairs).
fn e3_path_dependence(start_px: i64, n: usize) {
    let mut rng = Lcg(42);
    let ds: Vec<f64> = (0..n).map(|_| rng.delta_2p20() as f64 / P20 + 1e-7).collect();
    let mut a = start_px as f64;
    for d in &ds {
        a += d;
    }
    let mut b = start_px as f64;
    for pair in ds.chunks(2) {
        b += pair.iter().sum::<f64>();
    }
    let mut qa = start_px << FRAC_BITS;
    for d in &ds {
        qa += q16_from_f64(*d);
    }
    let mut qb = start_px << FRAC_BITS;
    for pair in ds.chunks(2) {
        qb += pair.iter().map(|d| q16_from_f64(*d)).sum::<i64>();
    }
    println!(
        "start={:>14} n={} | f64 per-event vs coalesced differ by {:.3e} px (bitwise equal: {}) | Q16 equal: {}",
        start_px,
        n,
        (a - b).abs(),
        a.to_bits() == b.to_bits(),
        qa == qb
    );
}

/// Relative jitter between two elements during a sub-pixel pan sweep.
fn e4_jitter(scale: f64) {
    // camera sweep 0..10 px in 0.01 px steps (Q16)
    let elems: Vec<i64> = (1000..1064).collect();
    let mut a_min = i64::MAX;
    let mut a_max = i64::MIN;
    let mut b_min = i64::MAX;
    let mut b_max = i64::MIN;
    let mut a_changes = 0usize;
    let mut b_changes = 0usize;
    let mut prev_a: Option<Vec<i64>> = None;
    let mut prev_b: Option<Vec<i64>> = None;
    for step in 0..=1000 {
        let c = q16_from_f64(step as f64 * 0.01);
        let cf = q16_to_f64(c);
        // Scheme A: round((e - c) * s) per element
        let pa: Vec<i64> = elems.iter().map(|&e| ((e as f64 - cf) * scale).round() as i64).collect();
        // Scheme B: round(e * s) - round(c * s)  (element and camera on the output's physical grid)
        let pb: Vec<i64> = elems
            .iter()
            .map(|&e| (e as f64 * scale).round() as i64 - (cf * scale).round() as i64)
            .collect();
        for w in 0..elems.len() - 1 {
            let da = pa[w + 1] - pa[w];
            let db = pb[w + 1] - pb[w];
            a_min = a_min.min(da);
            a_max = a_max.max(da);
            b_min = b_min.min(db);
            b_max = b_max.max(db);
        }
        if let Some(p) = &prev_a {
            // count steps where some neighbour distance changed under A
            for w in 0..elems.len() - 1 {
                if (pa[w + 1] - pa[w]) != (p[w + 1] - p[w]) {
                    a_changes += 1;
                    break;
                }
            }
        }
        if let Some(p) = &prev_b {
            for w in 0..elems.len() - 1 {
                if (pb[w + 1] - pb[w]) != (p[w + 1] - p[w]) {
                    b_changes += 1;
                    break;
                }
            }
        }
        prev_a = Some(pa);
        prev_b = Some(pb);
    }
    println!(
        "scale={:<5} | neighbour distance range (phys px): A=[{},{}] B=[{},{}] | sweep steps where any neighbour distance changed: A={}/1000 B={}/1000",
        scale, a_min, a_max, b_min, b_max, a_changes, b_changes
    );
}

/// Snapped adjacency at fractional scale: element 2 starts exactly where element 1 ends.
fn e5_adjacency() {
    let mut worst_i32root = 0usize;
    let mut worst_f64root = 0usize;
    let mut worst_f64bias = 0usize;
    let mut worst_vs_exact = 0usize;
    const DELTA: f64 = 1.0 / (1u64 << 20) as f64;
    let mut total = 0usize;
    let mut worst_scale_i32 = 0.0;
    for k in 120..=360u32 {
        // wp_fractional_scale_v1 scales are n/120
        let s = k as f64 / 120.0;
        let mut bad_i32 = 0usize;
        let mut bad_f64 = 0usize;
        let mut bad_bias = 0usize;
        let mut bad_exact = 0usize;
        for e1 in (0..600).step_by(7) {
            for w1 in (100..1600).step_by(13) {
                let e2 = e1 + w1;
                total += 1;
                // i32 root location (what render_elements_from_surface_tree takes):
                // root = round(e*s); size = round(root + w*s) - root  (Smithay edge rounding)
                let r1 = (e1 as f64 * s).round();
                let right1 = (r1 + w1 as f64 * s).round() as i64;
                let left2 = (e2 as f64 * s).round() as i64;
                if right1 != left2 {
                    bad_i32 += 1;
                }
                // f64 root location (WaylandSurfaceRenderElement::from_surface): edges rounded from canvas grid
                let right1f = (e1 as f64 * s + w1 as f64 * s).round() as i64;
                if right1f != left2 {
                    bad_f64 += 1;
                }
                // f64 root location with a constant tie-breaking bias DELTA on every edge
                let right1b = (e1 as f64 * s + w1 as f64 * s + DELTA).round() as i64;
                let left2b = (e2 as f64 * s + DELTA).round() as i64;
                if right1b != left2b {
                    bad_bias += 1;
                }
                // biased edge vs exact rational rounding of (x*k/120 + tiny) computed in integers:
                // floor((x*k*2 + 120) / 240) == round-half-up of x*k/120
                let exact = |x: i64| -> i64 { (x * k as i64 * 2 + 120).div_euclid(240) };
                if left2b != exact(e2) {
                    bad_exact += 1;
                }
            }
        }
        if bad_i32 > worst_i32root {
            worst_i32root = bad_i32;
            worst_scale_i32 = s;
        }
        worst_f64root = worst_f64root.max(bad_f64);
        worst_f64bias = worst_f64bias.max(bad_bias);
        worst_vs_exact = worst_vs_exact.max(bad_exact);
    }
    println!(
        "scales n/120 for n=120..=360, {} adjacency cases | gap/overlap per scale, worst: i32 root location {} (at scale {:.4}); f64 root location {}; f64 root + bias 2^-20 {}; biased edge != exact round-half-up {}",
        total, worst_i32root, worst_scale_i32, worst_f64root, worst_f64bias, worst_vs_exact
    );
}

/// Large-coordinate subtraction: exact Q16 vs f64.
fn e6_large_subtraction() {
    for &e in &[1_000_000_000i64, (1i64 << 36) - 5000, (1i64 << 40)] {
        let c_q = (e << FRAC_BITS) - q16_from_f64(1234.56789);
        let exact = q16_to_f64((e << FRAC_BITS) - c_q); // exact: |delta| small
        let f = e as f64 - q16_to_f64(c_q);
        let i32_ok = i32::try_from(e).is_ok();
        println!(
            "element at {:>15} | Q16 delta={:.10} f64 delta={:.10} |diff|={:.3e} | fits i32: {}",
            e,
            exact,
            f,
            (exact - f).abs(),
            i32_ok
        );
    }
}

/// Unproject/project round trip at zoom.
fn e7_roundtrip() {
    let mut rng = Lcg(7);
    for &zoom in &[1.0f64, 0.37, 2.5] {
        for &cam_px in &[0i64, 1_000_000_000, (1i64 << 36) - 4096] {
            let cam = (cam_px << FRAC_BITS) + q16_from_f64(0.3141);
            let mut worst = 0f64;
            for _ in 0..100_000 {
                let p = (rng.next_u32() as f64 / u32::MAX as f64) * 3840.0; // output-local logical
                let canvas = cam + q16_from_f64(p / zoom); // rounding point: unproject -> Canvas (Q16)
                let back = q16_to_f64(canvas - cam) * zoom; // project; delta exact, then f64 multiply
                worst = worst.max((back - p).abs());
            }
            println!(
                "zoom={:<5} camera≈{:>13} | max |project(unproject(p)) - p| = {:.3e} logical px (bound: zoom*2^-17 = {:.3e})",
                zoom,
                cam_px,
                worst,
                zoom / (1u64 << 17) as f64
            );
        }
    }
}

fn e8_equality() {
    let z = 0.0f64;
    let nz = -0.0f64;
    println!(
        "f64: 0.0 == -0.0 -> {}, bits equal -> {}; NaN == NaN -> {}; 0.1+0.2 == 0.3 -> {}",
        z == nz,
        z.to_bits() == nz.to_bits(),
        { let n = f64::NAN; #[allow(invalid_nan_comparisons)] let r = n == n; r },
        0.1 + 0.2 == 0.3
    );
    let v = (123_456_789i64 << FRAC_BITS) + 12345;
    let s = v.to_string();
    println!("Q16: {} -> \"{}\" -> {} (round trip exact: {})", v, s, s.parse::<i64>().unwrap(), s.parse::<i64>().unwrap() == v);
    let f = 68719476735.123456789f64;
    let fs = f.to_string();
    println!("f64: Display round trip of {} -> exact: {}", fs, fs.parse::<f64>().unwrap().to_bits() == f.to_bits());
}

/// Edge rounding at extreme coordinates: absolute x*s (naive) vs relative to a
/// lattice reference R (multiple of 120 => R*s integral for every n/120 scale).
fn e9_large_edges() {
    const DELTA: f64 = 1.0 / (1u64 << 20) as f64;
    for &base in &[0i64, 1_000_000_000, (1i64 << 36) - 100_000] {
        let mut bad_naive = 0usize;
        let mut bad_lattice = 0usize;
        let mut differs_from_origin = 0usize;
        let mut total = 0usize;
        for k in 120..=360i64 {
            let s = k as f64 / 120.0;
            let r = base.div_euclid(120) * 120; // lattice reference near the camera
            for e1 in (0..600).step_by(11) {
                for w1 in (100..1600).step_by(17) {
                    total += 1;
                    let (x1, x2) = (base + e1, base + e1 + w1);
                    // naive: absolute canvas coordinate times scale, biased
                    let r1 = (x1 as f64 * s + w1 as f64 * s + DELTA).round() as i64;
                    let l2 = (x2 as f64 * s + DELTA).round() as i64;
                    if r1 != l2 { bad_naive += 1; }
                    // lattice-relative: exact integer offset (x - R), small; R*s integral
                    let edge = |x: i64| ((x - r) as f64 * s + DELTA).round() as i64;
                    let right1 = (((x1 - r) as f64) * s + (w1 as f64) * s + DELTA).round() as i64;
                    if right1 != edge(x2) { bad_lattice += 1; }
                    // same scene at origin must give identical relative physical geometry
                    let e0 = |x: i64| (x as f64 * s + DELTA).round() as i64;
                    let rel_far = edge(x2) - edge(x1);
                    let rel_origin = e0(e1 + w1 + (base - r)) - e0(e1 + (base - r));
                    if rel_far != rel_origin { differs_from_origin += 1; }
                }
            }
        }
        println!(
            "base={:>13} cases={} | adjacency gap/overlap: naive absolute {} ; lattice-relative {} | relative geometry differs from same scene near origin: {}",
            base, total, bad_naive, bad_lattice, differs_from_origin
        );
    }
}

fn main() {
    println!("B1 numeric experiment — infynitae-native/experiments/b1-numeric");
    println!("Q16: i64 with {} fractional bits; step = {:e} px\n", FRAC_BITS, 1.0 / ONE as f64);
    e1_ulp_table();

    println!("== E2: camera accumulation vs exact sum (random trackpad deltas 0.05..4 px, 2^-20 granular) ==");
    for &start in &[0i64, 1_000_000_000, (1i64 << 36) - 1] {
        e2_accumulation(start, 1_000_000, 200);
        e2_accumulation(start, 10_000_000, 200);
    }
    println!("\n== E2b: constant non-representable delta 0.1 px (systematic bias) ==");
    for &start in &[0i64, 1_000_000_000, (1i64 << 36) - 1] {
        e2b_constant_delta(start, 0.1, 1_000_000);
        e2b_constant_delta(start, 0.3, 1_000_000);
    }
    println!("\n== E3: path dependence (same deltas, per-event vs coalesced pairs) ==");
    for &start in &[0i64, 1_000_000_000, (1i64 << 36) - 1] {
        e3_path_dependence(start, 1_000_000);
    }
    println!("\n== E4: relative jitter between neighbouring elements during a sub-pixel pan ==");
    for &s in &[1.0, 1.25, 1.5, 1.75, 2.0] {
        e4_jitter(s);
    }
    println!("\n== E5: snapped adjacency at fractional scales ==");
    e5_adjacency();
    println!("\n== E6: large-coordinate camera subtraction ==");
    e6_large_subtraction();
    println!("\n== E7: unproject -> Q16 -> project round trip ==");
    e7_roundtrip();
    println!("\n== E9: physical edge rounding at extreme coordinates ==");
    e9_large_edges();
    println!("\n== E8: equality and serialization semantics ==");
    e8_equality();
}
