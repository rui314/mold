# Fractal sample
The example calculates two classical Mandelbrot fractals with different concurrency levels.

The application window is divided into two areas where fractals are rendered. The example also has the console mode.

## Building the example
```
cmake <path_to_example> [EXAMPLES_UI_MODE=value]
cmake --build .

```
### Predefined CMake variables
* `EXAMPLES_UI_MODE` - defines the GUI mode, supported values are `gdi`, `d2d`, `con` on Windows, `x`,`con` on Linux and `mac`,`con` on macOS. The default mode is `con`. See the [common page](../../README.md) to get more information.

## Running the sample
### Predefined make targets
* `make run_fractal` - executes the example with predefined parameters.
* `make perf_run_fractal` - executes the example with suggested parameters to measure the oneTBB performance.
* `make light_test_fractal` - executes the example with suggested parameters to reduce execution time.

### Application parameters
Usage:
```
fractal [n-of-threads=value] [n-of-frames=value] [max-of-iterations=value] [grain-size=value] [use-auto-partitioner] [silent] [single] [-h] [n-of-threads [n-of-frames [max-of-iterations [grain-size]]]]
```
* `-h` - prints the help for command line options.
* `n-of-threads` - the number of threads to use; a range of the form low\[:high\], where low and optional high are non-negative integers or `auto` for a platform-specific default number.
* `n-of-frames` - the number of frames the example processes internally.
* `max-of-iterations` - the maximum number of the fractal iterations.
* `grain-size` - the optional grain size, must be a positive integer.
* `use-auto-partitioner` - use oneapi::tbb::auto_partitioner.
* `silent` - no output except elapsed time.
* `single` - process only one fractal.

### Interactive graphical user interface
The following hot keys can be used in interactive execution mode when the example is compiled with the graphical user interface:

* `left mouse button` - make the fractal active.
* `w` - move the active fractal up.
* `a` - move the active fractal to the left.
* `s` - move the active fractal down.
* `d` - move the active fractal to the right.
* `q` - zoom in the active fractal.
* `e` - zoom out the active fractal.
* `r` - increase quality (count of iterations for each pixel) the active fractal.
* `f` - decrease quality (count of iterations for each pixel) the active fractal.
* `esc` - stop execution.
