# Square sample
Text filter that demonstrates the use of `parallel_pipeline`. Example program reads a file containing decimal integers in text format, and changes each to its square.

## Building the example
```
cmake <path_to_example>
cmake --build .
```

## Running the sample
### Predefined make targets
* `make run_square` - executes the example with predefined parameters
* `make perf_run_square` - executes the example with suggested parameters to measure the oneTBB performance
* `make light_test_square` - executes the example with suggested parameters to reduce execution time.

### Application parameters
Usage:
```
square [n-of-threads=value] [input-file=value] [output-file=value] [max-slice-size=value] [silent] [-h] [n-of-threads [input-file [output-file [max-slice-size]]]]
```
* `-h` - prints the help for command line options.
* `n-of-threads` - the number of threads to use; a range of the form low\[:high\], where low and optional high are non-negative integers or `auto` for a platform-specific default number.
* `input`- file is an input file name.
* `output`- file is an output file name.
* `max-slice-size` - the maximum number of characters in one slice.
* `silent` - no output except elapsed time.
