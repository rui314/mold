use rayon_core::{broadcast, current_num_threads, join, ThreadPoolBuilder};

#[test]
fn sole_current_worker_needs_no_helper() {
    std::thread::spawn(|| {
        ThreadPoolBuilder::new()
            .num_threads(1)
            .use_current_thread()
            .build_global_async()
            .unwrap();
        assert_eq!(current_num_threads(), 1);
        assert_eq!(join(|| 3, || 4), (3, 4));
        assert_eq!(broadcast(|ctx| ctx.index()), vec![0]);
    })
    .join()
    .unwrap();
}
