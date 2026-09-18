# Release performance comparison

Both Rust linkers use the standard all-target Cargo release build, with Rust
1.97.1 and the repository's default release profile. The baseline is Rust
commit `b11bcd8e6db919f3e6d26697e3b3dd09b133a890`; the updated build contains
the changes documented in [upstream-sync.md](upstream-sync.md). The C++
reference is `dad2695f3e62d0dd6024b0b795dc2f940907be12`, built by GCC 13.3
with CMake `Release` (`-O3 -DNDEBUG`), mimalloc enabled and LTO disabled.

Measurements were taken on September 9, 2026, on an AMD Ryzen Threadripper
7980X (64 cores, 128 hardware threads). Each linker used 32 worker threads,
which is the default for both implementations on this machine. Workload
names containing `debug` or `release` describe the input application; every
linker binary measured here is a release build.

Each invocation ran in its workload directory under `~/bench`:

```sh
LINKER @rsp --no-fork --threads=32 -o TEMPORARY_OUTPUT
```

The output was removed after each invocation. `--no-fork` includes process
teardown in the elapsed time and permits complete child CPU accounting,
following the C++ benchmark suite's protocol. Each workload had one warm-up
per linker followed by three measured rounds, rotating linker order. The
table reports medians with warm input caches.

Before each invocation, the harness sampled machine CPU and disk activity
for two seconds. It waited if CPU activity exceeded 1.5 cores or either the
input or output filesystem's device was busy for more than 5% of that
interval, checking again after 15 seconds. Inactive dirty pages did not
delay a run. During each invocation it estimated other CPU activity from
host busy time minus linker CPU time, discarding and retrying samples
exceeding one other active core. Earlier measurements taken with different
activity checks are excluded.

Across the 21 workloads, the geometric mean Rust/C++ time ratio is **1.029**, and updated/baseline Rust is **0.939**. The sum of median Rust times divided by the C++ sum is 1.032.

Lower ratios indicate faster Rust links. These measurements describe this host and these workloads.

The largest remaining gap is `chrome-debug-arm64`, where Rust takes 9.6% longer than C++.

| Workload | Baseline Rust (s) | Updated Rust (s) | C++ (s) | Rust / C++ |
| --- | ---: | ---: | ---: | ---: |
| clang-debug | 1.281 | 1.229 | 1.343 | 0.915 |
| chrome-debug-x86_64 | 1.848 | 1.832 | 1.719 | 1.066 |
| chrome-debug-arm64 | 2.035 | 2.002 | 1.828 | 1.096 |
| firefox-debug | 0.849 | 0.770 | 0.750 | 1.027 |
| godot-release | 0.093 | 0.089 | 0.084 | 1.068 |
| blender-debug | 0.855 | 0.777 | 0.838 | 0.926 |
| blender-release | 0.189 | 0.187 | 0.182 | 1.028 |
| chrome-release-x86_64 | 0.741 | 0.700 | 0.668 | 1.047 |
| chrome-release-arm64 | 0.850 | 0.817 | 0.774 | 1.056 |
| clang-release | 0.111 | 0.109 | 0.100 | 1.089 |
| clickhouse-debug | 1.070 | 1.009 | 0.979 | 1.030 |
| clickhouse-release | 0.433 | 0.434 | 0.413 | 1.049 |
| firefox-debug-arm64 | 0.927 | 0.894 | 0.854 | 1.047 |
| firefox-release | 0.210 | 0.201 | 0.202 | 0.992 |
| godot-debug | 0.461 | 0.378 | 0.366 | 1.032 |
| libmergedlo.so-debug | 0.464 | 0.460 | 0.434 | 1.061 |
| libmergedlo.so-release | 0.186 | 0.185 | 0.189 | 0.979 |
| pytorch-debug | 0.779 | 0.764 | 0.783 | 0.976 |
| pytorch-release | 0.150 | 0.147 | 0.140 | 1.045 |
| tensorflow-debug | 2.967 | 2.281 | 2.149 | 1.062 |
| tensorflow-release | 0.639 | 0.498 | 0.479 | 1.039 |

The [raw samples](upstream-sync-benchmark.csv) include warm-ups and rejected runs; only accepted, non-warm-up samples enter the medians.

The final run contains 189 accepted measurements, 63 accepted warm-ups, and 55 rejected attempts.
