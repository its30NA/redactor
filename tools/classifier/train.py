#!/usr/bin/env python3
"""Train the token classifier of redactor-core.

Writes two files into crates/redactor-core/src/classifier/:
  model.rs     feature scaling folded into logistic-regression weights, plus parity vectors
  trigram.bin  27^3 int8 log2-probabilities of lowercase letter trigrams (scaled by 8)

Deterministic: the same seed and the same source trees give byte-identical output.
Only statistics leave this script; no text from the sources is written anywhere.

Usage: python3 tools/classifier/train.py [--seed N] [--out DIR]
"""

from __future__ import annotations

import argparse
import base64
import math
import os
import random
import re
import string
import sys
from pathlib import Path

import numpy as np

REPO = Path(__file__).resolve().parents[2]
DEFAULT_OUT = REPO / "crates" / "redactor-core" / "src" / "classifier"

# ----------------------------------------------------------------------------------------
# Features. crates/redactor-core/src/classifier/features.rs mirrors this exactly; the
# parity vectors written into model.rs keep the two in step.
# ----------------------------------------------------------------------------------------

TRI_SCALE = 8.0  # trigram.bin holds round(log2(p) * TRI_SCALE), clipped to [-127, 0]
TRI_NONE = -12.0  # average trigram log2-probability used when a token has no letters
VOWELS = set("aeiou")
WALK_ROWS = ["1234567890", "qwertyuiop", "asdfghjkl", "zxcvbnm", "abcdefghijklmnopqrstuvwxyz"]
B64_CHARS = set(string.ascii_letters + string.digits + "+/=_-")
B58_CHARS = set("123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz")
HEX_CHARS = set("0123456789abcdefABCDEF")
UUID_RE = re.compile(r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$")

FEATURE_NAMES = [
    "ln_len",
    "frac_lower",
    "frac_upper",
    "frac_digit",
    "frac_symbol",
    "classes",
    "entropy",
    "entropy_rel",
    "transitions",
    "longest_run",
    "vowel_ratio",
    "trigram_avg",
    "trigram_min3",
    "repeat_ratio",
    "keyboard_walk",
    "hex_shape",
    "b64_shape",
    "b58_shape",
    "uuid_shape",
    "ln_len_x_entropy_rel",
    "ln_len_x_trigram",
    "classes_x_transitions",
]


def char_class(c: str) -> int:
    o = ord(c)
    if 97 <= o <= 122:
        return 0
    if 65 <= o <= 90:
        return 1
    if 48 <= o <= 57:
        return 2
    return 3


def tri_index(c: str) -> int:
    o = ord(c)
    if 97 <= o <= 122:
        return o - 97
    if 65 <= o <= 90:
        return o - 65
    return 26


MAX_CHARS = 256  # longer tokens are scored on their first 256 characters


def features(tok: str, tri: np.ndarray) -> list[float]:
    tok = tok[:MAX_CHARS]
    chars = list(tok)
    n = len(chars)
    nf = float(n)
    counts = [0, 0, 0, 0]
    cls = [char_class(c) for c in chars]
    for k in cls:
        counts[k] += 1
    present = [c > 0 for c in counts]
    classes = float(sum(present))

    freq: dict[str, int] = {}
    for c in chars:
        freq[c] = freq.get(c, 0) + 1
    entropy = 0.0
    for c in sorted(freq):
        p = freq[c] / nf
        entropy -= p * math.log2(p)
    alphabet = 26 * present[0] + 26 * present[1] + 10 * present[2] + 32 * present[3]
    cap = min(n, alphabet)
    entropy_rel = entropy / math.log2(cap) if cap > 1 else 0.0

    transitions = sum(1 for i in range(1, n) if cls[i] != cls[i - 1])
    trans = transitions / (nf - 1.0) if n > 1 else 0.0
    longest = 1
    run = 1
    for i in range(1, n):
        run = run + 1 if cls[i] == cls[i - 1] else 1
        longest = max(longest, run)
    longest_run = longest / nf

    letters = counts[0] + counts[1]
    vowels = sum(1 for c, k in zip(chars, cls) if k < 2 and c.lower() in VOWELS)
    # ASCII-only lowercasing (Unicode case mapping differs between languages).
    low = "".join(c.lower() if k == 1 else c for c, k in zip(chars, cls))
    vowel_ratio = vowels / letters if letters else 0.0

    # Trigram log-likelihood over letters; context starts as two boundaries.
    idx = [tri_index(c) for c in chars]
    logps = []
    a, b = 26, 26
    for c in idx:
        if c < 26:
            logps.append(float(tri[(a * 27 + b) * 27 + c]) / TRI_SCALE)
        a, b = b, c
    if logps:
        tri_avg = sum(logps) / len(logps)
        # The least likely window of three consecutive letter scores.
        if len(logps) >= 3:
            tri_min3 = min(sum(logps[i : i + 3]) / 3.0 for i in range(len(logps) - 2))
        else:
            tri_min3 = tri_avg
    else:
        tri_avg = TRI_NONE
        tri_min3 = TRI_NONE

    if n > 1:
        bigrams = [tok[i : i + 2] for i in range(n - 1)]
        repeat_ratio = 1.0 - len(set(bigrams)) / float(len(bigrams))
    else:
        repeat_ratio = 0.0

    walk = 0.0
    for row in WALK_ROWS:
        for i in range(len(row) - 3):
            seg = row[i : i + 4]
            if seg in low or seg[::-1] in low:
                walk = 1.0
                break
        if walk:
            break

    hex_shape = 1.0 if n >= 8 and all(c in HEX_CHARS for c in chars) else 0.0
    b64_shape = (
        1.0 if n >= 16 and all(c in B64_CHARS for c in chars) and present[0] and present[1] and present[2] else 0.0
    )
    b58_shape = (
        1.0 if n >= 16 and all(c in B58_CHARS for c in chars) and present[0] and present[1] and present[2] else 0.0
    )
    uuid_shape = 1.0 if UUID_RE.match(tok) else 0.0

    ln_len = math.log(nf)
    return [
        ln_len,
        counts[0] / nf,
        counts[1] / nf,
        counts[2] / nf,
        counts[3] / nf,
        classes,
        entropy,
        entropy_rel,
        trans,
        longest_run,
        vowel_ratio,
        tri_avg,
        tri_min3,
        repeat_ratio,
        walk,
        hex_shape,
        b64_shape,
        b58_shape,
        uuid_shape,
        ln_len * entropy_rel,
        ln_len * tri_avg,
        classes * trans,
    ]


# ----------------------------------------------------------------------------------------
# Negative sources: identifiers and words from public source trees on this machine.
# ----------------------------------------------------------------------------------------

IDENT_RE = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")
STRING_RE = re.compile(r"\"(?:[^\"\\\n]|\\.)*\"|'(?:[^'\\\n]|\\.)*'")
FILE_CAP = 64 * 1024
TOTAL_CAP = 48 * 1024 * 1024


# Pinned inputs, so the model does not change when unrelated crates are downloaded: the
# Python standard library of this interpreter version (without installed packages) and the
# crates redactor-core itself is built from, at their Cargo.lock versions.
PYTHON_STDLIB = Path("/usr/lib/python3.12")
RUST_CRATES = [
    "aho-corasick-1.1.5",
    "arrayvec-0.7.8",
    "blake3-1.8.7",
    "block-buffer-0.10.4",
    "cc-1.11.1",
    "cfg-if-1.0.5",
    "constant_time_eq-0.4.2",
    "cpufeatures-0.2.17",
    "cpufeatures-0.3.1",
    "crypto-common-0.1.7",
    "digest-0.10.7",
    "find-msvc-tools-0.1.15",
    "generic-array-0.14.7",
    "itoa-1.0.18",
    "memchr-2.8.3",
    "proc-macro2-1.0.107",
    "quote-1.0.47",
    "regex-1.13.1",
    "regex-automata-0.4.18",
    "regex-syntax-0.8.11",
    "serde-1.0.229",
    "serde_core-1.0.229",
    "serde_derive-1.0.229",
    "serde_json-1.0.154",
    "sha2-0.10.9",
    "shlex-2.0.1",
    "syn-3.0.9",
    "tinyvec-1.13.3",
    "typenum-1.20.1",
    "unicode-ident-1.0.27",
    "unicode-normalization-0.1.26",
    "version_check-0.9.5",
    "zmij-1.0.23",
]


def source_files() -> list[Path]:
    files: list[Path] = []
    if PYTHON_STDLIB.is_dir():
        files.extend(
            f
            for f in sorted(PYTHON_STDLIB.rglob("*.py"))
            if "site-packages" not in f.parts and "dist-packages" not in f.parts
        )
    registry = Path.home() / ".cargo" / "registry" / "src"
    for name in RUST_CRATES:
        for crate in sorted(registry.glob(f"*/{name}")):
            files.extend(sorted(crate.rglob("*.rs")))
    return files


def harvest(rng: random.Random) -> tuple[list[str], list[str], list[str]]:
    """Identifiers, comment/docstring words, and dictionary words (if a dictionary exists)."""
    files = source_files()
    rng.shuffle(files)
    idents: dict[str, None] = {}
    words: dict[str, None] = {}
    total = 0
    for f in files:
        if total >= TOTAL_CAP:
            break
        try:
            with open(f, "rb") as fh:
                data = fh.read(FILE_CAP)
        except OSError:
            continue
        total += len(data)
        text = data.decode("utf-8", errors="ignore")
        # String literals hold test data (keys, digests); only code and comments are used.
        code = STRING_RE.sub(" ", text)
        for line in code.splitlines():
            s = line.strip()
            comment = s.startswith(("#", "//", "*", "///", "//!"))
            for m in IDENT_RE.finditer(line):
                t = m.group(0)
                if len(t) < 3 or len(t) > 64:
                    continue
                if comment and t.isalpha():
                    words[t] = None
                else:
                    idents[t] = None
    dict_words: dict[str, None] = {}
    dict_path = Path("/usr/share/dict/words")
    if dict_path.is_file():
        for w in dict_path.read_text(errors="ignore").splitlines():
            w = w.strip()
            if 3 <= len(w) <= 30 and w.isalpha() and w.isascii():
                dict_words[w] = None
    return sorted(idents), sorted(words), sorted(dict_words)


def looks_random(t: str) -> bool:
    """Drops identifiers that are themselves random-looking data (generated tables, test
    vectors outside quotes). Crude on purpose: long runs without vowels or long digit runs."""
    if re.search(r"[0-9]{7,}", t):
        return True
    letters = re.sub(r"[^A-Za-z]", "", t)
    if len(letters) >= 12 and sum(c.lower() in VOWELS for c in letters) / len(letters) < 0.15:
        return True
    return False


# ----------------------------------------------------------------------------------------
# Generated samples
# ----------------------------------------------------------------------------------------

BASE62 = string.digits + string.ascii_uppercase + string.ascii_lowercase
B64 = string.ascii_uppercase + string.ascii_lowercase + string.digits + "+/"
B64URL = string.ascii_uppercase + string.ascii_lowercase + string.digits + "-_"
B58 = "".join(sorted(B58_CHARS))
PRINTABLE = string.ascii_letters + string.digits + "!#$%&*+-.:;<=>?@^_~"
SYMBOLS = "!#$%&*+-.?@_~"
VENDOR = [
    ("ghp_", BASE62, 36),
    ("gho_", BASE62, 36),
    ("github_pat_", BASE62 + "_", 82),
    ("glpat-", BASE62 + "-_", 20),
    ("sk-", BASE62, 48),
    ("sk-proj-", BASE62 + "-_", 64),
    ("xoxb-", string.digits, 12),
    ("AKIA", string.ascii_uppercase + string.digits, 16),
    ("AIza", BASE62 + "-_", 35),
    ("hf_", BASE62, 34),
    ("npm_", BASE62, 36),
    ("SG.", BASE62 + "-_", 22),
    ("sk_live_", BASE62, 24),
    ("dop_v1_", "0123456789abcdef", 64),
    ("pypi-Ag", BASE62 + "-_", 60),
]


def rand_str(rng: random.Random, alphabet: str, n: int) -> str:
    return "".join(rng.choice(alphabet) for _ in range(n))


def positives(rng: random.Random, n: int, wordlist: list[str]) -> list[str]:
    out = []
    while len(out) < n:
        kind = rng.random()
        if kind < 0.22:
            t = rand_str(rng, BASE62, rng.randint(8, 64))
        elif kind < 0.36:
            raw = bytes(rng.getrandbits(8) for _ in range(rng.randint(9, 48)))
            t = base64.b64encode(raw).decode()
            if rng.random() < 0.5:
                t = t.rstrip("=")
        elif kind < 0.44:
            raw = bytes(rng.getrandbits(8) for _ in range(rng.randint(9, 48)))
            t = base64.urlsafe_b64encode(raw).decode().rstrip("=")
        elif kind < 0.54:
            t = rand_str(rng, "0123456789abcdef", rng.randint(16, 64))
            if rng.random() < 0.2:
                t = t.upper()
        elif kind < 0.60:
            t = rand_str(rng, B58, rng.randint(16, 50))
        elif kind < 0.74:
            t = rand_str(rng, PRINTABLE, rng.randint(8, 32))
        elif kind < 0.86:
            # Human-style: Word + digits + symbol, two words with separators.
            w = rng.choice(wordlist)
            w = w.capitalize() if rng.random() < 0.6 else w
            parts = [w, str(rng.randint(0, 9999))]
            if rng.random() < 0.7:
                parts.append(rng.choice(SYMBOLS))
            if rng.random() < 0.4:
                parts.append(rng.choice(wordlist).capitalize())
            if rng.random() < 0.3:
                parts.append(rand_str(rng, BASE62, rng.randint(2, 4)))
            sep = rng.choice(["", "", "-", "_", ".", "!", "@"])
            t = sep.join(parts)
        else:
            prefix, alphabet, ln = rng.choice(VENDOR)
            t = prefix + rand_str(rng, alphabet, ln)
        if len(t) >= 4:
            out.append(t)
    return out


def split_ident(t: str) -> list[str]:
    parts = re.findall(r"[A-Z]+(?=[A-Z][a-z])|[A-Z]?[a-z]+|[A-Z]+|[0-9]+", t)
    return [p.lower() for p in parts if p]


def compose(rng: random.Random, pieces: list[str]) -> str:
    style = rng.randrange(6)
    if style == 0:
        return pieces[0] + "".join(p.capitalize() for p in pieces[1:])
    if style == 1:
        return "".join(p.capitalize() for p in pieces)
    if style == 2:
        return "_".join(pieces)
    if style == 3:
        return "-".join(pieces)
    if style == 4:
        return "_".join(pieces).upper()
    return ".".join(pieces)


def generated_negatives(rng: random.Random, n: int, vocab: list[str]) -> list[str]:
    out = []
    while len(out) < n:
        k = rng.random()
        if k < 0.40:
            pieces = [rng.choice(vocab) for _ in range(rng.randint(2, 5))]
            t = compose(rng, pieces)
            if rng.random() < 0.15:
                t += str(rng.randint(0, 99))
        elif k < 0.52:
            y, mo, d = rng.randint(1970, 2040), rng.randint(1, 12), rng.randint(1, 28)
            h, mi, s = rng.randint(0, 23), rng.randint(0, 59), rng.randint(0, 59)
            t = rng.choice(
                [
                    f"{y}-{mo:02}-{d:02}",
                    f"{y}{mo:02}{d:02}",
                    f"{d:02}.{mo:02}.{y}",
                    f"{mo:02}/{d:02}/{y}",
                    f"{y}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z",
                    f"{y}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}.{rng.randint(0, 999):03}+01:00",
                    f"{h:02}:{mi:02}:{s:02}",
                    f"{y}{mo:02}{d:02}T{h:02}{mi:02}{s:02}",
                ]
            )
        elif k < 0.62:
            v = [str(rng.randint(0, 30)) for _ in range(rng.randint(2, 4))]
            t = ".".join(v)
            t = rng.choice(["", "v", "V", "r"]) + t
            if rng.random() < 0.3:
                t += rng.choice(["-alpha", "-beta.", "-rc.", "+build.", "-dev", ".post"]) + str(rng.randint(0, 20))
        elif k < 0.74:
            segs = [rng.choice(vocab) for _ in range(rng.randint(1, 5))]
            ext = rng.choice(["", ".py", ".rs", ".txt", ".json", ".toml", ".yaml", ".so.6", ".log"])
            t = rng.choice(["/", "./", "", "~/", "C:\\"]) + rng.choice(["/", "\\"]).join(segs) + ext
            if t.startswith("C:\\"):
                t = t.replace("/", "\\")
            else:
                t = t.replace("\\", "/")
        elif k < 0.80:
            u = "%08x-%04x-%04x-%04x-%012x" % (
                rng.getrandbits(32),
                rng.getrandbits(16),
                rng.getrandbits(16),
                rng.getrandbits(16),
                rng.getrandbits(48),
            )
            t = u.upper() if rng.random() < 0.2 else u
        elif k < 0.84:
            t = "#" + rand_str(rng, "0123456789abcdef", rng.choice([3, 6, 8]))
        elif k < 0.88:
            t = rng.choice(
                [
                    f"{rng.randint(1, 255)}.{rng.randint(0, 255)}.{rng.randint(0, 255)}.{rng.randint(0, 255)}",
                    f"{rng.randint(1, 999)},{rng.randint(0, 999):03},{rng.randint(0, 999):03}",
                    f"{rng.randint(0, 99999)}.{rng.randint(0, 99)}",
                    f"{rng.randint(1, 65535)}/tcp",
                    f"{rng.choice(['en', 'de', 'fr', 'fa'])}_{rng.choice(['US', 'DE', 'FR', 'IR'])}.UTF-8",
                    f"x86_64-{rng.choice(['unknown', 'pc', 'apple'])}-{rng.choice(['linux-gnu', 'windows-msvc', 'darwin'])}",
                ]
            )
        else:
            w = rng.choice(vocab)
            t = rng.choice([w, w.capitalize(), w.upper(), w + "s", w + str(rng.randint(1, 9))])
        if len(t) >= 4:
            out.append(t)
    return out


# ----------------------------------------------------------------------------------------
# Trigram table and model
# ----------------------------------------------------------------------------------------


def trigram_table(words: list[str]) -> np.ndarray:
    counts = np.zeros(27 * 27 * 27, dtype=np.float64)
    for w in words:
        a, b = 26, 26
        for c in [tri_index(ch) for ch in w] + [26]:
            counts[(a * 27 + b) * 27 + c] += 1.0
            a, b = b, c
    ctx = counts.reshape(27 * 27, 27)
    k = 0.1
    probs = (ctx + k) / (ctx.sum(axis=1, keepdims=True) + 27 * k)
    q = np.clip(np.round(np.log2(probs) * TRI_SCALE), -127, 0).astype(np.int8)
    return q.reshape(-1)


def fit_logistic(X: np.ndarray, y: np.ndarray, l2: float) -> np.ndarray:
    """Newton / IRLS with an L2 penalty (not on the bias). X has a leading ones column."""
    w = np.zeros(X.shape[1])
    reg = np.full(X.shape[1], l2)
    reg[0] = 0.0
    for _ in range(50):
        z = X @ w
        p = 1.0 / (1.0 + np.exp(-z))
        g = X.T @ (p - y) + reg * w
        s = p * (1.0 - p)
        H = (X * s[:, None]).T @ X + np.diag(reg)
        step = np.linalg.solve(H, g)
        w -= step
        if np.max(np.abs(step)) < 1e-10:
            break
    return w


def auc(scores: np.ndarray, y: np.ndarray) -> float:
    order = np.argsort(scores, kind="mergesort")
    s = scores[order]
    ranks = np.empty(len(s))
    i = 0
    while i < len(s):
        j = i
        while j + 1 < len(s) and s[j + 1] == s[i]:
            j += 1
        ranks[i : j + 1] = (i + j) / 2.0 + 1.0
        i = j + 1
    r = np.empty(len(s))
    r[order] = ranks
    pos = y == 1
    n_pos, n_neg = pos.sum(), (~pos).sum()
    return float((r[pos].sum() - n_pos * (n_pos + 1) / 2.0) / (n_pos * n_neg))


PARITY_TOKENS = [
    "getUserAccountSettings",
    "node_modules",
    "2024-11-30",
    "v1.2.3",
    "Hunter2-Fake-9x",
    "ticketBlob",
    "Hello",
    "password_input",
    "correct-horse",
    "Xy7-Qa9_Lm2",
    "550e8400-e29b-41d4-a716-446655440000",
    "/usr/lib/python3/json",
    "qwerty1234",
    "aaaaaaaa",
    "x",
    "Grüße",
]


def rust_f64(x: float) -> str:
    s = repr(float(x))
    if "e" not in s and "." not in s and "inf" not in s and "nan" not in s:
        s += ".0"
    return s


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--seed", type=int, default=20261106)
    ap.add_argument("--out", type=Path, default=DEFAULT_OUT)
    ap.add_argument("--per-class", type=int, default=30000)
    args = ap.parse_args()

    rng = random.Random(args.seed)
    idents, comment_words, dict_words = harvest(rng)
    idents = [t for t in idents if not looks_random(t)]
    print(
        f"sources: {len(idents)} identifiers, {len(comment_words)} comment words, "
        f"{len(dict_words)} dictionary words",
        file=sys.stderr,
    )
    vocab_d: dict[str, None] = {}
    for t in idents + comment_words + dict_words:
        for p in split_ident(t):
            if len(p) >= 2 and p.isalpha():
                vocab_d[p] = None
    vocab = sorted(vocab_d)
    english = sorted(set(w.lower() for w in comment_words + dict_words if len(w) >= 4))
    if not vocab or not english:
        print("no source text found", file=sys.stderr)
        return 1

    # Negatives: harvested tokens and generated shapes, deduplicated, shuffled.
    n = args.per_class
    harvested = idents + comment_words + dict_words
    rng.shuffle(harvested)
    neg = harvested[: n // 2] + generated_negatives(rng, n - min(len(harvested), n // 2), vocab)
    pos = positives(rng, n, english)
    neg = [t for t in neg if len(t) >= 4]

    # Split before building the trigram table: hold-out tokens never feed it.
    def split(xs: list[str]) -> tuple[list[str], list[str]]:
        cut = int(len(xs) * 0.8)
        return xs[:cut], xs[cut:]

    rng.shuffle(neg)
    rng.shuffle(pos)
    neg_tr, neg_te = split(neg)
    pos_tr, pos_te = split(pos)

    # The trigram model is English and code: training-split identifiers and words.
    tri_words = []
    for t in neg_tr:
        if any(c.isalpha() for c in t):
            tri_words.append(t)
    held_out = set(neg_te)
    tri_words += [w for w in vocab if w not in held_out]
    tri = trigram_table(tri_words)

    def matrix(xs: list[str]) -> np.ndarray:
        return np.array([features(t, tri) for t in xs], dtype=np.float64)

    Xtr = np.vstack([matrix(pos_tr), matrix(neg_tr)])
    ytr = np.concatenate([np.ones(len(pos_tr)), np.zeros(len(neg_tr))])
    Xte = np.vstack([matrix(pos_te), matrix(neg_te)])
    yte = np.concatenate([np.ones(len(pos_te)), np.zeros(len(neg_te))])

    mean = Xtr.mean(axis=0)
    std = Xtr.std(axis=0)
    std[std == 0] = 1.0
    Ztr = np.hstack([np.ones((len(Xtr), 1)), (Xtr - mean) / std])
    Zte = np.hstack([np.ones((len(Xte), 1)), (Xte - mean) / std])
    w = fit_logistic(Ztr, ytr, l2=1.0)

    score_te = Zte @ w
    score_tr = Ztr @ w
    a_te = auc(score_te, yte)
    a_tr = auc(score_tr, ytr)

    # Fold the scaling into the weights: z = bias + sum(weight_i * x_i).
    weights = w[1:] / std
    bias = w[0] - float(np.sum(w[1:] * mean / std))

    def prob(t: str) -> float:
        z = bias + float(np.dot(weights, np.array(features(t, tri))))
        return 1.0 / (1.0 + math.exp(-z))

    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / "trigram.bin").write_bytes(tri.astype(np.int8).tobytes())

    lines = [
        "//! Generated by `tools/classifier/train.py`; do not edit. Regenerate with",
        f"//! `python3 tools/classifier/train.py --seed {args.seed}`.",
        "//!",
        f"//! Hold-out AUC {a_te:.4f} (train {a_tr:.4f}); {len(pos)} positive and {len(neg)} negative tokens.",
        "",
        f"/// Number of features (see `features.rs`).",
        f"pub(crate) const N_FEATURES: usize = {len(FEATURE_NAMES)};",
        "",
        "/// Scale of the int8 log2-probabilities in `trigram.bin`.",
        f"pub(crate) const TRI_SCALE: f64 = {rust_f64(TRI_SCALE)};",
        "",
        "/// Average trigram log2-probability of a token without letters.",
        f"pub(crate) const TRI_NONE: f64 = {rust_f64(TRI_NONE)};",
        "",
        "/// Logistic-regression bias (feature scaling folded in).",
        f"pub(crate) const BIAS: f64 = {rust_f64(bias)};",
        "",
        "/// Weights, one per feature, in `features.rs` order.",
        "pub(crate) const WEIGHTS: [f64; N_FEATURES] = [",
    ]
    for name, x in zip(FEATURE_NAMES, weights):
        lines.append(f"    {rust_f64(x)}, // {name}")
    lines += [
        "];",
        "",
        "/// `(token, p)` pairs computed by the training script, checked by a unit test so the",
        "/// Rust features stay identical to the Python ones.",
        "#[cfg(test)]",
        "pub(crate) const PARITY: &[(&str, f64)] = &[",
    ]
    for t in PARITY_TOKENS:
        lines.append(f"    ({rust_string(t)}, {rust_f64(prob(t))}),")
    lines += ["];", ""]
    (args.out / "model.rs").write_text("\n".join(lines), encoding="utf-8")

    print(f"hold-out AUC: {a_te:.4f} (train {a_tr:.4f})")
    print(f"trigram.bin: {len(tri)} bytes; model.rs: {len(weights)} weights")
    for t in ["getUserAccountSettings", "node_modules", "2024-11-30", "v1.2.3", "ticketBlob", "Hello"]:
        print(f"  p({t}) = {prob(t):.4f}")
    if a_te < 0.98:
        print("hold-out AUC below the 0.98 target", file=sys.stderr)
        return 2
    return 0


def rust_string(s: str) -> str:
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


if __name__ == "__main__":
    sys.exit(main())
