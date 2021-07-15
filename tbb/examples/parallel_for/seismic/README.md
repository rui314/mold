# Seismic sample
Parallel seismic wave simulation that demonstrates use of `parallel_for` and `affinity_partitioner`.

## Building the example
```
cmake <path_to_example> [EXAMPLES_UI_MODE=value]
cmake --build .
```
### Predefined CMake variables
* `EXAMPLES_UI_MODE` - defines the GUI mode, supported values are `gdi`, `d2d`, `con` on Windows, `x`,`con` on Linux and `mac`,`con` on macOS. The default mode is `con`. See the [common page](../../README.md) to get more information.

## Running the sample
### Predefined make targets
* `make run_seismic` - executes the example with predefined parameters.
* `make perf_run_seismic` ` - executes the example with suggested parameters to measure the oneTBB performance.
* `make light_test_seismic` - executes the example with suggested parameters to reduce execution time.

### Application parameters
Usage:
```
seismic [n-of-threads=value] [n-of-frames=value] [silent] [serial] [-h] [n-of-threads [n-of-frames]]
```
* `-h` - prints the help for command line options.
* `n-of-threads` -e number of threads to use; a range of the form low\[:high\], where low and optional high are non-negative integers or `auto` for a platform-specific default number.
* `n-of-frames` - number of frames the example processes internally.
* `silent` - no output except elapsed time.
* `serial` - in GUI mode start with serial version of algorithm.

### Interactive graphical user interface
The following hot keys can be used in interactive execution mode when the example is compiled with the graphical user interface:

* `left mouse button` - starts new seismic wave in place specified by mouse cursor.
* `space` - toggles between parallel and serial execution modes.
* `p` - enables parallel execution mode.
* `s` - enables serial execution mode.
* `e` - enables screen updates.
* `d` - disables screen updates (strongly recommended when measuring performance or scalability; see note below).
* `esc` - stop execution.
