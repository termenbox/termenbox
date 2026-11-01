use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::io::Write;
use std::time::Duration;

use super::*;

// A per-thread counting allocator: `ALLOCS` is how many allocations the thread made, `PEAK` the most heap it held at
// once. Per thread, so tests running side by side do not disturb each other's numbers.
thread_local! {
    static ALLOCS: Cell<u64> = const { Cell::new(0) };
    static LIVE: Cell<i64> = const { Cell::new(0) };
    static PEAK: Cell<i64> = const { Cell::new(0) };
}

struct Counting;

fn note(delta: i64, allocation: bool) {
    let _ = LIVE.try_with(|live| {
        live.set(live.get() + delta);
        let _ = PEAK.try_with(|peak| peak.set(peak.get().max(live.get())));
    });
    if allocation {
        let _ = ALLOCS.try_with(|n| n.set(n.get() + 1));
    }
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note(layout.size() as i64, true);
        System.alloc(layout)
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note(layout.size() as i64, true);
        System.alloc_zeroed(layout)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        note(-(layout.size() as i64), false);
        System.dealloc(ptr, layout)
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note(new_size as i64 - layout.size() as i64, true);
        System.realloc(ptr, layout, new_size)
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

pub(crate) fn allocs() -> u64 {
    ALLOCS.with(Cell::get)
}

/// Starts measuring the heap peak from what is held now.
fn reset_peak() -> i64 {
    let live = LIVE.with(Cell::get);
    PEAK.with(|p| p.set(live));
    live
}

fn peak_above(base: i64) -> i64 {
    PEAK.with(Cell::get) - base
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("termenbox-scan-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    fn file(&self, name: &str, data: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, data).unwrap();
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn unlimited() -> Limits {
    Limits {
        max_hits: usize::MAX,
        per_session: usize::MAX,
        max_time: Duration::from_secs(600),
        max_bytes: u64::MAX,
        since_ms: None,
    }
}

type Found = Vec<(u64, Vec<u8>, bool)>;

fn scan(path: &Path, query: &str, limits: Limits) -> (Found, End, Budget) {
    let needle = Needle::new(query).expect("query folds to something");
    let file = File::open(path).unwrap();
    let size = file.metadata().unwrap().len();
    let mut budget = Budget::new(limits, Cancel::default());
    let mut found = Found::new();
    let end = Scanner::new()
        .scan_file(&file, size, &needle, &mut budget, |c| {
            found.push((c.offset, c.bytes.to_vec(), c.complete));
            ControlFlow::Continue(())
        })
        .unwrap();
    (found, end, budget)
}

/// The same answer by the obvious route: split, filter, reverse. For ASCII data and an ASCII query.
fn naive(data: &[u8], query: &str) -> Found {
    let q = query.to_ascii_lowercase();
    let mut out = Found::new();
    let mut offset = 0usize;
    for segment in data.split_inclusive(|&b| b == b'\n') {
        let complete = segment.ends_with(b"\n");
        let line = if complete {
            &segment[..segment.len() - 1]
        } else {
            segment
        };
        let hit = !line.is_empty()
            && line.len() <= MAX_LINE
            && String::from_utf8_lossy(line)
                .to_ascii_lowercase()
                .contains(&q);
        if hit {
            out.push((offset as u64, line.to_vec(), complete));
        }
        offset += segment.len();
    }
    out.reverse();
    out
}

fn folds(line: &str, query: &str) -> bool {
    Needle::new(query)
        .unwrap()
        .matches(line.as_bytes(), &mut Vec::new())
}

#[test]
fn a_query_ignores_case_and_accents() {
    assert!(folds("un café noir", "CAFE"));
    assert!(folds("UN CAFÉ NOIR", "café"));
    assert!(folds("un cafe noir", "CAFÉ"));
    assert!(folds("naïve façade", "naive facade"));
    assert!(folds("ПРИВЕТ, мир", "привет"));
    assert!(folds("decomposed cafe\u{301} here", "café"));
    assert!(!folds("un thé noir", "café"));
}

#[test]
fn a_line_that_is_not_utf8_neither_panics_nor_hides_a_match() {
    let line = b"\xff\xfe broken then Needle here \xc3";
    let n = Needle::new("needle").unwrap();
    assert!(n.matches(line, &mut Vec::new()));
    assert!(!n.matches(b"\xff\xfe \xc3", &mut Vec::new()));
}

#[test]
fn a_query_with_nothing_to_look_for_is_refused() {
    assert!(Needle::new("").is_none());
    assert!(Needle::new("\u{301}").is_none());
}

#[test]
fn candidates_come_newest_first_with_their_offsets() {
    let dir = TempDir::new();
    let data = b"alpha needle\nbeta\n\ngamma NEEDLE tail\ndelta\nneedle last\n";
    let path = dir.file("a.jsonl", data);
    let (found, end, _) = scan(&path, "needle", unlimited());
    assert_eq!(end, End::Done);
    assert_eq!(found, naive(data, "needle"));
    assert_eq!(found.len(), 3);
    assert_eq!(found[0].0, data.len() as u64 - 12);
    assert!(found.iter().all(|f| f.2));
}

#[test]
fn a_torn_last_line_is_reported_incomplete() {
    let dir = TempDir::new();
    let data = b"first needle\nsecond needle, still being wri";
    let path = dir.file("a.jsonl", data);
    let (found, _, _) = scan(&path, "needle", unlimited());
    assert_eq!(found, naive(data, "needle"));
    assert!(!found[0].2, "the last line has no newline");
    assert!(found[1].2);
}

#[test]
fn odd_files_scan_without_a_panic() {
    let dir = TempDir::new();
    for (name, data) in [
        ("empty", &b""[..]),
        ("newlines", b"\n\n\n"),
        ("one-line-no-newline", b"needle"),
        ("one-line", b"needle\n"),
        ("miss", b"nothing here\nnor here\n"),
        ("crlf", b"needle\r\nneedle\r\n"),
    ] {
        let path = dir.file(name, data);
        let (found, end, _) = scan(&path, "needle", unlimited());
        assert_eq!(end, End::Done, "{name}");
        assert_eq!(found, naive(data, "needle"), "{name}");
    }
}

/// Deterministic pseudo-random lines of 1..max_len bytes, about one in `every` holding the needle.
fn generated(seed: u64, total: usize, max_len: usize, every: u64) -> Vec<u8> {
    let mut state = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
    let mut next = move || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        state >> 33
    };
    let mut data = Vec::with_capacity(total + max_len);
    while data.len() < total {
        let len = 1 + (next() as usize % max_len);
        let mut line = vec![b'x'; len];
        for b in line.iter_mut() {
            *b = b'a' + (next() % 26) as u8;
        }
        if len >= 8 && next() % every == 0 {
            let at = next() as usize % (len - 6);
            line[at..at + 6].copy_from_slice(b"NeEdLe");
        }
        data.extend_from_slice(&line);
        data.push(b'\n');
    }
    data
}

#[test]
fn lines_that_straddle_block_boundaries_are_found_once() {
    let dir = TempDir::new();
    for (seed, max_len, trim) in [(1, 3000, 0), (2, 70_000, 0), (3, 3000, 5), (4, 400_000, 0)] {
        let mut data = generated(seed, 3 * BLOCK + 17, max_len, 5);
        data.truncate(data.len() - trim); // a torn last line
        let path = dir.file("g.jsonl", &data);
        let (found, end, _) = scan(&path, "needle", unlimited());
        assert_eq!(end, End::Done);
        let want = naive(&data, "needle");
        assert!(!want.is_empty());
        assert_eq!(found.len(), want.len(), "seed {seed}");
        assert!(found == want, "seed {seed}: offsets, bytes or order differ");
    }
}

#[test]
fn a_line_longer_than_a_block_is_searched_and_one_past_the_cap_is_skipped() {
    let dir = TempDir::new();
    let mut data = Vec::new();
    data.extend_from_slice(b"head needle\n");
    data.extend_from_slice(&vec![b'a'; MAX_LINE + 5]);
    data.extend_from_slice(b" needle\n"); // too long: skipped
    data.extend_from_slice(b"middle needle\n");
    data.extend_from_slice(&vec![b'b'; 2 * BLOCK + 1]);
    data.extend_from_slice(b" needle\n"); // longer than a block, within the cap
    data.extend_from_slice(b"tail needle\n");
    let path = dir.file("long.jsonl", &data);
    let (found, end, _) = scan(&path, "needle", unlimited());
    assert_eq!(end, End::Done);
    assert_eq!(found, naive(&data, "needle"));
    assert_eq!(found.len(), 4);
    assert_eq!(found[3].1, b"head needle");
}

#[test]
fn a_file_that_is_one_long_line_is_one_candidate() {
    let dir = TempDir::new();
    let mut data = vec![b'q'; 3 * BLOCK + 3];
    data.extend_from_slice(b"needle");
    let path = dir.file("one.jsonl", &data);
    let (found, _, _) = scan(&path, "needle", unlimited());
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].0, 0);
    assert!(!found[0].2);
}

