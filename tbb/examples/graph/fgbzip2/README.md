# fgbzip2 sample
fgbzip2 is a parallel implementation of bzip2 block-sorting file compressor that uses `oneapi::tbb::flow`. The output of this application is fully compatible with bzip2 v1.0.6 or newer.

This example includes software developed by Julian R Seward. See here for copyright information.
It exemplifies support for asynchronous capabilities in the flow graph API.

## Building the example
```
cmake <path_to_example>
cmake --build .
```

## Running the sample
### Predefined make targets
* `make run_fgbzip2` - executes the example with predefined parameters.
* `make perf_run_fgbzip2` - executes the example with suggested parameters to measure the oneTBB performance.

### Application parameters
Usage:
```
fgbzip2 [-b=value] [-v] [-l=value] [-async] [filename=value] [-h] [filename]
```
* `-h` - prints the help for command line options.
* `-b` - block size in 100 KB chunks, [1 .. 9].
* `-v` - prints diagnostic output to screen.
* `-l` - use memory limit for compression algorithm with 1 MB (minimum) granularity.
* `-async` - use graph `async_node`-based implementation.
* `filename` - name of the file to compress.
