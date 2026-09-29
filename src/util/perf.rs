//! Statistics counters and wall-clock and CPU time accounting.

// Counter is used to collect statistics numbers.

use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex, Once};
use std::time::Instant;

use crate::out;

static COUNTERS_ENABLED: AtomicBool = AtomicBool::new(false);
static COUNTERS: Mutex<Vec<&'static Counter>> = Mutex::new(Vec::new());

pub struct Counter {
    name: &'static str,
    value: AtomicI64,
    registered: Once,
}

impl Counter {
    pub const fn new(name: &'static str) -> Self {
        Self { name, value: AtomicI64::new(0), registered: Once::new() }
    }

    pub fn enable() {
        COUNTERS_ENABLED.store(true, Ordering::Relaxed);
    }

    #[inline]
    pub fn increment(&'static self) {
        self.add(1);
    }

    #[inline]
    pub fn add(&'static self, delta: i64) {
        if !COUNTERS_ENABLED.load(Ordering::Relaxed) {
            return;
        }
        self.registered.call_once(|| {
            COUNTERS.lock().unwrap().push(self);
        });
        self.value.fetch_add(delta, Ordering::Relaxed);
    }

    pub fn print(stats: impl IntoIterator<Item = (&'static str, i64)>) {
        let mut counters: Vec<_> = COUNTERS
            .lock()
            .unwrap()
            .iter()
            .map(|counter| (counter.name, counter.value.load(Ordering::Relaxed)))
            .collect();
        counters.extend(stats);
        counters.sort_by_key(|&(_, value)| value);
        for (name, value) in counters {
            out!("{:>20}={}", name, value);
        }
    }
}

#[derive(Debug)]
struct Record {
    name: String,
    start: Instant,
    end: Option<Instant>,
    user_start: f64,
    sys_start: f64,
    user: f64,
    sys: f64,
    parent: Option<usize>,
    children: Vec<usize>,
}

// Infer implicit parents from complete intervals, as C++ mold does. Overlapping
// timers need not be nested, and explicitly named parents take precedence.
fn nest_records(records: &mut [Record]) {
    for record in records.iter_mut() {
        record.children.clear();
    }
    for i in 0..records.len() {
        if records[i].parent.is_none() {
            let inner = &records[i];
            records[i].parent = records[..i].iter().rposition(|outer| {
                outer.start <= inner.start && inner.end.unwrap() <= outer.end.unwrap()
            });
        }
        if let Some(parent) = records[i].parent {
            records[parent].children.push(i);
        }
    }
}

/// Collects wall-clock and CPU timing records for the passes of a link.
/// Cloning shares the underlying records.
#[derive(Clone, Debug)]
pub struct Timers {
    records: Option<Arc<Mutex<Vec<Record>>>>,
}

/// A running timer, stopped when dropped.
#[must_use = "a timer records its interval when dropped"]
pub struct Timer {
    timers: Timers,
    index: usize,
    stopped: bool,
}

/// A clonable handle for starting timers beneath a running timer.
#[derive(Clone)]
pub struct TimerHandle {
    timers: Timers,
    parent: usize,
}

#[cfg(windows)]
#[repr(C)]
struct FileTime {
    low: u32,
    high: u32,
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentProcess() -> *mut std::ffi::c_void;
    fn GetProcessTimes(
        process: *mut std::ffi::c_void,
        creation: *mut FileTime,
        exit: *mut FileTime,
        kernel: *mut FileTime,
        user: *mut FileTime,
    ) -> i32;
}

#[cfg(not(windows))]
fn rusage() -> (f64, f64) {
    // SAFETY: `usage` is a valid, writable rusage struct.
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &raw mut usage) };
    let to_secs = |t: libc::timeval| t.tv_sec as f64 + t.tv_usec as f64 / 1_000_000.0;
    (to_secs(usage.ru_utime), to_secs(usage.ru_stime))
}

#[cfg(windows)]
fn rusage() -> (f64, f64) {
    let mut creation = FileTime { low: 0, high: 0 };
    let mut exit = FileTime { low: 0, high: 0 };
    let mut kernel = FileTime { low: 0, high: 0 };
    let mut user = FileTime { low: 0, high: 0 };
    // SAFETY: all FILETIME pointers are valid outputs and the pseudo-handle
    // returned by GetCurrentProcess is always valid in this process.
    unsafe {
        GetProcessTimes(GetCurrentProcess(), &mut creation, &mut exit, &mut kernel, &mut user);
    }
    let to_secs = |time: FileTime| {
        let ticks = (u64::from(time.high) << 32) | u64::from(time.low);
        ticks as f64 / 10_000_000.0
    };
    (to_secs(user), to_secs(kernel))
}

impl Default for Timers {
    fn default() -> Self {
        Self::new()
    }
}

