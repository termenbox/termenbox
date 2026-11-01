//! The bounded scan under every adapter's `search` (decision 0157, guardrail
//! `session-search-is-bounded-scrubbed-and-never-logs-the-query`).
//!
//! A session file is read backwards, newest lines first, with `pread` into one reusable buffer. Each line is folded
//! (case and accents) into a second reusable buffer and searched with `memchr::memmem`; only a line that holds the
//! needle is handed to the adapter, which parses it. A line that does not match is neither parsed nor allocated for.
//! Matched lines are the only place the scan allocates (the adapter copies what it keeps), and `mmap` is never used:
//! a file truncated while mapped raises SIGBUS.
//!
//! A block whose whole lines are all ASCII is searched as it is, case-insensitively, with no copy and no line split
//! (see `BlockPlan` for why accents need no case there); any other block takes the line path above.
//!
//! A search is capped by hits, bytes read and wall time, and can be cancelled. The caps are checked between reads, so
//! a search overshoots by at most one block. When one fires, the hits found so far stay and the cause is reported
//! (`Cut`), so the sheet can say the search was cut.
//!
//! The scan runs on a blocking thread (`blocking`), never on the control path. Nothing here logs: a needle, a line and
//! a path never reach an error text either (`io::Error` from the OS carries none).

use std::collections::VecDeque;
use std::fs::File;
use std::io;
use std::ops::ControlFlow;
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use aho_corasick::AhoCorasick;
use memchr::memmem;

use super::{fold_char, Fingerprint};

/// Bytes read at a time, and so the most a search overshoots a cap by.
pub const BLOCK: usize = 256 * 1024;
/// A line longer than this is skipped, not searched: the entry it holds is capped far below it anyway, and it bounds
/// the memory a single line can hold.
pub const MAX_LINE: usize = 8 * 1024 * 1024;
/// The distance between marks in a `LineIndex`.
pub const INDEX_STRIDE: u64 = 64 * 1024;
/// How many files' indexes are kept.
pub const INDEX_CACHE_FILES: usize = 8;

/// What bounds one search, across all the files it reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_hits: usize,
    pub per_session: usize,
    pub max_time: Duration,
    pub max_bytes: u64,
    /// Only records at or after this time (ms since the epoch) are wanted; none means any age.
    pub since_ms: Option<u64>,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_hits: 50,
            per_session: 3,
            max_time: Duration::from_secs(2),
            max_bytes: 200 * 1024 * 1024,
            since_ms: None,
        }
    }
}

/// Why a search stopped before it had looked everywhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cut {
    Hits,
    Bytes,
    Time,
    Cancelled,
}

/// A flag the connection sets to stop a search (a new query, a closed sheet, a closed connection).
#[derive(Clone, Debug, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// Whether `other` is a handle to the same flag.
    pub fn same(&self, other: &Cancel) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// The running totals of one search, shared by every file it scans.
#[derive(Debug)]
pub struct Budget {
    limits: Limits,
    started: Instant,
    bytes: u64,
    hits: usize,
    cancel: Cancel,
    cut: Option<Cut>,
    older: usize,
}

impl Budget {
    pub fn new(limits: Limits, cancel: Cancel) -> Self {
        Budget {
            limits,
            started: Instant::now(),
            bytes: 0,
            hits: 0,
            cancel,
            cut: None,
            older: 0,
        }
    }

    pub fn limits(&self) -> &Limits {
        &self.limits
    }

    pub fn bytes_read(&self) -> u64 {
        self.bytes
    }

    pub fn hits(&self) -> usize {
        self.hits
    }

    /// How many sessions the `since` bound kept from being read in full: skipped whole or stopped part way.
    pub fn older(&self) -> usize {
        self.older
    }

    pub fn note_older(&mut self) {
        self.older += 1;
    }

    /// The first cap that fired, if any.
    pub fn cut(&self) -> Option<Cut> {
        self.cut
    }

    fn stop(&mut self, why: Cut) -> Result<(), Cut> {
        self.cut.get_or_insert(why);
        Err(self.cut.unwrap_or(why))
    }

