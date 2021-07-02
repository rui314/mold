# Logic_sim sample
This directory contains  `oneapi::tbb::flow` example that performs simplistic digital logic simulations with basic logic gates that can be easily composed to create more interesting circuits.

## Building the example
```
cmake <path_to_example>
cmake --build .
```

## Running the sample
### Predefined make targets
* `make run_logic_sim` - executes the example with predefined parameters.
* `make perf_run_logic_sim` - executes the example with suggested parameters to measure the oneTBB performance.

### Application parameters
Usage:
```
logic_sim [#threads=value] [verbose] [silent] [-h] [#threads]
```
* `-h` - prints the help for command line options.
* `#threads` - the number of threads to use; a range of the form low[:high] where low and optional high are non-negative integers, or `auto` for a platform-specific default number.
* `verbose` - prints diagnostic output to screen.
* `silent` limits output to timing info; overrides verbose
