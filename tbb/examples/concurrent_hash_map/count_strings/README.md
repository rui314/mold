# Count_strings sample
The example counts the number of unique words in a text.

## Building the example
```
cmake <path_to_example>
cmake --build .
```

## Running the sample
### Predefined make targets
* `make run_count_strings` - executes the example with predefined parameters.
* `make perf_run_count_strings` - executes the example with suggested parameters to measure the oneTBB performance.

### Application parameters
Usage:
```
count_strings [n-of-threads=value] [n-of-strings=value] [verbose] [silent] [count_collisions] [-h] [n-of-threads [n-of-strings]]
```
* `-h` - prints the help for command line options.
* `n-of-threads` - number of threads to use; a range of the form low\[:high\], where low and optional high are non-negative integers or `auto` for a platform-specific default number.
* `n-of-strings` - number of strings.
* `verbose` - prints diagnostic output to screen.
* `silent` - no output except elapsed time.
