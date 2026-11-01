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

fn allocs() -> u64 {
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