#[test]
fn the_adapter_can_stop_the_scan() {
    let dir = TempDir::new();
    let path = dir.file("a.jsonl", b"needle 1\nneedle 2\nneedle 3\nneedle 4\n");
    let needle = Needle::new("needle").unwrap();
    let file = File::open(&path).unwrap();
    let size = file.metadata().unwrap().len();
    let mut budget = Budget::new(unlimited(), Cancel::default());
    let mut seen = 0;
    let end = Scanner::new()
        .scan_file(&file, size, &needle, &mut budget, |_| {
            seen += 1;
            if seen == 3 {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        })
        .unwrap();
    assert_eq!(end, End::Stopped);
    assert_eq!(seen, 3);
}

#[test]
fn the_byte_cap_cuts_the_search_and_keeps_the_hits_so_far() {
    let dir = TempDir::new();
    let data = generated(7, 8 * BLOCK, 3000, 3);
    let path = dir.file("a.jsonl", &data);
    let limits = Limits {
        max_bytes: 3 * BLOCK as u64,
        ..unlimited()
    };
    let (found, end, budget) = scan(&path, "needle", limits);
    assert_eq!(end, End::Cut(Cut::Bytes));
    assert_eq!(budget.cut(), Some(Cut::Bytes));
    assert!(budget.bytes_read() <= 3 * BLOCK as u64);
    assert!(!found.is_empty());
    let all = naive(&data, "needle");
    assert!(
        found.iter().all(|f| all.contains(f)),
        "partial hits are real"
    );
    assert_eq!(found[0], all[0], "the newest hit comes first");
}

#[test]
fn the_time_cap_returns_the_partial_hits_with_cut_set() {
    let dir = TempDir::new();
    let data = generated(8, 6 * BLOCK, 2000, 2);
    let path = dir.file("a.jsonl", &data);
    let needle = Needle::new("needle").unwrap();
    let file = File::open(&path).unwrap();
    let size = file.metadata().unwrap().len();
    let limits = Limits {
        max_time: Duration::from_millis(40),
        ..unlimited()
    };
    let mut budget = Budget::new(limits, Cancel::default());
    let mut hits = 0;
    let end = Scanner::new()
        .scan_file(&file, size, &needle, &mut budget, |_| {
            hits += 1;
            std::thread::sleep(Duration::from_millis(60)); // a slow adapter
            ControlFlow::Continue(())
        })
        .unwrap();
    assert_eq!(end, End::Cut(Cut::Time));
    assert_eq!(budget.cut(), Some(Cut::Time));
    assert!(hits >= 1, "the partial hits are kept");
}

#[test]
fn a_cancel_stops_the_scan_between_reads() {
    let dir = TempDir::new();
    let path = dir.file("a.jsonl", &generated(9, 6 * BLOCK, 2000, 2));
    let needle = Needle::new("needle").unwrap();
    let file = File::open(&path).unwrap();
    let size = file.metadata().unwrap().len();
    let cancel = Cancel::default();
    let mut budget = Budget::new(unlimited(), cancel.clone());
    let mut hits = 0;
    let end = Scanner::new()
        .scan_file(&file, size, &needle, &mut budget, |_| {
            hits += 1;
            cancel.cancel();
            ControlFlow::Continue(())
        })
        .unwrap();
    assert_eq!(end, End::Cut(Cut::Cancelled));
    assert!(hits >= 1);
    assert!(budget.bytes_read() < size);
}

#[test]
fn the_hit_cap_fires_on_the_last_hit_allowed() {
    let mut budget = Budget::new(
        Limits {
            max_hits: 2,
            ..Limits::default()
        },
        Cancel::default(),
    );
    assert_eq!(budget.record_hit(), Ok(()));
    assert_eq!(budget.record_hit(), Err(Cut::Hits));
    assert_eq!(budget.cut(), Some(Cut::Hits));
    assert_eq!(budget.check(), Err(Cut::Hits), "a cut search stays cut");
}

#[test]
fn the_defaults_are_the_agreed_caps() {
    let l = Limits::default();
    assert_eq!(l.max_hits, 50);
    assert_eq!(l.per_session, 3);
    assert_eq!(l.max_time, Duration::from_secs(2));
    assert_eq!(l.max_bytes, 200 * 1024 * 1024);
}

#[test]
fn files_are_visited_newest_first() {
    let dir = TempDir::new();
    let now = SystemTime::now();
    let mut paths = Vec::new();
    for (name, age) in [("old", 300), ("new", 10), ("mid", 100)] {
        let p = dir.file(name, b"x\n");
        File::options()
            .write(true)
            .open(&p)
            .unwrap()
            .set_modified(now - Duration::from_secs(age))
            .unwrap();
        paths.push(p);
    }
    let names: Vec<_> = newest_first(paths)
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["new", "mid", "old"]);
}

