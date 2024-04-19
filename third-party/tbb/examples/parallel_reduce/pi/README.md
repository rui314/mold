# Pi Sample
Parallel version of calculating &pi; by numerical integration.

## Build
To build the sample, run the following commands:
```
cmake <path_to_example>
cmake --build .
```

## Run
### Predefined Make Targets
* `make run_pi` - executes the example with predefined parameters
* `make perf_run_pi` - executes the example with suggested parameters to measure the oneTBB performance

### Application Parameters
You can use the following application parameters:
```
pi [n-of-threads=value] [n-of-intervals=value] [silent] [-h] [n-of-threads [n-of-intervals]]
```
* `-h` - prints the help for command-line options.
* `n-of-threads` - the number of threads to use. This number is specified in the low\[:high\] range format, where both ``low`` and, optionally, ``high`` are non-negative integers. You can also use ``auto`` to let the system choose a default number of threads suitable for the platform.
* `n-of-intervals` - the number of intervals to subdivide into. Must be a positive integer.
* `silent` - no output except the elapsed time.
