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
    pub const fn new(name: &'static str) -> Counter {
        Counter {
            name,
            value: AtomicI64::new(0),
            registered: Once::new(),
        }
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

    pub fn print() {
        let mut counters = COUNTERS.lock().unwrap().clone();
        counters.sort_by_key(|counter| counter.value.load(Ordering::Relaxed));
        for counter in counters {
            out!(
                "{:>20}={}",
                counter.name,
                counter.value.load(Ordering::Relaxed)
            );
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

/// Collects wall-clock and CPU timing records for the passes of a link.
/// Cloning shares the underlying records.
#[derive(Clone, Debug)]
pub struct Timers {
    records: Option<Arc<Mutex<Vec<Record>>>>,
}

/// A running timer, stopped when dropped.
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
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
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
        GetProcessTimes(
            GetCurrentProcess(),
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        );
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
        Timers {
            records: Some(Arc::new(Mutex::new(Vec::new()))),
        }
    }

    /// Skips clock reads, system calls and shared recording when --perf is off.
    pub fn disabled() -> Self {
        Timers { records: None }
    }

    fn inactive(&self) -> Timer {
        Timer {
            timers: self.clone(),
            index: 0,
            stopped: true,
        }
    }

    /// Starts a timer, nested in the timer still running that started last.
    pub fn start(&self, name: &str) -> Timer {
        let Some(records) = &self.records else {
            return self.inactive();
        };
        let records = records.lock().unwrap();
        let parent = records.iter().rposition(|r| r.end.is_none());
        drop(records);
        self.start_child(name, parent)
    }

    fn start_child(&self, name: &str, parent: Option<usize>) -> Timer {
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
        if let Some(parent) = parent {
            records[parent].children.push(index);
        }
        Timer {
            timers: self.clone(),
            index,
            stopped: false,
        }
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
            let mut children = r.children.clone();
            children.sort_by_key(|&c| records[c].start);
            for child in children {
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
    pub fn child(&self, name: &str) -> Timer {
        self.timers.start_child(name, Some(self.index))
    }

    pub fn handle(&self) -> TimerHandle {
        TimerHandle {
            timers: self.timers.clone(),
            parent: self.index,
        }
    }

    pub fn stop(&mut self) {
        if !self.stopped {
            self.stopped = true;
            self.timers.stop(self.index);
        }
    }
}

impl TimerHandle {
    pub fn child(&self, name: &str) -> Timer {
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
    fn optional_recording_includes_nested_and_background_timers() {
        for timers in [Timers::new(), Timers::disabled()] {
            let mut root = timers.start("root");
            let child = root.child("child");
            let handle = root.handle();
            std::thread::spawn(move || drop(handle.child("background")))
                .join()
                .unwrap();
            drop(child);
            root.stop();
            root.stop();
            if let Some(records) = &timers.records {
                let records = records.lock().unwrap();
                assert_eq!(records.len(), 3);
                assert!(records.iter().all(|record| record.end.is_some()));
                assert_eq!(records[0].children, [1, 2]);
            } else {
                assert!(root.timers.records.is_none());
            }
        }
    }
}