#[test]
fn the_line_index_grows_with_the_file_and_resets_when_it_is_rewritten() {
    let dir = TempDir::new();
    let data = generated(11, 4 * INDEX_STRIDE as usize, 500, 9);
    let path = dir.file("a.jsonl", &data);
    let file = File::open(&path).unwrap();
    let mut scanner = Scanner::new();
    let mut index = LineIndex::default();
    index.refresh(&file, &mut scanner).unwrap();
    assert_eq!(index.end(), data.len() as u64, "the file ends in a newline");
    assert!(index.marks().len() >= 4);
    assert_eq!(index.marks()[0], 0);
    for &m in index.marks() {
        assert!(
            m == 0 || data[m as usize - 1] == b'\n',
            "a mark is a line start"
        );
    }
    assert_eq!(index.seek(0), 0);
    let target = data.len() as u64 / 2;
    let sought = index.seek(target);
    assert!(sought <= target && target - sought < INDEX_STRIDE + 500);

    // An append, with a torn line at the end, extends the index and leaves the torn line out.
    let before = index.marks().to_vec();
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    f.write_all(&generated(12, 2 * INDEX_STRIDE as usize, 500, 9))
        .unwrap();
    f.write_all(b"torn").unwrap();
    drop(f);
    let file = File::open(&path).unwrap();
    index.refresh(&file, &mut scanner).unwrap();
    assert!(index.end() > data.len() as u64);
    assert_eq!(index.end(), file.metadata().unwrap().len() - 4);
    assert_eq!(&index.marks()[..before.len()], &before[..]);

    // A new file under the same path is a different inode: start over.
    std::fs::remove_file(&path).unwrap();
    dir.file("a.jsonl", b"short\nfile\n");
    let file = File::open(&path).unwrap();
    index.refresh(&file, &mut scanner).unwrap();
    assert_eq!(index.end(), 11);
    assert_eq!(index.marks(), &[0]);
}