    /// Whether the search may go on: not cancelled, not out of time, not already cut.
    pub fn check(&mut self) -> Result<(), Cut> {
        if let Some(why) = self.cut {
            return Err(why);
        }
        if self.cancel.is_cancelled() {
            return self.stop(Cut::Cancelled);
        }
        if self.started.elapsed() >= self.limits.max_time {
            return self.stop(Cut::Time);
        }
        Ok(())
    }

    /// Counts `n` bytes about to be read; refuses the read that would pass the cap.
    pub fn charge(&mut self, n: u64) -> Result<(), Cut> {
        if self.bytes + n > self.limits.max_bytes {
            return self.stop(Cut::Bytes);
        }
        self.bytes += n;
        Ok(())
    }

    /// Counts a confirmed hit. `Err(Cut::Hits)` when it was the last one allowed: the adapter keeps this hit and
    /// stops.
    pub fn record_hit(&mut self) -> Result<(), Cut> {
        self.hits += 1;
        if self.hits >= self.limits.max_hits {
            return self.stop(Cut::Hits);
        }
        Ok(())
    }
}

/// The query, folded once: lower case, accents removed. Matches a line that holds it after the line is folded the
/// same way, so "CAFÉ", "café" and "cafe" are one query.
pub struct Needle {
    finder: memmem::Finder<'static>,
    plan: BlockPlan,
}

/// How a block that is entirely ASCII may be searched. Folding ASCII text only lowercases it, and the needle is
/// folded once here, so the rule needs no case for accents: "cafe" and "CAFÉ" are the same folded needle `cafe`, and
/// a block of ASCII holds it exactly when a case-insensitive search of the raw bytes does.
enum BlockPlan {
    /// Search the raw block, ignoring ASCII case, without copying any line.
    Ascii(AhoCorasick),
    /// The folded needle has a non-ASCII byte, which a block of ASCII cannot hold.
    Never,
    /// The needle holds a newline, which a search of whole lines never matches across: take the line path.
    Lines,
}

impl Needle {
    /// `None` when nothing is left to look for.
    pub fn new(query: &str) -> Option<Self> {
        let mut folded = Vec::with_capacity(query.len());
        for c in query.chars() {
            fold_char(c, |f| push_char(&mut folded, f));
        }
        if folded.is_empty() {
            return None;
        }
        let finder = memmem::Finder::new(&folded).into_owned();
        let plan = if folded.contains(&b'\n') {
            BlockPlan::Lines
        } else if !folded.is_ascii() {
            BlockPlan::Never
        } else {
            match AhoCorasick::builder()
                .ascii_case_insensitive(true)
                .build([&folded])
            {
                Ok(ac) => BlockPlan::Ascii(ac),
                Err(_) => BlockPlan::Lines,
            }
        };
        Some(Needle { finder, plan })
    }

    /// Pushes onto `spans` the `(start, end)` of each line in `region` that holds the needle, oldest first, where
    /// `region` is whole lines each ending in a newline, all ASCII. `None` when the needle needs the line path.
    fn ascii_lines(&self, region: &[u8], spans: &mut Vec<(usize, usize)>) -> Option<()> {
        let ac = match &self.plan {
            BlockPlan::Ascii(ac) => ac,
            BlockPlan::Never => return Some(()),
            BlockPlan::Lines => return None,
        };
        let mut from = 0;
        while let Some(m) = ac.find(&region[from..]) {
            let at = from + m.start();
            // `region` ends in a newline, so the line always ends inside it.
            let start = memchr::memrchr(b'\n', &region[..at]).map_or(0, |i| i + 1);
            let end = at + memchr::memchr(b'\n', &region[at..]).unwrap_or(region.len() - at);
            spans.push((start, end));
            from = end + 1;
        }
        Some(())
    }

    /// Whether `line` holds the needle. `scratch` receives the folded line; its capacity is reused.
    pub fn matches(&self, line: &[u8], scratch: &mut Vec<u8>) -> bool {
        scratch.clear();
        if line.is_ascii() {
            scratch.extend(line.iter().map(u8::to_ascii_lowercase));
        } else {
            for chunk in line.utf8_chunks() {
                for c in chunk.valid().chars() {
                    fold_char(c, |f| push_char(scratch, f));
                }
                scratch.extend_from_slice(chunk.invalid());
            }
        }
        self.finder.find(scratch).is_some()
    }
}

