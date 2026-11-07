# Token classifier

`train.py` builds the token classifier of `redactor-core`: a logistic model that
estimates how likely a single token (no whitespace) is a randomly generated secret rather than
a word, an identifier, a date, a version or a path.

```
python3 tools/classifier/train.py            # default seed 20261106
python3 tools/classifier/train.py --seed 7 --out /some/dir
```

Requirements: Python 3.12 and numpy 2.2. The script prints the hold-out AUC and exits non-zero
when it is below 0.98.

## Output

Both files go to `crates/redactor-core/src/classifier/` and are committed:

- `model.rs`: the bias and one weight per feature (feature scaling is folded in), plus a few
  `(token, p)` parity pairs that a Rust unit test recomputes, so `features.rs` and the
  Python `features()` cannot drift apart.
- `trigram.bin`: 27³ = 19 683 bytes, the int8 table `round(8 · log2 p(c₃ | c₁ c₂))` over
  lowercase letters plus one boundary symbol (anything that is not a letter).

Only these statistics are written. No text from the sources leaves the script.

## Features

Length (log); fractions of lowercase, uppercase, digits and other characters; number of
classes present; Shannon entropy; entropy relative to the class alphabet; class transitions
per character; longest same-class run; vowel ratio; average and worst three-letter-window
trigram log-likelihood; repeated-bigram ratio; keyboard-walk flag; hex, base64, base58 and
UUID shape flags; three products (length × relative entropy, length × trigram average,
classes × transitions). Tokens are scored on their first 256 characters.

## Data

The split is 80 % training and 20 % hold-out. The trigram table is built from training
tokens only, so the hold-out AUC is measured on tokens the table never saw.

**Positives** are generated with the seeded `random.Random`: base62, base64 (with and
without padding), base64url, hex (lower and upper case), base58, generator-style passwords
with symbols, human-style passwords (a word plus digits and a symbol, sometimes a second
word), and vendor-shaped tokens (a known prefix plus a random body). They are generated in
memory on every run and never written to disk.

**Negatives** are about half harvested, half generated:

- Identifiers and comment words from public source trees on the build machine, pinned so
  that unrelated downloads do not change the model: the Python 3.12 standard library under
  `/usr/lib/python3.12` (installed packages excluded), the Rust crates `redactor-core` is
  built from, at their `Cargo.lock` versions, under `~/.cargo/registry/src` (the list is
  `RUST_CRATES` in `train.py`), and `/usr/share/dict/words` when it exists (it does not on
  the machine that produced the committed model). Toolchain sources under `~/.rustup` are
  not used: the `rust-src` component is not installed there, and installing it would
  silently change the inputs. String literals are removed before harvesting (they hold test
  data such as digests and keys), and identifiers that are themselves random-looking data
  (long digit runs, long runs with almost no vowels) are dropped. At most 64 KiB per file and
  48 MiB in total are read, in a seeded order.
- Generated shapes that should score low: composite identifiers (camelCase, PascalCase,
  snake_case, kebab-case, SCREAMING_CASE, dotted) built from harvested words; dates, times
  and timestamps in common formats; versions (`v1.2.3`, `1.2.3-rc.1`, `2.0.0+build.5`);
  Unix and Windows paths; UUIDs; CSS colours; IPv4 addresses, grouped numbers, ports,
  locale names and target triples.

**Decisions.**

- Random hex counts as a positive: many API keys are hex, and the classifier also scores the
  values of secret-named keys. Git SHAs and digests are therefore *not* negatives (they are
  indistinguishable from hex keys by shape). Whether a standalone hex run is a digest is
  decided by context in the engine: hex-only runs are never a Balanced finding, and in
  Paranoid a run after `commit`, `sha`, `hash`, `digest`, `checksum` or `integrity` is
  skipped.
- UUIDs are negatives (they are identifiers far more often than secrets). A UUID used as a
  key is still caught when its key is secret-named.
- Human-style passwords are about 12 % of the positives, so word-plus-digits values score in
  the middle rather than at either end.

## Determinism

The same seed and the same source trees give byte-identical `model.rs` and `trigram.bin`
(files are read in sorted order and shuffled with the seeded generator; no set or hash
ordering reaches the output). A machine with different source trees produces a slightly
different model.
