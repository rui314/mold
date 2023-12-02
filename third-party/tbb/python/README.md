# Python* API for Intel&reg; oneAPI Threading Building Blocks (oneTBB) .

## Overview
It is a preview Python* module which unlocks opportunities for additional performance in
multi-threaded and multiprocess Python programs by enabling threading composability
between two or more thread-enabled libraries like Numpy, Scipy, Sklearn, Dask, Joblib, and etc.

The biggest improvement can be achieved when a task pool like the ThreadPool or Pool from the Python
standard library or libraries like Dask or Joblib (used either in multi-threading or multi-processing mode)
execute tasks calling compute-intensive functions of Numpy/Scipy/Sklearn/PyDAAL which in turn are
parallelized using Intel&reg; oneAPI Math Kernel Library or/and oneTBB.

The module implements Pool class with the standard interface using oneTBB which can be used to replace Python's ThreadPool.
Thanks to the monkey-patching technique implemented in class Monkey, no source code change is needed in order to enable threading composability in Python programs.

For more information and examples, please refer to [forum discussion](https://community.intel.com/t5/Intel-Distribution-for-Python/TBB-module-Unleash-parallel-performance-of-Python-programs/m-p/1074459).

## Directories
 - **rml** - The folder contains sources for building the plugin with cross-process dynamic thread scheduler implementation.
 - **tbb** - The folder contains Python module sources.

## Files
 - **setup.py** - Standard Python setup script.
 - **TBB.py** - Alternative entry point for Python module.

## CMake predefined targets
 - `irml` - compilation of plugin with cross-process dynamic thread scheduler implementation.
 - `python_build` - building of oneTBB module for Python.

## Command-line interface

 - `python3 -m tbb -h` - Print documentation on command-line interface.
 - `pydoc tbb` - Read built-in documentation for Python interfaces.
 - `python3 -m tbb your_script.py` - Run your_script.py in context of `with tbb.Monkey():` when oneTBB is enabled. By default only multi-threading will be covered.
 - `python3 -m tbb --ipc your_script.py` - Run your_script.py in context of `with tbb.Monkey():` when oneTBB enabled in both multi-threading and multi-processing modes.
 - `python3 setup.py build -b<output_directory_path> -f check` - Build oneTBB module for Python. (Prerequisites: built and sourced oneTBB and IRML libraries)
 - `python3 setup.py build -b<output_directory_path> build_ext -I<path_to_tbb_includes> -L<path_to_prebuilt_libraries> install -f <additional_flags> ` - Build and install oneTBB module for Python. (Prerequisites: built oneTBB and IRML libraries)
 - `python3 -m TBB test` - run test for oneTBB module for Python.
 - `python3 -m tbb test` - run test for oneTBB module for Python.

## System Requirements
 - The Python module was not tested on older versions of Python thus we require at least Python and 3.5 or higher.
 - SWIG must be of version 3.0.6 or higher.
