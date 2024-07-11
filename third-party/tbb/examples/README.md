# Code Samples of oneAPI Threading Building Blocks (oneTBB)
This directory contains example usages of oneAPI Threading Building Blocks.

| Code sample name | Description
|:--- |:---
| getting_started/sub_string_finder | Example referenced by the [oneAPI Threading Building Blocks Get Started Guide](https://oneapi-src.github.io/oneTBB/GSG/get_started.html#get-started-guide). Finds the largest matching substrings.
| concurrent_hash_map/count_strings | Concurrently inserts strings into a `concurrent_hash_map` container.
| concurrent_priority_queue/shortpath | Solves the single source shortest path problem using a  `concurrent_priority_queue` container.
| graph/binpack | A solution to the binpacking problem using a `queue_node`, a `buffer_node`, and `function_node`s.
| graph/cholesky | Several versions of Cholesky Factorization algorithm implementation.
| graph/dining_philosophers | An implementation of dining philosophers in a graph using the reserving `join_node`.
| graph/fgbzip2 | A parallel implementation of bzip2 block-sorting file compressor.
| graph/logic_sim | An example of a collection of digital logic gates that can be easily composed into larger circuits.
| graph/som | An example of a Kohonen Self-Organizing Map using cancellation.
| parallel_for/game_of_life | Game of life overlay.
| parallel_for/polygon_overlay | Polygon overlay.
| parallel_for/seismic | Parallel seismic wave simulation.
| parallel_for/tachyon | Parallel 2-D raytracer/renderer.
| parallel_for_each/parallel_preorder | Parallel preorder traversal of a graph.
| parallel_pipeline/square | Another string transformation example that squares numbers read from a file.
| parallel_reduce/convex_hull | Parallel version of convex hull algorithm (quick hull).
| parallel_reduce/pi | Parallel version of calculating &pi; by numerical integration.
| parallel_reduce/primes | Parallel version of the Sieve of Eratosthenes.
| task_arena/fractal |The example calculates two classical Mandelbrot fractals with different concurrency limits.
| task_group/sudoku | Compute all solutions for a Sudoku board.
| test_all/fibonacci | Compute Fibonacci numbers in different ways.

## System Requirements
Refer to the [System Requirements](https://github.com/oneapi-src/oneTBB/blob/master/SYSTEM_REQUIREMENTS.md) for the list of supported hardware and software.

### Graphical User Interface (GUI)
Some examples (e.g., fractal, seismic, tachyon, polygon_overlay) support different GUI modes, which may be defined via the `EXAMPLES_UI_MODE` CMake variable. 
Supported values are:
- Cross-platform:
    - `con` - Console mode (Default).
- Windows* OS:
    - `gdi` - `GDI+` based implementation.
    - `d2d` - `Direct 2D` based implementation. May offer superior performance but can only be used if the Microsoft* DirectX* SDK is installed on your system(`DXSDK_DIR` should be defined).
- Linux* OS:
    - `x` - `X11` based implementation. Also `libXext` may be required to display the output correctly.
- macOS*:
    - `mac` - `OpenGL` based implementation. Also requires the `Foundation` and `Cocoa` libraries availability.
