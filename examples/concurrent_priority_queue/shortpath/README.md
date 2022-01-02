# Shortpath sample
This directory contains an example that solves the single source shortest path problem.

It is parameterized by `N`, a number of nodes, and a start and end node in `[0..N)`. A graph is generated with `N` nodes and some random number of connections between those nodes. A parallel algorithm based on `A*` is used to find the shortest path.

This algorithm varies from serial `A*` in that it needs to add nodes back to the open set when the `g` estimate (shortest path from start to the node) is improved, even if the node has already been "visited". This is because nodes are added and removed from the open-set in parallel, resulting in some less optimal paths being explored. The open-set is implemented with the `concurrent_priority_queue`.

Note that since we re-visit nodes, the `f` estimate (on which the priority queue is sorted) is not technically needed, so we could use this same parallel algorithm with just a `concurrent_queue`. However, keeping the `f` estimate and using `concurrent_priority_queue` results in much better performance.

Silent mode prints run time only, regular mode prints the shortest path length, and verbose mode prints out the shortest path.

The generated graph follows a pattern in which the closer two pairs of node ids are together, the fewer hops there are in a typical path between those nodes. So, for example, the path between 5 and 7 likely has few hops whereas 14 to 78 has more and 0 to 9999 has even more, etc.

## Building the example
```
cmake <path_to_example>
cmake --build .
```

## Running the sample
### Predefined make targets
* `make run_shortpath` - executes the example with predefined parameters.
* `make perf_run_shortpath` - executes the example with suggested parameters to measure the oneTBB performance.

### Application parameters
Usage:
```
shortpath [#threads=value] [verbose] [silent] [N=value] [start=value] [end=value] [-h] [#threads]
```
* `-h` - prints the help for command line options.
* `n-of-threads` - number of threads to use; a range of the form low[:high], where low and optional high are non-negative integers or `auto` for a platform-specific default number.
* `verbose` - prints diagnostic output to screen.
* `silent` - no output except elapsed time.
* `N` - number of nodes in graph.
* `start` - node to start path at.
* `end` - node to end path at.
