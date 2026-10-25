"""C12.F14 (OB-24 D4): how far may a kept lattice reference be from the view?

F-LATTICE-1 evaluates edge(x) = round((x - R)*k + delta) in f64. This model compares that f64
value with the exact value of the same expression (rational arithmetic on the same f64 k) for
visible edges at |x - R|*k ~ 2^e, on both sides of the reference, for three kinds of k:
  generic     zoom 2^U(-20,16) times a scale n/120 (30..960): arbitrary f64 mantissas
  n/120       zoom 2^j (j integer) times n/120
  tie-dyadic  zoom 2^j times 1.25, 1.5, 0.75 or 2.5: (x - R)*k hits exact half-integers
Run: python3 -I exactness_bound_model.py  (deterministic; output in exactness_bound_model.out)
"""
import math
import random
from fractions import Fraction as F

D = 2.0 ** -20


def f64_round(t):  # Rust f64::round: half away from zero
    a = abs(t)
    r = math.floor(a) + (1 if a - math.floor(a) >= 0.5 else 0)
    return math.copysign(r, t)


def exact_round(t):
    s = 1 if t >= 0 else -1
    return s * math.floor(abs(t) + F(1, 2))


def k_of(kind, rnd):
    if kind == "generic":
        return 2.0 ** rnd.uniform(-20, 16) * (rnd.randint(30, 960) / 120)
    if kind == "n/120":
        return 2.0 ** rnd.randint(-20, 16) * (rnd.randint(30, 960) / 120)
    return 2.0 ** rnd.randint(-20, 16) * rnd.choice([1.25, 1.5, 0.75, 2.5])


def trial(e, kind, rnd, n_k=60, n_x=200):
    bad = tot = 0
    for _ in range(n_k):
        k = k_of(kind, rnd)
        off = int(2 ** e / k)  # |v - R| in Canvas px
        if off < 1 or off > 2 ** 37:  # a reference is a Canvas value a view origin had
            continue
        for sign in (1, -1):
            for _ in range(n_x):
                d = sign * (off + rnd.randint(0, int(2 ** 15 / k) + 1))  # x - R, visible x
                bad += f64_round(d * k + D) != exact_round(d * F(k) + F(D))
                tot += 1
    return bad, tot


if __name__ == "__main__":
    rnd = random.Random(20261017)
    print("wrong f64 edges / edges checked, by |x - R|*k ~ 2^e")
    for e in (26, 31, 32, 33, 34, 35, 36, 40, 46, 50):
        cells = "  ".join(f"{kind} {b}/{t}" for kind in ("generic", "n/120", "tie-dyadic") for b, t in [trial(e, kind, rnd)])
        print(f"2^{e}: {cells}")