#[test]
fn the_index_cache_keeps_the_most_recent_files() {
    let cache = IndexCache::new(2);
    assert!(cache.is_empty());
    let a = cache.get(Path::new("/a"));
    let _b = cache.get(Path::new("/b"));
    assert!(
        Arc::ptr_eq(&a, &cache.get(Path::new("/a"))),
        "a hit is shared"
    );
    let _c = cache.get(Path::new("/c")); // /b is the least recent now
    assert_eq!(cache.len(), 2);
    assert!(Arc::ptr_eq(&a, &cache.get(Path::new("/a"))));
    let b2 = cache.get(Path::new("/b"));
    assert!(!Arc::ptr_eq(&a, &b2));
}

#[tokio::test]
async fn a_blocking_scan_runs_off_the_runtime_thread() {
    let here = std::thread::current().id();
    let there = blocking(|| std::thread::current().id()).await.unwrap();
    assert_ne!(here, there);
    assert!(blocking(|| -> u8 { panic!("a scan bug") }).await.is_none());
}

/// A file of lines, some with accents and Cyrillic, none holding "zebra quartz" except those the test adds.
fn write_bulk(path: &Path, bytes: usize, tail: &[&str]) {
    let mut out = std::io::BufWriter::with_capacity(1 << 20, File::create(path).unwrap());
    let mut written = 0usize;
    let mut n = 0u64;
    let mut line = String::new();
    while written < bytes {
        line.clear();
        n += 1;
        let body = if n.is_multiple_of(10) {
            "Привет, мир: создаём файл с кириллицей и акцентами — café, naïve, façade, ünïcödé "
        } else {
            "The assistant read the file, ran the build, and reported that every check passed on the first try "
        };
        use std::fmt::Write as _;
        let _ = write!(
            line,
            r#"{{"type":"assistant","uuid":"{n:016x}","message":{{"content":[{{"type":"text","text":"{body}{body}{body}"}}]}}}}"#
        );
        line.push('\n');
        out.write_all(line.as_bytes()).unwrap();
        written += line.len();
    }
    for t in tail {
        writeln!(out, r#"{{"type":"user","message":{{"content":"{t}"}}}}"#).unwrap();
    }
    out.flush().unwrap();
}

fn vm_rss_kib() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .find_map(|l| l.strip_prefix("VmRSS:"))
        .and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok())
        .unwrap()
}

#[test]
fn a_non_matching_line_costs_no_allocation() {
    let dir = TempDir::new();
    let path = dir.0.join("bulk.jsonl");
    write_bulk(&path, 2 * 1024 * 1024, &[]);
    let needle = Needle::new("zebra quartz").unwrap();
    let file = File::open(&path).unwrap();
    let size = file.metadata().unwrap().len();
    let mut scanner = Scanner::new();
    let cancel = Cancel::default(); // an Arc: made before the count starts
    let run = |scanner: &mut Scanner| {
        let mut budget = Budget::new(unlimited(), cancel.clone());
        let mut candidates = 0;
        let end = scanner
            .scan_file(&file, size, &needle, &mut budget, |_| {
                candidates += 1;
                ControlFlow::Continue(())
            })
            .unwrap();
        assert_eq!((end, candidates), (End::Done, 0));
    };
    run(&mut scanner); // the buffers reach their working size
    let before = allocs();
    run(&mut scanner);
    assert_eq!(allocs() - before, 0, "ASCII and non-ASCII lines, no match");
}

