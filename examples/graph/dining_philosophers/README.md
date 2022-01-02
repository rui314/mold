# Dining_philosophers sample
The Dining Philosophers problem demonstrates `oneapi::tbb::flow` and the use of the reserving `join_node` to solve the potential deadlock.

This program runs some number of philosophers in parallel, each thinking and then waiting for chopsticks to be available before eating. Eating and thinking are implemented with `sleep()`. The chopstick positions are represented by a `queue_node` with one item.

## Building the example
```
cmake <path_to_example>
cmake --build .
```

## Running the sample
### Predefined make targets
* `make run_dining_philosophers` - executes the example with predefined parameters.
* `make light_test_dining_philosophers` -  executes the example with suggested parameters to reduce execution time.

### Application parameters
Usage:
```
dining_philosophers [n-of_threads=value] [n-of-philosophers=value] [verbose] [-h] [n-of_threads [n-of-philosophers]]
```
* `-h` - prints the help for command line options.
* `n-of_threads` - number of threads to use; a range of the form low\[:high\[:(+|*|#)step\]\], where low and optional high are non-negative integers or 'auto' for the default choice, and optional step expression specifies how thread numbers are chosen within the range.
* `n-of-philosophers` - how many philosophers, from 2-26.
* `verbose` - prints diagnostic output to screen.
