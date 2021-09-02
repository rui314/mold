# Primes sample
Parallel version of the Sieve of Eratosthenes.

## Building the example
```
cmake <path_to_example>
cmake --build .
```

## Running the sample
### Predefined make targets
* `make run_primes` - executes the example with predefined parameters
* `make perf_run_primes` - executes the example with suggested parameters to measure the oneTBB performance

### Application parameters
Usage:
```
primes [n-of-threads=value] [number=value] [grain-size=value] [n-of-repeats=value] [silent] [-h] [n-of-threads [number [grain-size [n-of-repeats]]]]
```
* `-h` - prints the help for command line options.
* `n-of-threads` - the number of threads to use; a range of the form low\[:high\], where low and optional high are non-negative integers or `auto` for a platform-specific default number.
* `number` - the upper bound of range to search primes in, must be a positive integer.
* `grain-size` - the optional grain size, must be a positive integer.
* `n-of-repeats` - the number of the calculation repeats, must be a positive integer.
* `silent` - no output except elapsed time.
