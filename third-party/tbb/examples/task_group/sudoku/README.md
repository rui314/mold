# Sudoku sample
This directory contains an example that finds all solutions to a Sudoku board.

It uses a straightforward state-space search algorithm that exhibits OR-parallelism. It can be optionally run until it obtains just the first solution. The point of the example is to teach how to use the `task_group` interface.

## Building the example
```
cmake <path_to_example>
cmake --build .
```

## Running the sample
### Predefined make targets
* `make run_sudoku` - executes the example with predefined parameters.
* `make perf_run_sudoku` - executes the example with suggested parameters to measure the oneTBB performance.
* `make benchmark_sudoku` - executes the example with suggested parameters to repeat performance measurements several times and report their relative error.
* `make benchmark_sudoku_data` - same as `benchmark_sudoku` saving results into `benchmark_sudoku_data.csv` file.

### Application parameters
Usage:
```
sudoku [n-of-threads=value] [filename=value] [verbose] [silent] [find-one] [-h] [n-of-threads [filename]]
```
* `-h` - prints the help for command line options.
* `n-of-threads` - the number of threads to use; a range of the form low\[:high\], where low and optional high are non-negative integers or `auto` for a platform-specific default number.
* `filename` - the input filename.
* `n-of-repeats` - the number of repeats to find the same solution; when it is greater than 1 then relative error will be calculated for the repeats.
* `verbose` - prints the first solution.
* `silent` - no output except elapsed time and relative errors.
* `find-one` - stops after finding first solution.

The example's directory contains following files that may be used as an input file:
`input1` - Sample input file with modest number of solutions.
`input2` - Sample input file with small number of solutions.
`input3` - Sample input file with larger number of solutions.
`input4` - Sample input file with very large number of solutions.
