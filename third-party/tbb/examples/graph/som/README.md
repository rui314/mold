# Self-Organizing Map (SOM) sample
The Self-Organizing Map demonstrates `oneapi::tbb::flow` and the use of cancellation in scheduling multiple iterations of map updates.

For tutorials on Self-organizing Maps, see [here](http://www.ai-junkie.com/ann/som/som1.html) and [here](http://davis.wpi.edu/~matt/courses/soms/).

The program trains the map with several examples, splitting the map into subsections and looking for best-match for multiple examples. When an example is used to update the map, the graphs examining the sections being updated for the next example are cancelled and restarted after the update.

## Building the example
```
cmake <path_to_example>
cmake --build .
```

## Running the sample
### Predefined make targets
* `make run_som` - executes the example with predefined parameters.
* `make light_test_som` - executes the example with suggested parameters to reduce execution time.

### Application parameters
Usage:
```
som [n-of-threads=value] [radius-fraction=value] [number-of-epochs=value] [cancel-test] [debug] [nospeculate] [-h] [n-of-threads [radius-fraction [number-of-epochs]]]
```
* `-h` - prints the help for command line options.
* `n-of-threads` - number of threads to use; a range of the form low\[:high\], where low and optional high are non-negative integers or `auto` for the oneTBB default.
* `radius-fraction` - size of radius at which to start speculating.
* `number-of-epochs` - number of examples used in learning phase.
* `cancel-test` - test for cancel signal while finding BMU.
* `debug` - additional output.
* `nospeculate` - don't speculate in SOM map teaching.
