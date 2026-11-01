# Session log scan: baseline figures

Taken for step 2 of `docs/plans/session-search-followups.md`, at the commit that adds the measurement. Reproduce with:

```
scripts/dev.sh cargo test --release -p termenbox-server --bin termenbox-server measure_the_scan_baseline -- --ignored --nocapture
```

The test is `#[ignore]`d: it is a measurement, not a check. It writes three 200 MB corpora in a temp dir (`ascii`: every
line ASCII; `mixed`: every tenth line Cyrillic with accents, as the budget test; `cyrillic`: every line non-ASCII), scans
each end to end for a needle that is absent, and evicts the file's pages (`posix_fadvise DONTNEED`) for the cold run.

Machine: AMD EPYC (virtualised), 8 vCPU visible, 24 GB RAM, Linux 5.15, Docker `rust` image, release build, one thread.
Figures below are the second run (the first agreed within 20%, except cold ASCII: 187 and 147 MB/s, the disk is shared).

| Corpus   | Cold MB/s | Hot MB/s | Hot pread alone | Fold + `memmem` | Parse per line |
|----------|-----------|----------|-----------------|-----------------|----------------|
| ascii    | 147       | 601      | ~3900 MB/s      | fold 75%, find 21% | 2.0 µs      |
| mixed    | 112       | 206      | ~4300 MB/s      | fold 85%, find 7%  | 2.3 µs      |
| cyrillic | 30        | 36       | ~3800 MB/s      | fold 97%, find 2%  | 2.4 µs      |

- The phase shares are of isolated passes over the same bytes (one timer per 256 KiB block), compared with the full
  scan's time. They are not additive: the ASCII newline split alone costs about 30% in the isolated pass, which the real
  scan does with `memrchr` for less. Read them as "where the time goes", not as a budget.
- **The fold is the cost.** `pread` runs at 4 GB/s hot, so reading is never the limit once the page cache is warm; the
  copy-and-fold of every line is. A single non-ASCII line makes that line take the slow `chars()` + NFD path, and
  one in ten lines is enough to cut the ASCII figure to a third.
- **Cold reads cap at the disk** (30 to 150 MB/s here), so a cold search of a large history is cut by the 2 s cap
  long before it covers the 200 MB cap. Not reading what a time bound excludes (step 6) matters more there than the
  scan speed.
- **Memory**: RSS did not move across any run (8.2 MB before and after); the scan's heap peak is 258 KiB, the 256 KiB
  block plus small buffers.
- **A fresh search allocates 8 to 13 times** (`Scanner::new()` and the first lines), including the 256 KiB block that
  glibc `mmap`s and returns at once. That is what step 3 pools (done: `ScannerPool`, 0 allocations for the 2nd to 100th search); a warm scanner allocates nothing per non-matching line
  (the existing `a_non_matching_line_costs_no_allocation` test).
- **Parse** is about 2 µs for a ~340-byte line, so 10 000 candidates cost 20 ms: parse only matters when the needle
  is common or matches tool output (step 5).

## What each later step expects to move

- Step 3 (pool the buffers): allocations per fresh search from 8-13 to 0 after the first; heap peak and RSS unchanged.
- Step 4 (search blocks): hot MB/s on `ascii` from 601 towards the `pread` ceiling (several GB/s); `mixed` and
  `cyrillic` unchanged until the fold itself is reworked (blocks with any non-ASCII byte keep the fold path).
- Step 5 (reject before parse): parse count and allocations for a needle found in tool output, not MB/s.
- Step 6 (time bounds): bytes read for a search with a `since`, not MB/s.
