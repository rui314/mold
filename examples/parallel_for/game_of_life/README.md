# Game_of_life sample
The "Game of life" example demonstrates interoperability of oneAPI Threading Building Blocks (oneTBB) and Microsoft* .NET*.

This program runs 2 simultaneous instances of the classic Conway's "Game of Life". One of these instances uses serial calculations to update the board. The other one calculates in parallel with oneTBB. The visualization is written in managed C++ and uses .NET CLR.

## Building the example
```
cmake <path_to_example>
cmake --build .
```

## Running the sample
### Predefined make targets
* `make run_game_of_life` - executes the example with predefined parameters.
* `make light_test_game_of_life` - executes the example with suggested parameters to reduce execution time.

### Application parameters
Usage:
```
game_of_life [M[:N] -t execution_time] [-h]
```
* `-h` - prints the help for command line options.
* `M:N` - range of numbers of threads to be used.
* `execution_time` - time (in sec) for execution `game_of_life` iterations.
