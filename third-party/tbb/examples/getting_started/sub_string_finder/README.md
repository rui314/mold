# Sub_string_finder sample
An example that uses the `parallel_for` template in a substring matching program. The [oneAPI Threading Building Blocks [](https://software.intel.com/content/www/us/en/develop/documentation/get-started-with-onetbb/top.html) describes this example.

For each position in a string, the program displays the length of the largest matching substring elsewhere in the string. The program also displays the location of a largest match for each position. Consider the string "babba" as an example. Starting at position 0, "ba" is the largest substring with a match elsewhere in the string (position 3).

## Building the example
```
cmake <path_to_example>
cmake --build .
```

### Predefined make targets
* `make sub_string_finder_simple` - builds the example as it appears in the Get Started Guide.
* `make sub_string_finder_extended` - builds the similar example with more attractive printing of the results.
* `make sub_string_finder_pretty` - builds the example extended with a sequential implementation.
* `make sub_string_finder` - builds all sample versions.

## Running the sample
### Predefined make targets
* `make run_sub_string_finder` - executes the example with predefined parameters.
* `make light_test_sub_string_finder` - executes the example with suggested parameters to reduce execution time.

### Application parameters
Usage:
```
sub_string_finder_simple
sub_string_finder_extended
sub_string_finder_pretty
```

The example does not requires application parameters.