/// A first line longer than this is not parsed for its time, so it never ends a scan.
const MAX_STAMP_LINE: usize = 64 * 1024;

/// A record's own top-level `timestamp`; every other key, nested ones included, is skipped without being built.
#[derive(serde::Deserialize)]
struct Stamp<'a> {
    #[serde(borrow, default)]
    timestamp: Option<&'a str>,
}

/// The time (ms since the epoch) of the first whole line of `block`. Anything unreadable (torn, escaped, missing,
/// oversized) is `None`, which never stops a scan.
fn first_line_time(block: &[u8]) -> Option<u64> {
    let first = memchr::memchr(b'\n', block)? + 1;
    let rest = &block[first..];
    let line = &rest[..memchr::memchr(b'\n', rest)?];
    if line.len() > MAX_STAMP_LINE {
        return None;
    }
    let stamp: Stamp = serde_json::from_slice(line).ok()?;
    parse_iso_ms(stamp.timestamp?.as_bytes())
}

/// `2025-01-31T12:34:56.789Z` (a leading part of `s`) as ms since the epoch, in UTC.
pub fn parse_iso_ms(s: &[u8]) -> Option<u64> {
    let num = |from: usize, len: usize| -> Option<i64> {
        let d = s.get(from..from + len)?;
        d.iter()
            .all(u8::is_ascii_digit)
            .then(|| d.iter().fold(0i64, |n, b| n * 10 + i64::from(b - b'0')))
    };
    let (y, m, d) = (num(0, 4)?, num(5, 2)?, num(8, 2)?);
    let (hh, mm, ss) = (num(11, 2)?, num(14, 2)?, num(17, 2)?);
    if s[4] != b'-' || s[7] != b'-' || s[10] != b'T' || s[13] != b':' || s[16] != b':' {
        return None;
    }
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || hh > 23 || mm > 59 || ss > 60 {
        return None;
    }
    let mut ms = 0;
    if s.get(19) == Some(&b'.') {
        let mut scale = 100;
        for b in s[20..].iter().take_while(|b| b.is_ascii_digit()).take(3) {
            ms += i64::from(b - b'0') * scale;
            scale /= 10;
        }
    }
    // Days from the civil date (Howard Hinnant's algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(((days * 24 + hh) * 60 + mm) * 60_000 + ss * 1000 + ms).ok()
}

fn push_char(out: &mut Vec<u8>, c: char) {
    let mut b = [0u8; 4];
    out.extend_from_slice(c.encode_utf8(&mut b).as_bytes());
}

/// A line that holds the needle, for the adapter to parse and confirm. `bytes` is valid for the call only.
pub struct Candidate<'a> {
    /// Where the line starts in the file.
    pub offset: u64,
    pub bytes: &'a [u8],
    /// Whether a newline follows it. The last line of a file that is still being written may not be: it is torn
    /// unless it parses.
    pub complete: bool,
}

/// How a scan of one file ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    /// Every line was looked at.
    Done,
    /// The adapter had enough (`ControlFlow::Break`).
    Stopped,
    /// A cap fired; the budget says which.
    Cut(Cut),
    /// The rest of the file is older than `Limits::since_ms`.
    Range,
}

/// The buffers a scan reuses, kept across the files of one search.
pub struct Scanner {
    buf: Vec<u8>,
    carry: Vec<u8>,
    line: Vec<u8>,
    fold: Vec<u8>,
    spans: Vec<(usize, usize)>,
    /// Where the last `scan_file` left off when it did not finish: the file's bytes before it were not looked at.
    stopped: Option<u64>,
}

impl Default for Scanner {
    fn default() -> Self {
        Self::new()
    }
}

impl Scanner {
    pub fn new() -> Self {
        Scanner {
            buf: vec![0; BLOCK],
            carry: Vec::new(),
            line: Vec::new(),
            fold: Vec::new(),
            spans: Vec::new(),
            stopped: None,
        }
    }

    /// The offset a stopped or cut `scan_file` ended at, always just after a newline (or the file's size): scanning
    /// the file again as if it were that long continues where it left off. None when the last scan ran to its end.
    pub fn stopped_at(&self) -> Option<u64> {
        self.stopped
    }

