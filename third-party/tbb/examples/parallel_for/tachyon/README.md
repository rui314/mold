# Tachyon sample
Parallel raytracer / renderer that demonstrates the use of parallel_for.

*This example includes software developed by John E. Stone.*

This example is a 2-D raytracer/renderer that visually shows different parallel scheduling methods and their resulting speedup. The code was parallelized by speculating that each pixel could be rendered in parallel. The resulting parallel code was then checked for correctness by using Intel® Thread Checker, which pointed out where synchronization was needed. Minimal synchronization was then inserted into the parallel code. The resulting parallel code exhibits good speedup.

## Building the example
```
cmake <path_to_example> [EXAMPLES_UI_MODE=value]
cmake --build .
```
### Predefined CMake variables
* `EXAMPLES_UI_MODE` - defines the GUI mode, supported values are `gdi`, `d2d`, `con` on Windows, `x`,`con` on Linux and `mac`,`con` on macOS. The default mode is `con`. See the [common page](../../README.md) to get more information.

* `TACHYON_VERSION` - this examples contains several version that may be changed via `TACHYON_VERSION` Cmake variable.
    * **serial** - Original sequential version.
    * **tbb1d** - Parallel version that uses oneAPI Threading Building Blocks (oneTBB) and `blocked_range` to parallelize over tasks that are groups of scan-lines.
    * **tbb** (Default) - Parallel version that uses oneTBB and `blocked_range2d` to parallelize over tasks that are rectangular sub-areas.

## Running the sample
### Predefined make targets
* `make run_tachyon` - executes the example with predefined parameters.
* `make perf_run_tachyon` ` - executes the example with suggested parameters to measure the oneTBB performance.
* `make light_test_tachyon` - executes the example with suggested parameters to reduce execution time.

### Application parameters
Usage:
```
tachyon [dataset=value] [boundthresh=value] [no-display-updating] [no-bounding] [silent] [-h] [dataset [boundthresh]]
```
* `-h` - Prints the help for command line options.
* `dataset` - path/name of one of the *.dat files in the dat directory for the example.
* `boundthresh` - bounding threshold value.
* `no-display-updating` - disable run-time display updating.
* `no-bounding` - disable bounding technique.

### Environment variables
The `tbb` and `tbb1d` version of examples has the following settings that may be handled by environment variables:
* By default, these versions use one thread per available processor. To change this default, set the `TBB_NUM_THREADS` environment variable to the desired number of threads before running.
* These versions use `auto_partitioner` by default. To change this default, set the `TBB_PARTITIONER` environment variable to the `aff` value to use `affinity_partitioner` and to `simp` to use `simple_partitioner`.
* These versions use a reasonable task grain size by default. To change this default, set the `TBB_GRAINSIZE` environment variable to the desired grain size before running. The grain size corresponds to the number of pixels (in the `X` or `Y` direction, for a rectangular sub-area) in each parallel task.

### Interactive graphical user interface
The following hot keys can be used in interactive execution mode when the example is compiled with the graphical user interface:

* `any key` - enable repetition of rendering after the pause. Press ESC to stop the application.
* `space` - toggle run-time display updating mode while rendering (see no-display-updating above).
* `p` - holds the picture after rendering completion. Press 'p' again to continue.
* `esc` - stop execution.
