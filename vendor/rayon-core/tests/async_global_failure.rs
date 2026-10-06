#[cfg(unix)]
#[test]
fn late_spawn_failure_aborts() {
    use rayon_core::ThreadPoolBuilder;
    use std::os::unix::process::ExitStatusExt;
    use std::process::Command;
    use std::time::{Duration, Instant};
    const CHILD: &str = "RAYON_ASYNC_FAILURE_CHILD";
    if std::env::var_os(CHILD).is_some() {
        ThreadPoolBuilder::new()
            .num_threads(2)
            .stack_size(usize::MAX)
            .build_global_async()
            .unwrap();
        std::thread::sleep(Duration::from_secs(5));
        panic!("worker creation failure was not fail-stop");
    }
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "late_spawn_failure_aborts", "--nocapture"])
        .env(CHILD, "1")
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert_eq!(status.signal(), Some(libc::SIGABRT));
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("failed startup hung instead of aborting");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