    /// Empties the working buffers and drops the ones a huge line grew, so an idle scanner stays small.
    fn tidy(&mut self) {
        for v in [&mut self.carry, &mut self.line, &mut self.fold] {
            v.clear();
            if v.capacity() > IDLE_CAP {
                *v = Vec::new();
            }
        }
        self.spans.clear();
        if self.spans.capacity() * size_of::<(usize, usize)>() > IDLE_CAP {
            self.spans = Vec::new();
        }
    }

    /// Calls `on_candidate` for each line of `file` that holds `needle`, newest line first. Reads the file as it was
    /// `size` bytes long, so a file that grows during the scan is not chased.
    pub fn scan_file(
        &mut self,
        file: &File,
        size: u64,
        needle: &Needle,
        budget: &mut Budget,
        on_candidate: impl FnMut(Candidate<'_>) -> ControlFlow<()>,
    ) -> io::Result<End> {
        let last = std::cell::Cell::new(size);
        self.stopped = None;
        let end = self.scan_inner(file, size, needle, budget, on_candidate, &last)?;
        if matches!(end, End::Stopped | End::Cut(_)) {
            self.stopped = Some(last.get());
        }
        Ok(end)
    }

    fn scan_inner(
        &mut self,
        file: &File,
        size: u64,
        needle: &Needle,
        budget: &mut Budget,
        mut on_candidate: impl FnMut(Candidate<'_>) -> ControlFlow<()>,
        last: &std::cell::Cell<u64>,
    ) -> io::Result<End> {
        let Scanner {
            buf,
            carry,
            line,
            fold,
            spans,
            ..
        } = self;
        carry.clear();
        let mut skipping = false;
        // Whether the line in `carry` ends with a newline. Only the last line of the file can lack one.
        let mut carry_complete = true;
        let mut end = size;
        let mut first = true;
        let mut prev_older = false;

        // Hands one line to the adapter if it holds the needle.
        // `known` is true when the caller already found the needle in `bytes`, so the fold is skipped.
        let mut emit =
            |offset: u64, bytes: &[u8], complete: bool, known: bool| -> ControlFlow<()> {
                if bytes.is_empty()
                    || bytes.len() > MAX_LINE
                    || (!known && !needle.matches(bytes, fold))
                {
                    return ControlFlow::Continue(());
                }
                last.set(offset);
                on_candidate(Candidate {
                    offset,
                    bytes,
                    complete,
                })
            };

        while end > 0 {
            // The carry is the tail of a line that starts before `end`: the file up to its last byte is still to read.
            last.set(end + carry.len() as u64);
            if let Err(why) = budget.check() {
                return Ok(End::Cut(why));
            }
            let start = end.saturating_sub(BLOCK as u64);
            let len = (end - start) as usize;
            if let Err(why) = budget.charge(len as u64) {
                return Ok(End::Cut(why));
            }
            read_exact_at(file, &mut buf[..len], start)?;
            let block = &buf[..len];
            if first {
                carry_complete = block[len - 1] == b'\n';
                first = false;
            }
            // Records are in time order, so what lies before a line older than the bound is older too. One odd line
            // (a clock jump, a copied record) must not hide newer ones, so the scan ends only at the second block in
            // a row whose first whole line is older; the first of them is still searched.
            let older = start > 0
                && budget
                    .limits()
                    .since_ms
                    .is_some_and(|since| first_line_time(block).is_some_and(|t| t < since));
            if older && prev_older {
                budget.note_older();
                return Ok(End::Range);
            }
            prev_older = older;

            let mut newlines = memchr::memrchr_iter(b'\n', block);
            let Some(last_nl) = newlines.next() else {
                // No newline: the whole block is the middle of one line. Prepend it to the carry.
                if !skipping {
                    if carry.len() + len > MAX_LINE {
                        skipping = true;
                        carry.clear();
                    } else {
                        line.clear();
                        line.extend_from_slice(block);
                        line.extend_from_slice(carry);
                        // Copied back, not swapped: each buffer keeps its own capacity, so a steady scan stops growing them.
                        carry.clear();
                        carry.extend_from_slice(line);
                    }
                }
                end = start;
                continue;
            };

            // The line the block ends in: its start is here, the rest is the carry.
            if skipping {
                skipping = false;
            } else if len - (last_nl + 1) + carry.len() <= MAX_LINE {
                line.clear();
                line.extend_from_slice(&block[last_nl + 1..]);
                line.extend_from_slice(carry);
                if emit(start + last_nl as u64 + 1, line, carry_complete, false).is_break() {
                    return Ok(End::Stopped);
                }
            }
            carry.clear();
            carry_complete = true;

            // Whole lines inside the block, right to left.
            let first_nl = memchr::memchr(b'\n', block).unwrap_or(last_nl);
            let region = &block[first_nl + 1..=last_nl];
            spans.clear();
            let hi = if region.is_ascii() && needle.ascii_lines(region, spans).is_some() {
                // The whole lines are all ASCII: search the block itself and only look at the lines that match.
                for &(s, e) in spans.iter().rev() {
                    let at = first_nl + 1 + s;
                    if emit(start + at as u64, &block[at..first_nl + 1 + e], true, true).is_break()
                    {
                        return Ok(End::Stopped);
                    }
                }
                first_nl
            } else {
                spans.clear();
                let mut hi = last_nl;
                for nl in newlines {
                    if emit(start + nl as u64 + 1, &block[nl + 1..hi], true, false).is_break() {
                        return Ok(End::Stopped);
                    }
                    hi = nl;
                }
                hi
            };
            // What is left of the block, `block[..hi]`, ends a line that started earlier.
            if start == 0 {
                if emit(0, &block[..hi], true, false).is_break() {
                    return Ok(End::Stopped);
                }
            } else {
                carry.extend_from_slice(&block[..hi]);
            }
            end = start;
        }

        if end == 0 && !carry.is_empty() && !skipping {
            // The file starts in the middle of the blocks that had no newline.
            if emit(0, carry, carry_complete, false).is_break() {
                return Ok(End::Stopped);
            }
        }
        Ok(End::Done)
    }
}

/// Most scanners kept idle: two searches overlap at most briefly, and each holds a 256 KiB block.
const POOL_MAX: usize = 2;
/// Largest a line, carry or fold buffer may be when a scanner goes back to the pool.
const IDLE_CAP: usize = 64 * 1024;

/// Scanners waiting for the next search. A keystroke starts a search about every 250 ms, and the 256 KiB block is
/// above glibc's `mmap` threshold, so allocating it per search pays page faults each time.
pub struct ScannerPool {
    idle: Mutex<Vec<Scanner>>,
}

static POOL: ScannerPool = ScannerPool::new();

impl ScannerPool {
    pub const fn new() -> Self {
        ScannerPool {
            idle: Mutex::new(Vec::new()),
        }
    }

    /// An idle scanner, or a new one when none is. It returns here when dropped, whichever way the search ended.
    pub fn take(&'static self) -> PooledScanner {
        let idle = self.idle.lock().unwrap_or_else(|e| e.into_inner()).pop();
        PooledScanner {
            scanner: Some(idle.unwrap_or_default()),
            pool: self,
        }
    }

    #[cfg(test)]
    fn idle(&self) -> usize {
        self.idle.lock().unwrap_or_else(|e| e.into_inner()).len()
    }
}

/// A `Scanner` on loan from the pool.
pub struct PooledScanner {
    scanner: Option<Scanner>,
    pool: &'static ScannerPool,
}

impl PooledScanner {
    /// One from the process-wide pool.
    pub fn take() -> Self {
        POOL.take()
    }
}

impl std::ops::Deref for PooledScanner {
    type Target = Scanner;
    fn deref(&self) -> &Scanner {
        self.scanner.as_ref().expect("held until drop")
    }
}

impl std::ops::DerefMut for PooledScanner {
    fn deref_mut(&mut self) -> &mut Scanner {
        self.scanner.as_mut().expect("held until drop")
    }
}

impl Drop for PooledScanner {
    fn drop(&mut self) {
        let Some(mut scanner) = self.scanner.take() else {
            return;
        };
        scanner.tidy();
        let mut idle = self.pool.idle.lock().unwrap_or_else(|e| e.into_inner());
        if idle.len() < POOL_MAX {
            idle.push(scanner);
        }
    }
}

fn read_exact_at(file: &File, buf: &mut [u8], offset: u64) -> io::Result<()> {
    let mut done = 0;
    while done < buf.len() {
        match file.read_at(&mut buf[done..], offset + done as u64) {
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(n) => done += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// The files, newest modification first: the order a search visits them in.
pub fn newest_first(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut keyed: Vec<(SystemTime, PathBuf)> = paths
        .into_iter()
        .map(|p| {
            let m = std::fs::metadata(&p)
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            (m, p)
        })
        .collect();
    keyed.sort_by_key(|k| std::cmp::Reverse(k.0));
    keyed.into_iter().map(|(_, p)| p).collect()
}

/// Line start offsets of one file, about every `INDEX_STRIDE` bytes, so a window can seek near a turn instead of
/// reading from the top. The files are append-only: the index is extended from where it ended, and dropped when the
/// fingerprint says the file was rewritten.
#[derive(Debug, Default)]
pub struct LineIndex {
    fingerprint: Option<Fingerprint>,
    marks: Vec<u64>,
    /// Everything before this is indexed, and it is a line start. A torn last line is left for the next refresh.
    end: u64,
}

impl LineIndex {
    pub fn end(&self) -> u64 {
        self.end
    }

    pub fn marks(&self) -> &[u64] {
        &self.marks
    }

    /// The last mark at or before `target`: a line start to read forward from.
    pub fn seek(&self, target: u64) -> u64 {
        let i = self.marks.partition_point(|&m| m <= target);
        if i == 0 {
            0
        } else {
            self.marks[i - 1]
        }
    }

    /// Brings the index up to date with `file`.
    pub fn refresh(&mut self, file: &File, scanner: &mut Scanner) -> io::Result<()> {
        let now = Fingerprint::of_meta(&file.metadata()?);
        if !self.fingerprint.is_some_and(|f| f.still_holds(&now)) {
            *self = LineIndex::default();
        }
        self.fingerprint = Some(now);
        if self.marks.is_empty() {
            self.marks.push(0);
        }
        let mut next_mark = self.marks.last().copied().unwrap_or(0) + INDEX_STRIDE;
        let mut pos = self.end;
        while pos < now.size {
            let len = (now.size - pos).min(BLOCK as u64) as usize;
            read_exact_at(file, &mut scanner.buf[..len], pos)?;
            for nl in memchr::memchr_iter(b'\n', &scanner.buf[..len]) {
                let line_start = pos + nl as u64 + 1;
                self.end = line_start;
                if line_start >= next_mark {
                    self.marks.push(line_start);
                    next_mark = line_start + INDEX_STRIDE;
                }
            }
            pos += len as u64;
        }
        Ok(())
    }
}

/// The indexes of the most recently used files.
pub struct IndexCache {
    cap: usize,
    entries: Mutex<VecDeque<(PathBuf, Arc<Mutex<LineIndex>>)>>,
}

impl Default for IndexCache {
    fn default() -> Self {
        Self::new(INDEX_CACHE_FILES)
    }
}

impl IndexCache {
    pub fn new(cap: usize) -> Self {
        IndexCache {
            cap: cap.max(1),
            entries: Mutex::new(VecDeque::new()),
        }
    }

    /// The index for `path`, made if it is new; the least recently used one is dropped past the cap.
    pub fn get(&self, path: &Path) -> Arc<Mutex<LineIndex>> {
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(i) = entries.iter().position(|(p, _)| p == path) {
            let entry = entries.remove(i).expect("position is in range");
            let index = entry.1.clone();
            entries.push_front(entry);
            return index;
        }
        let index = Arc::new(Mutex::new(LineIndex::default()));
        entries.push_front((path.to_path_buf(), index.clone()));
        entries.truncate(self.cap);
        index
    }

    pub fn len(&self) -> usize {
        self.entries.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Runs a scan on a blocking thread, off the control path. `None` if it panicked.
pub async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    tokio::task::spawn_blocking(f).await.ok()
}

#[cfg(test)]
pub(crate) mod tests;
