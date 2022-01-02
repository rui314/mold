.. _Throughput_of_pipeline:

Throughput of pipeline
======================


The throughput of a pipeline is the rate at which tokens flow through
it, and is limited by two constraints. First, if a pipeline is run with
``N`` tokens, then obviously there cannot be more than ``N`` operations
running in parallel. Selecting the right value of ``N`` may involve some
experimentation. Too low a value limits parallelism; too high a value
may demand too many resources (for example, more buffers). Second, the
throughput of a pipeline is limited by the throughput of the slowest
sequential filter. This is true even for a pipeline with no parallel
filters. No matter how fast the other filters are, the slowest
sequential filter is the bottleneck. So in general you should try to
keep the sequential filters fast, and when possible, shift work to the
parallel filters.


The text processing example has relatively poor speedup, because the
serial filters are limited by the I/O speed of the system. Indeed, even
with files that are on a local disk, you are unlikely to see a speedup
much more than 2. To really benefit from a pipeline, the parallel
filters need to be doing some heavy lifting compared to the serial
filters.


The window size, or sub-problem size for each token, can also limit
throughput. Making windows too small may cause overheads to dominate the
useful work. Making windows too large may cause them to spill out of
cache. A good guideline is to try for a large window size that still
fits in cache. You may have to experiment a bit to find a good window
size.

