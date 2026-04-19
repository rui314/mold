# Fibonacci sample
This directory contains an example that computes Fibonacci numbers using emulation for TBB Task API.

## Building the example
```
cmake <path_to_example>
cmake --build .
```

## Running the sample
### Predefined make targets
* `make run_recursive_fibonacci` - executes the example with predefined parameters (extended testing enabled).
* `make perf_run_recursive_fibonacci` - executes the example with suggested parameters to measure the oneTBB performance.

### Application parameters
Usage:
```
recursive_fibonacci N C I T
```
* `N` - specifies the fibonacci number which would be calculated.
* `C` - cutoff that will be used to stop recursive split.
* `I` - number of iteration to measure benchmark time.
* `T` - enables extended testing (recycle task in a loop).