/// The 200 MB benchmark of decision 0157: a full scan stays within a time budget and a heap and RSS budget, and the
/// default caps cut a scan that would run longer. The budgets are for a debug build on a slow, shared CI box.
#[test]
fn bench_200_mb_scan_stays_within_budget() {
    const FILE_BYTES: usize = 200 * 1024 * 1024;
    let dir = TempDir::new();
    let path = dir.0.join("bulk.jsonl");
    write_bulk(
        &path,
        FILE_BYTES,
        &["a Zebra Quartz at last", "nothing", "second ZEBRA QUARTZ"],
    );
    let size = std::fs::metadata(&path).unwrap().len();
    assert!(size >= FILE_BYTES as u64);

    // The whole file, no cap in the way.
    let needle = Needle::new("zebra quartz").unwrap();
    let file = File::open(&path).unwrap();
    let mut scanner = Scanner::new();
    let rss_before = vm_rss_kib();
    let heap_base = reset_peak();
    let started = Instant::now();
    let mut budget = Budget::new(unlimited(), Cancel::default());
    let mut offsets = Vec::new();
    let end = scanner
        .scan_file(&file, size, &needle, &mut budget, |c| {
            offsets.push(c.offset);
            ControlFlow::Continue(())
        })
        .unwrap();
    let full = started.elapsed();
    let heap = peak_above(heap_base);
    let rss = vm_rss_kib().saturating_sub(rss_before);
    eprintln!("200 MB scan: {full:?}, heap peak {heap} B, RSS growth {rss} KiB");
    assert_eq!(end, End::Done);
    assert_eq!(budget.bytes_read(), size);
    assert_eq!(offsets.len(), 2, "both lines holding the query");
    assert!(offsets[0] > offsets[1], "newest first");
    assert!(heap < 4 * 1024 * 1024, "heap peak {heap} B");
    assert!(
        rss < 64 * 1024,
        "RSS grew {rss} KiB: the file must not be mapped or held"
    );
    assert!(full < Duration::from_secs(60), "full scan took {full:?}");

    // The default caps: a search never runs much past 2 s, and says so when it was cut.
    let started = Instant::now();
    let mut budget = Budget::new(Limits::default(), Cancel::default());
    let mut found = 0;
    let end = scanner
        .scan_file(&file, size, &needle, &mut budget, |_| {
            found += 1;
            ControlFlow::Continue(())
        })
        .unwrap();
    let capped = started.elapsed();
    assert!(
        capped < Duration::from_secs(5),
        "capped scan took {capped:?}"
    );
    assert!(budget.bytes_read() <= Limits::default().max_bytes);
    assert!(
        found >= 1,
        "the newest lines are read first, so their hits survive a cut"
    );
    if full > Duration::from_secs(2) {
        assert_eq!(end, End::Cut(Cut::Time));
    } else {
        assert_eq!(end, End::Cut(Cut::Bytes), "200 MB is the byte cap's edge");
    }
}

/// A corpus of one kind for the baseline: `ascii` lines only, `mixed` (every tenth line non-ASCII, as the benchmark
/// above) or `cyrillic` lines only.
fn write_corpus(path: &Path, bytes: usize, kind: &str) {
    let mut out = std::io::BufWriter::with_capacity(1 << 20, File::create(path).unwrap());
    let ascii = "The assistant read the file, ran the build, and reported that every check passed on the first try ";
    let cyr = "Привет, мир: создаём файл с кириллицей и акцентами — café, naïve, façade, ünïcödé ";
    let (mut written, mut n) = (0usize, 0u64);
    while written < bytes {
        n += 1;
        let body = match kind {
            "ascii" => ascii,
            "cyrillic" => cyr,
            _ if n.is_multiple_of(10) => cyr,
            _ => ascii,
        };
        let line = format!(
            r#"{{"type":"assistant","uuid":"{n:016x}","message":{{"content":[{{"type":"text","text":"{body}{body}{body}"}}]}}}}{}"#,
            "\n"
        );
        out.write_all(line.as_bytes()).unwrap();
        written += line.len();
    }
    out.flush().unwrap();
}

/// Drops the file's pages from the page cache so the next read is cold.
fn evict(file: &File) {
    use std::os::fd::AsRawFd;
    unsafe { libc::posix_fadvise(file.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED) };
}

