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
- [New Features](#new_features)
- [Known Limitations](#known-limitations)
- [Fixed Issues](#fixed-issues)
- [Open-source Contributions Integrated](#open-source-contributions-integrated)

## :white_check_mark: New Features
- Improved support and use of the latest C++ standards for parallel_sort that allows using this algorithm with user-defined and standard library-defined objects with modern semantics.
- The following features are now fully functional: task_arena extensions, collaborative_call_once, adaptive mutexes, heterogeneous overloads for concurrent_hash_map, and task_scheduler_handle.
- Added support for Windows* Server 2022 and Python 3.10.

## :rotating_light: Known Limitations
- An application using Parallel STL algorithms in libstdc++ versions 9 and 10 may fail to compile due to incompatible interface changes between earlier versions of Threading Building Blocks (TBB) and oneAPI Threading Building Blocks (oneTBB). Disable support for Parallel STL algorithms by defining PSTL_USE_PARALLEL_POLICIES (in libstdc++ 9) or _GLIBCXX_USE_TBB_PAR_BACKEND (in libstdc++ 10) macro to zero before inclusion of the first standard header file in each translation unit.
- On Linux* OS, if oneAPI Threading Building Blocks (oneTBB) or Threading Building Blocks (TBB) are installed in a system folder like /usr/lib64, the application may fail to link due to the order in which the linker searches for libraries. Use the -L linker option to specify the correct location of oneTBB library. This issue does not affect the program execution.
- The oneapi::tbb::info namespace interfaces might unexpectedly change the process affinity mask on Windows* OS systems (see https://github.com/open-mpi/hwloc/issues/366 for details) when using hwloc version lower than 2.5.
- Using a hwloc version other than 1.11, 2.0, or 2.5 may cause an undefined behavior on Windows OS. See https://github.com/open-mpi/hwloc/issues/477 for details.
- The NUMA topology may be detected incorrectly on Windows OS machines where the number of NUMA node threads exceeds the size of 1 processor group.
- On Windows OS on ARM64*, when compiling an application using oneTBB with the Microsoft* Compiler, the compiler issues a warning C4324 that a structure was padded due to the alignment specifier. Consider suppressing the warning by specifying /wd4324 to the compiler command line.
- oneTBB does not support fork(), to work-around the issue, consider using task_scheduler_handle to join oneTBB worker threads before using fork().
- C++ exception handling mechanism on Windows* OS on ARM64* might corrupt memory if an exception is thrown from any oneTBB parallel algorithm (see Windows* OS on ARM64* compiler issue: https://developercommunity.visualstudio.com/t/ARM64-incorrect-stack-unwinding-for-alig/1544293).

## :hammer: Fixed Issues
- Memory allocator crash on a system with an incomplete /proc/meminfo (GitHub* [#584](https://github.com/oneapi-src/oneTBB/issues/584)).
- Incorrect blocking of task stealing (GitHub* #[478](https://github.com/oneapi-src/oneTBB/issues/478)).
- Hang due to incorrect decrement of a limiter_node (GitHub* [#634](https://github.com/oneapi-src/oneTBB/issues/634)).
- Memory corruption in some rare cases when passing big messages in a flow graph (GitHub* [#639](https://github.com/oneapi-src/oneTBB/issues/639)).
- Possible deadlock in a throwable flow graph node with a lightweight policy. The lightweight policy is now ignored for functors that can throw exceptions (GitHub* [#420](https://github.com/oneapi-src/oneTBB/issues/420)).
- Crash when obtaining a range from empty ordered and unordered containers (GitHub* [#641](https://github.com/oneapi-src/oneTBB/issues/641)).
- Deadlock in a concurrent_vector resize() that could happen when the new size is less than the previous size (GitHub* [#733](https://github.com/oneapi-src/oneTBB/issues/733)).

## :octocat: Open-source Contributions Integrated
- Improved aligned memory allocation. Contributed by Andrey Semashev (https://github.com/oneapi-src/oneTBB/pull/671).
- Optimized usage of atomic_fence on IA-32 and Intel(R) 64 architectures. Contributed by Andrey Semashev (https://github.com/oneapi-src/oneTBB/pull/328).
- Fixed incorrect definition of the assignment operator in containers. Contributed by Andrey Semashev (https://github.com/oneapi-src/oneTBB/issues/372).
