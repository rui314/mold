<!--
******************************************************************************
* 
* Licensed under the Apache License, Version 2.0 (the "License");
* you may not use this file except in compliance with the License.
* You may obtain a copy of the License at
*
*     http://www.apache.org/licenses/LICENSE-2.0
*
* Unless required by applicable law or agreed to in writing, software
* distributed under the License is distributed on an "AS IS" BASIS,
* WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
* See the License for the specific language governing permissions and
* limitations under the License.
*******************************************************************************/-->

# Release Notes <!-- omit in toc -->
This document contains changes of oneTBB compared to the last release.

## Table of Contents <!-- omit in toc -->
- [Known Limitations](#known-limitations)
- [Fixed Issues](#fixed-issues)

## :rotating_light: Known Limitations
- The ``oneapi::tbb::info`` namespace interfaces might unexpectedly change the process affinity mask on Windows* OS systems (see https://github.com/open-mpi/hwloc/issues/366 for details) when using hwloc version lower than 2.5.
- Using a hwloc version other than 1.11, 2.0, or 2.5 may cause an undefined behavior on Windows OS. See https://github.com/open-mpi/hwloc/issues/477 for details.
- The NUMA topology may be detected incorrectly on Windows* OS machines where the number of NUMA node threads exceeds the size of 1 processor group.
- On Windows OS on ARM64*, when compiling an application using oneTBB with the Microsoft* Compiler, the compiler issues a warning C4324 that a structure was padded due to the alignment specifier. Consider suppressing the warning by specifying /wd4324 to the compiler command line.
- C++ exception handling mechanism on Windows* OS on ARM64* might corrupt memory if an exception is thrown from any oneTBB parallel algorithm (see Windows* OS on ARM64* compiler issue: https://developercommunity.visualstudio.com/t/ARM64-incorrect-stack-unwinding-for-alig/1544293).
- Using ``TBBConfig.cmake`` in 32-bit environment may cause incorrect linkage with 64-bit oneTBB library. As a workaround, set ``CMAKE_PREFIX_PATH``:
  - On Linux* OS: to ``TBBROOT/lib32/``
  - On Windows* OS: to ``TBBROOT/lib32/;TBBROOT/bin32/``

> **_NOTE:_**  To see known limitations that impact all versions of oneTBB, refer to [oneTBB Documentation](https://oneapi-src.github.io/oneTBB/main/intro/limitations.html).


## :hammer: Fixed Issues
- Fixed ``tbb::this_task_arena()`` behavior for specific ``tbb::task_arena{1,0}``.
- Restored performance on systems with a high number of CPU cores that support ``_tpause``.