/// The figures of `session_log/README.md`: run by hand in a release build, `-- --ignored --nocapture`.
#[test]
#[ignore = "a measurement, not a check"]
fn measure_the_scan_baseline() {
    const BYTES: usize = 200 * 1024 * 1024;
    let mb = |d: Duration| BYTES as f64 / 1048576.0 / d.as_secs_f64();
    let dir = TempDir::new();
    let needle = Needle::new("zebra quartz").unwrap();
    for kind in ["ascii", "mixed", "cyrillic"] {
        let path = dir.0.join(format!("{kind}.jsonl"));
        write_corpus(&path, BYTES, kind);
        let file = File::open(&path).unwrap();
        let size = file.metadata().unwrap().len();
        let mut scanner = Scanner::new();
        let scan = |scanner: &mut Scanner| {
            let mut budget = Budget::new(unlimited(), Cancel::default());
            let t = Instant::now();
            let end = scanner
                .scan_file(&file, size, &needle, &mut budget, |_| {
                    ControlFlow::Continue(())
                })
                .unwrap();
            assert_eq!(end, End::Done);
            t.elapsed()
        };
        evict(&file);
        let cold = scan(&mut scanner);
        let hot = (0..3).map(|_| scan(&mut scanner)).min().unwrap();

        // Phases over the same bytes, hot: pread alone, then the newline split, then the fold of every line, then
        // memmem over folded lines. The rest of `hot` is the loop and the carry copies.
        let mut buf = vec![0u8; BLOCK];
        let t = Instant::now();
        let mut at = 0u64;
        while at < size {
            let len = BLOCK.min((size - at) as usize);
            read_exact_at(&file, &mut buf[..len], at).unwrap();
            at += len as u64;
        }
        let pread = t.elapsed();
        let (mut split, mut fold_t, mut find_t) = (Duration::ZERO, Duration::ZERO, Duration::ZERO);
        let mut folded = Vec::new();
        let mut at = 0u64;
        while at < size {
            let len = BLOCK.min((size - at) as usize);
            read_exact_at(&file, &mut buf[..len], at).unwrap();
            at += len as u64;
            let block = &buf[..len];
            let t = Instant::now();
            let lines: Vec<&[u8]> = block.split(|&b| b == b'\n').collect();
            split += t.elapsed();
            // One timer per block: a timer per line costs more than the fold of an ASCII line.
            let fold_line = |l: &[u8], folded: &mut Vec<u8>| {
                folded.clear();
                if l.is_ascii() {
                    folded.extend(l.iter().map(u8::to_ascii_lowercase));
                } else {
                    for chunk in l.utf8_chunks() {
                        for c in chunk.valid().chars() {
                            fold_char(c, |f| push_char(folded, f));
                        }
                    }
                }
            };
            let t = Instant::now();
            for l in &lines {
                fold_line(l, &mut folded);
            }
            fold_t += t.elapsed();
            let t = Instant::now();
            for l in &lines {
                fold_line(l, &mut folded);
                std::hint::black_box(memchr::memmem::find(&folded, b"zebra quartz"));
            }
            find_t += t.elapsed();
        }
        let find_t = find_t.saturating_sub(fold_t);

        // One line's JSON parse, as the adapters pay it per candidate.
        let line = {
            let mut first = Vec::new();
            read_exact_at(&file, &mut buf[..4096], 0).unwrap();
            first.extend_from_slice(buf[..4096].split(|&b| b == b'\n').next().unwrap());
            first
        };
        let t = Instant::now();
        for _ in 0..20_000 {
            std::hint::black_box(serde_json::from_slice::<serde_json::Value>(&line).unwrap());
        }
        let parse_us = t.elapsed().as_secs_f64() * 1e6 / 20_000.0;

        // A fresh `Scanner` and one search of a small file: what a keystroke's search allocates.
        let small = dir.0.join("small.jsonl");
        write_corpus(&small, 1024 * 1024, kind);
        let sf = File::open(&small).unwrap();
        let ssize = sf.metadata().unwrap().len();
        let (rss0, a0, h0) = (vm_rss_kib(), allocs(), reset_peak());
        let mut s2 = Scanner::new();
        let mut budget = Budget::new(unlimited(), Cancel::default());
        s2.scan_file(&sf, ssize, &needle, &mut budget, |_| {
            ControlFlow::Continue(())
        })
        .unwrap();
        let (n_allocs, peak) = (allocs() - a0, peak_above(h0));
        drop(s2);
        eprintln!(
            "[{kind}] cold {:.0} MB/s ({cold:?}), hot {:.0} MB/s ({hot:?}); phases hot: pread {:.0} MB/s, newline split {:.1}% , fold {:.1}%, memmem {:.1}% of hot; parse {parse_us:.1} us/line; fresh search: {n_allocs} allocs, heap peak {peak} B, rss {} KiB -> {} KiB",
            mb(cold),
            mb(hot),
            mb(pread),
            100.0 * split.as_secs_f64() / hot.as_secs_f64(),
            100.0 * fold_t.as_secs_f64() / hot.as_secs_f64(),
            100.0 * find_t.as_secs_f64() / hot.as_secs_f64(),
            rss0,
            vm_rss_kib(),
        );
        std::fs::remove_file(&path).unwrap();
    }
}

