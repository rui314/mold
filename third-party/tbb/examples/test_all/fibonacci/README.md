# Fibonacci sample
This directory contains an example that computes Fibonacci numbers in several different ways.

The purpose of the example is to exercise every include file and class in Intel® oneAPI Threading Building Blocks. Most of the computations are deliberately silly and not expected to show any speedup on multiprocessors.

## Building the example
```
cmake <path_to_example>
cmake --build .
```

## Running the sample
### Predefined make targets
* `make run_fibonacci` - executes the example with predefined parameters.

### Application parameters
Usage:
```
fibonacci K [M[:N]] [R]
```
* `K` - specifies the fibonacci number which would be calculated.
* `[M:N]` -a range of numbers of threads to be used.
* `R` - the number of times to repeat the calculation.
