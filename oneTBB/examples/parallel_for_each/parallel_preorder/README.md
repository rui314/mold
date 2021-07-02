# Parallel_preorder sample
Example that uses `parallel_for_each` to do parallel preorder traversal of a sparse graph.

Each vertex in the graph is called a "cell". Each cell has a value. The value is a matrix. Some of the cells have operators that compute the cell's value, using other cell's values as input. A cell that uses the value of cell `x` is called a successor of `x`.

The algorithm works as follows.

1. Compute the set of cells that have no inputs. This set is called `root_set`.
2. Each cell has an associated field `ref_count` that is an atomic integer. Initialize `ref_count` to the number of inputs for the `Cell`.
3. Update each cell in `root_set`, by applying a `parallel_for_each` to a `root_set`.
4. After updating a cell, for each of its successors
    1. Atomically decrement the successor's ref_count
    2. If the count became zero, add the cell to the set of cells to be updated, by calling `feeder::add`.

The times printed are for the traversal and update, and do not include time for computing the `root_set`.

The example is using custom synchronization via ref_count atomic variable. Correctness checking tools might not take this into account, and report data races between different tasks that are actually synchronized.

**Note:** It is important to understand that this example is unlikely to show speedup if the cell values are changed to type "float". The reason is twofold.
* The smaller value type causes each `Cell` to be significantly smaller than a cache line, which leads to false sharing conflicts.
* The time to update the cells becomes very small, and consequently the overhead of `parallel_for_each` swamps the useful work.

## Building the example
```
cmake <path_to_example>
cmake --build .
```

## Running the sample
### Predefined make targets
* `make run_parallel_preorder` - executes the example with predefined parameters
* `make perf_run_parallel_preorder` - executes the example with suggested parameters to measure the oneTBB performance
* `make light_test_parallel_preorder` - executes the example with suggested parameters to reduce execution time.

### Application parameters
Usage:
```
parallel_preorder [n-of-threads=value] [n-of-nodes=value] [n-of-traversals=value] [silent] [-h] [n-of-threads [n-of-nodes [n-of-traversals]]]
```
* `-h` - prints the help for command line options.
* `n-of-threads` - the number of threads to use; a range of the form low\[:high\], where low and optional high are non-negative integers or `auto` for a platform-specific default number.
* `n-of-nodes` - the number of nodes in the graph. Default value is 1000.
* `n-of-traversals` - the number of times to evaluate the graph. Default value is 500.
* `silent` - no output except elapsed time.