#[test]
fn a_hundred_searches_allocate_one_set_of_blocks() {
    let dir = TempDir::new();
    let path = dir.0.join("bulk.jsonl");
    write_bulk(&path, 1024 * 1024, &[]);
    let needle = Needle::new("zebra quartz").unwrap();
    let file = File::open(&path).unwrap();
    let size = file.metadata().unwrap().len();
    // A pool of its own, so searches in other tests do not hand this one a warm scanner.
    let pool: &'static ScannerPool = Box::leak(Box::new(ScannerPool::new()));
    let cancel = Cancel::default();
    let search = || {
        let mut scanner = pool.take();
        let mut budget = Budget::new(unlimited(), cancel.clone());
        scanner
            .scan_file(&file, size, &needle, &mut budget, |_| {
                ControlFlow::Continue(())
            })
            .unwrap();
    };
    let (a0, h0) = (allocs(), reset_peak());
    search();
    let first = allocs() - a0;
    let a1 = allocs();
    for _ in 0..99 {
        search();
    }
    // The first search made the block and the working buffers; the other 99 reuse them (the pool lock and the
    // scanner's buffers are the only things touched, none of which allocate).
    assert_eq!(allocs() - a1, 0, "99 more searches, first made {first}");
    assert!(peak_above(h0) < 2 * BLOCK as i64, "one block, not many");
    // Process RSS is not asserted: tests share the process. No allocation means no growth to measure.
    assert_eq!(pool.idle(), 1);
}

#[test]
fn the_pool_keeps_two_idle_scanners_and_trims_big_buffers() {
    let pool: &'static ScannerPool = Box::leak(Box::new(ScannerPool::new()));
    let (a, b, c) = (pool.take(), pool.take(), pool.take());
    drop((a, b, c));
    assert_eq!(pool.idle(), POOL_MAX);
    let mut s = pool.take();
    s.line.resize(IDLE_CAP * 4, 0);
    drop(s);
    let s = pool.take();
    assert!(s.line.capacity() <= IDLE_CAP && s.line.is_empty());
    assert_eq!(s.buf.len(), BLOCK);
}

#[test]
fn a_cancelled_or_early_search_returns_its_scanner() {
    let dir = TempDir::new();
    let path = dir.0.join("bulk.jsonl");
    write_bulk(&path, 1024 * 1024, &[]);
    let file = File::open(&path).unwrap();
    let size = file.metadata().unwrap().len();
    let needle = Needle::new("zebra quartz").unwrap();
    let pool: &'static ScannerPool = Box::leak(Box::new(ScannerPool::new()));
    let cancel = Cancel::default();
    cancel.cancel();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut scanner = pool.take();
        let mut budget = Budget::new(unlimited(), cancel.clone());
        let end = scanner
            .scan_file(
                &file,
                size,
                &needle,
                &mut budget,
                |_| ControlFlow::Break(()),
            )
            .unwrap();
        assert!(matches!(end, End::Cut(_)));
        panic!("unwinds with the scanner held");
    }));
    assert!(result.is_err());
    assert_eq!(pool.idle(), 1);
}

/// The lines that hold `query`, newest first, found the slow way: every line folded and compared.
fn lines_with(text: &[u8], query: &str) -> Vec<(u64, Vec<u8>)> {
    let needle = Needle::new(query).unwrap();
    let mut scratch = Vec::new();
    let mut out = Vec::new();
    let mut offset = 0u64;
    for line in text.split_inclusive(|&b| b == b'\n') {
        let body = line.strip_suffix(b"\n").unwrap_or(line);
        if !body.is_empty() && needle.matches(body, &mut scratch) {
            out.push((offset, body.to_vec()));
        }
        offset += line.len() as u64;
    }
    out.reverse();
    out
}

fn scan_lines(path: &Path, query: &str) -> Vec<(u64, Vec<u8>)> {
    let file = File::open(path).unwrap();
    let size = file.metadata().unwrap().len();
    let needle = Needle::new(query).unwrap();
    let mut budget = Budget::new(unlimited(), Cancel::default());
    let mut found = Vec::new();
    let end = Scanner::new()
        .scan_file(&file, size, &needle, &mut budget, |c| {
            found.push((c.offset, c.bytes.to_vec()));
            ControlFlow::Continue(())
        })
        .unwrap();
    assert_eq!(end, End::Done);
    found
}

#[test]
fn the_block_search_finds_what_the_line_search_finds() {
    // Several blocks of lines, ASCII runs and accented runs, matches at the start, middle and end of blocks, a match
    // that is only a prefix of a word, the same line twice in a row, and a needle in more than one place per line.
    let mut text = Vec::new();
    let mut i = 0u32;
    while text.len() < 3 * BLOCK + 1000 {
        let line = match i % 11 {
            0 => format!("{{\"n\":{i},\"t\":\"Zebra Quartz here\"}}"),
            1 => format!("{{\"n\":{i},\"t\":\"café zebra QUARTZ\"}}"),
            2 => format!("{{\"n\":{i},\"t\":\"zebra quartzzebra quartz\"}}"),
            3 => format!("{{\"n\":{i},\"t\":\"zebra\"}}"),
            4 => String::new(),
            _ => format!(
                "{{\"n\":{i},\"t\":\"{}\"}}",
                "filler ".repeat(1 + (i % 40) as usize)
            ),
        };
        text.extend_from_slice(line.as_bytes());
        text.push(b'\n');
        i += 1;
    }
    text.extend_from_slice(b"zebra quartz without a newline");
    let dir = TempDir::new();
    let path = dir.0.join("mixed.jsonl");
    std::fs::write(&path, &text).unwrap();
    for query in [
        "zebra quartz",
        "ZEBRA",
        "quartz",
        "café",
        "cafe",
        "é",
        "filler",
        "a\nb",
        "nothing here",
    ] {
        assert_eq!(
            scan_lines(&path, query),
            lines_with(&text, query),
            "{query:?}"
        );
    }
}

