# Cholesky sample
This directory contains an example of several versions of Cholesky Factorization algorithm.

**dpotrf**: An implementation that calls the oneAPI Math Kernel Library (oneMKL) `dpotrf` function to directly perform the factorization. This can be a serial implementation or threaded implementation depending on the version of the oneMKL library that is linked against.

**crout**: A serial implementation that uses the Crout-Cholesky algorithm for factorization. The same approach is parallelized for the other oneAPI Threading Building Blocks (oneTBB) based approaches below.

**depend**: A parallel version of Crout-Cholesky factorization that uses the oneTBB flow graph. This version uses a dependency graph made solely of `continue_node` objects. This an inspector-executor approach, where a loop nest that is similar to the serial implementation is used to create an unrolled version of the computation. Where the oneMKL calls would have been made in the original serial implementation of Crout-Cholesky, graph nodes are created instead and these nodes are linked by edges to the other nodes they are dependent upon. The resulting graph is relatively large, with a node for each instance of each oneMKL call. For example, there are many nodes that call `dtrsm`; one for each invocation of `dtrsm` in the serial implementation. The is very little overhead in message management for this version and so it is often the highest performing.

**join**: A parallel version of Crout-Cholesky factorization that uses the oneTBB flow graph. This version uses a data flow approach. This is a small, compact graph that passes tiles along its edges. There is one node per type of oneMKL call, plus `join_node`s that combine the inputs required for each call. So for example, there is only a single node that applies all calls to `dtrsm`. This node is invoked when the tiles that hold the inputs and outputs for an invocation are matched together in the tag-matching `join_node` that precedes it. The tag represents the iteration values of the `i`, `j`, `k` loops in the serial implementation at that invocation of the call. There is some overhead in message matching and forwarding, so it may not perform as well as the dependency graph implementation.

This sample code requires a oneTBB library and also the oneMKL library.

## Building the example
```
cmake <path_to_example>
cmake --build .
```

## Running the sample
### Predefined make targets
* `make run_cholesky` - executes the example with predefined parameters.

### Application parameters
Usage:
```
cholesky [size=value] [blocksize=value] [num_trials=value] [output_prefix=value] [algorithm=value] [num_tbb_threads=value] [input_file=value] [-x] [-h] [size [blocksize [num_trials [output_prefix [algorithm [num_tbb_threads]]]]]]
```
* `-h` - prints the help for command line options.
* `size` - the row/column size of `NxN` matrix (size <= 46000).
* `blocksize` - the block size; size must be a multiple of the blocksize.
* `num_trials` - the number of times to run each algorithm.
* `output_prefix` - if provided the prefix will be prepended to output files: <output_prefix>_posdef.txt and <output_prefix>_X.txt; where `X` is the algorithm used. If `output_prefix` is not provided, no output will be written.
* `algorithm` - name of the used algorithm - can be dpotrf, crout, depend or join.
* `num_tbb_threads` - number of oneTBB threads.
* `input_file` - input matrix (optional). If omitted, randomly generated values are used.
* `-x` - skips all validation.