impl Timers {
    pub fn new() -> Self {
        Self { records: Some(Arc::new(Mutex::new(Vec::new()))) }
    }

    /// Skips clock reads, system calls and shared recording when --perf is off.
    pub fn disabled() -> Self {
        Self { records: None }
    }

    fn inactive(&self) -> Timer {
        Timer { timers: self.clone(), index: 0, stopped: true }
    }

    /// Starts a timer whose nesting is inferred from completed time intervals.
    pub fn start(&self, name: &str) -> Timer {
        self.start_child(name, None)
    }

    fn start_child(&self, name: impl std::fmt::Display, parent: Option<usize>) -> Timer {
        let Some(records) = &self.records else {
            return self.inactive();
        };
        let (user, sys) = rusage();
        let mut records = records.lock().unwrap();
        let index = records.len();
        records.push(Record {
            name: name.to_string(),
            start: Instant::now(),
            end: None,
            user_start: user,
            sys_start: sys,
            user: 0.0,
            sys: 0.0,
            parent,
            children: Vec::new(),
        });
        Timer { timers: self.clone(), index, stopped: false }
    }

    fn stop(&self, index: usize) {
        let (user, sys) = rusage();
        let mut records = self.records.as_ref().unwrap().lock().unwrap();
        let record = &mut records[index];
        if record.end.is_none() {
            record.end = Some(Instant::now());
            record.user = user - record.user_start;
            record.sys = sys - record.sys_start;
        }
    }

    pub fn print(&self) {
        let Some(records) = &self.records else {
            return;
        };
        let mut records = records.lock().unwrap();
        let now = Instant::now();
        for r in records.iter_mut() {
            if r.end.is_none() {
                r.end = Some(now);
            }
        }
        nest_records(&mut records);

        fn print_rec(records: &[Record], i: usize, indent: usize) {
            let r = &records[i];
            let real = r.end.unwrap().duration_since(r.start).as_secs_f64();
            println!(
                " {:8.3} {:8.3} {:8.3}  {}{}",
                r.user,
                r.sys,
                real,
                " ".repeat(indent * 2),
                r.name
            );
            // nest_records adds children in their recorded start order.
            for &child in &r.children {
                print_rec(records, child, indent + 1);
            }
        }

        println!("     User   System     Real  Name");
        for i in 0..records.len() {
            if records[i].parent.is_none() {
                print_rec(&records, i, 0);
            }
        }
    }
}

impl Timer {
    /// Starts a timer nested in this one. Timers started from parallel
    /// tasks name their parent this way, since which timer started last
    /// says nothing about nesting then.
    pub fn child(&self, name: impl std::fmt::Display) -> Self {
        self.timers.start_child(name, Some(self.index))
    }

    pub fn handle(&self) -> TimerHandle {
        TimerHandle { timers: self.timers.clone(), parent: self.index }
    }

    pub fn stop(&mut self) {
        if !self.stopped {
            self.stopped = true;
            self.timers.stop(self.index);
        }
    }
}

impl TimerHandle {
    pub fn child(&self, name: impl std::fmt::Display) -> Timer {
        self.timers.start_child(name, Some(self.parent))
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn implicit_timer_parents_follow_completed_intervals() {
        let timers = Timers::new();
        let root = timers.start("root");
        let _running = [
            timers.start("foreground"),
            timers.start("nested"),
            timers.start("background"),
            timers.start("next foreground"),
            root.child("explicit child"),
        ];
        let mut records = timers.records.as_ref().unwrap().lock().unwrap();
        let start = records[0].start;
        for (record, (begin, end)) in
            records.iter_mut().zip([(0, 100), (10, 40), (15, 25), (20, 70), (50, 80), (60, 65)])
        {
            record.start = start + std::time::Duration::from_millis(begin);
            record.end = Some(start + std::time::Duration::from_millis(end));
        }
        nest_records(&mut records);
        let children: Vec<_> = records.iter().map(|r| r.children.clone()).collect();
        drop(records);
        assert_eq!(children, [vec![1, 3, 4, 5], vec![2], vec![], vec![], vec![], vec![]]);
    }

    #[test]
    fn optional_recording_includes_nested_and_background_timers() {
        for timers in [Timers::new(), Timers::disabled()] {
            let mut root = timers.start("root");
            let child = root.child("child");
            let handle = root.handle();
            std::thread::spawn(move || drop(handle.child("background"))).join().unwrap();
            drop(child);
            root.stop();
            root.stop();
            if let Some(records) = &timers.records {
                let mut records = records.lock().unwrap();
                nest_records(&mut records);
                assert_eq!(records.len(), 3);
                assert!(records.iter().all(|record| record.end.is_some()));
                assert_eq!(records[0].children, [1, 2]);
            } else {
                assert!(root.timers.records.is_none());
            }
        }
    }
}
