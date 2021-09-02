# Binpack sample
This directory contains an `oneapi::tbb::flow` example that performs binpacking of `N` integer values into a near-optimal number of bins of capacity `V`.

It features a `source_node` which passes randomly generated integer values of `size <= V` to a `queue_node`. Multiple function nodes set about taking values from this `queue_node` and packing them into bins according to a best-fit policy. Items that cannot be made to fit are rejected and returned to the queue. When a bin is packed as well as it can be, it is passed to a `buffer_node` where it waits to be picked up by another `function_node`. This final function node gathers stats about the bin and optionally prints its contents. When all bins are accounted for, it optionally prints a summary of the quality of the bin-packing.

## Building the example
```
cmake <path_to_example>
cmake --build .
```

## Running the sample
### Predefined make targets
* `make run_binpack` - executes the example with predefined parameters.
* `make perf_run_binpack` - executes the example with suggested parameters to measure the oneTBB performance.

### Application parameters
Usage:
```
binpack [#threads=value] [verbose] [silent] [elements_num=value] [bin_capacity=value] [#packers=value] [optimality=value] [-h] [#threads]
```
* `-h` - prints the help for command line options.
* `#threads` - the number of threads to use; a range of the form low\[:high\] where low and optional high are non-negative integers, or `auto` for a platform-specific default number.
* `verbose` - prints diagnostic output to screen.
* `silent` - limits output to timing info; overrides verbose.
* `N` - number of values to pack.
* `V` - capacity of each bin.
* `#packers` - number of concurrent bin packers to use (`default=#threads`).
* `optimality` - controls optimality of solution; 1 is highest, use larger numbers for less optimal but faster solution.
