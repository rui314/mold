# Convex_hull sample
Parallel version of convex hull algorithm (quick hull).

## Building the example
```
cmake <path_to_example>
cmake --build .
```

This sample contains two additional predefined build targets:
- `convex_hull_sample` -  builds parallel version of the example which uses `parallel_reduce`, `parallel_for` and `concurrent_vector`.
- `convex_hull_bench` - build version of the example that compares serial and parallel buffered and unbuffered implementations.

## Running the sample
### Predefined make targets
* `make run_convex_hull` - executes the example with predefined parameters.
* `make perf_run_convex_hull` - executes the example with suggested parameters to measure the oneTBB performance.
* `make light_test_convex_hull` - executes the example with suggested parameters to reduce execution time.

### Application parameters
Usage:
```
convex_hull_sample [n-of-threads=value] [n-of-points=value] [silent] [verbose] [-h] [n-of-threads [n-of-points]]
convex_hull_bench [n-of-threads=value] [n-of-points=value] [silent] [verbose] [-h] [n-of-threads [n-of-points]]
```
* `-h` - prints the help for command line options.
* `n-of-threads` - the number of threads to use; a range of the form low\[:high\], where low and optional high are non-negative integers or `auto` for a platform-specific default number.
* `n-of-points` - number of points.
* `silent` - no output except elapsed time.
* `verbose` - turns verbose ON.