#[test]
fn accents_do_not_matter_on_either_path() {
    // One ASCII block and one with an accent: "cafe" must find "CAFÉ" and "café" must find "cafe" in both.
    let dir = TempDir::new();
    let ascii = dir.0.join("ascii.jsonl");
    std::fs::write(&ascii, b"{\"t\":\"a cafe\"}\n{\"t\":\"CAFE\"}\nnone\n").unwrap();
    let accented = dir.0.join("accented.jsonl");
    std::fs::write(&accented, "{\"t\":\"un café\"}\n{\"t\":\"CAFÉ\"}\nnone\n").unwrap();
    for query in ["cafe", "café", "CAFÉ", "Cafe"] {
        assert_eq!(scan_lines(&ascii, query).len(), 2, "ASCII text, {query:?}");
        assert_eq!(
            scan_lines(&accented, query).len(),
            2,
            "accented text, {query:?}"
        );
    }
    // A needle that is not ASCII once folded cannot be in ASCII text.
    assert!(scan_lines(&ascii, "привет").is_empty());
}

#[test]
fn iso_times_parse_to_epoch_milliseconds() {
    assert_eq!(parse_iso_ms(b"1970-01-01T00:00:00.000Z"), Some(0));
    assert_eq!(parse_iso_ms(b"2000-03-01T00:00:00Z"), Some(951_868_800_000));
    assert_eq!(
        parse_iso_ms(b"2024-02-29T12:34:56.789Z\",x"),
        Some(1_709_210_096_789)
    );
    assert_eq!(parse_iso_ms(b"2025-13-01T00:00:00Z"), None);
    assert_eq!(parse_iso_ms(b"not a time at all.."), None);
    assert_eq!(parse_iso_ms(b"2025-01-01"), None);
}

/// `lines` records, each `pad` bytes of text, one a second from `start_s`; the last holds "needle".
fn timed_file(path: &Path, start_s: u64, lines: u64, pad: usize) {
    let mut out = std::io::BufWriter::new(File::create(path).unwrap());
    let pad = "x".repeat(pad);
    for n in 0..lines {
        let t = (start_s + n) * 1000;
        let (s, ms) = (t / 1000, t % 1000);
        let (days, rem) = (s / 86_400, s % 86_400);
        // 1970-01-01 plus `days`, which only needs to stay within January 1970 here.
        let day = 1 + days;
        let text = if n == lines - 1 { "needle" } else { "plain" };
        writeln!(
            out,
            r#"{{"type":"user","timestamp":"1970-01-{day:02}T{:02}:{:02}:{:02}.{ms:03}Z","message":"{text} {pad}"}}"#,
            rem / 3600,
            rem / 60 % 60,
            rem % 60
        )
        .unwrap();
    }
    out.flush().unwrap();
}

#[test]
fn a_file_stops_at_the_first_block_older_than_the_bound() {
    let dir = TempDir::new();
    let path = dir.0.join("timed.jsonl");
    // 4000 lines of ~1 KB, one a second: about 4 MB, 16 blocks. Needle only in the newest line.
    timed_file(&path, 0, 4000, 1000);
    let size = std::fs::metadata(&path).unwrap().len();
    let all = scan(&path, "needle", unlimited());
    assert_eq!((all.0.len(), all.1), (1, End::Done));
    assert_eq!(all.2.bytes_read(), size);
    assert_eq!(all.2.older(), 0);

    // Everything from second 3900 on is wanted: the needle is found, the older 3.8 MB is not read.
    let wanted = Limits {
        since_ms: Some(3900 * 1000),
        ..unlimited()
    };
    let (found, end, budget) = scan(&path, "needle", wanted);
    assert_eq!((found.len(), end), (1, End::Range));
    assert_eq!(budget.older(), 1);
    assert!(
        budget.bytes_read() <= 2 * BLOCK as u64,
        "read {} of {size}",
        budget.bytes_read()
    );

    // A bound older than the whole file reads all of it and reports nothing skipped.
    let all_new = Limits {
        since_ms: Some(0),
        ..unlimited()
    };
    let (found, end, budget) = scan(&path, "needle", all_new);
    assert_eq!((found.len(), end, budget.older()), (1, End::Done, 0));
}
