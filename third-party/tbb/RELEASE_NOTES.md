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
- [Preview Features](#preview-features)
- [Known Limitations](#known-limitations)
- [Issues Fixed](#issues-fixed)
- [Open-Source Contributions Integrated](#open-source-contributions-integrated)

## :tada: Preview Features
- Extended the Flow Graph receiving nodes with a new ``try_put_and_wait`` API that submits a message to the graph and waits for its completion.

## :rotating_light: Known Limitations
- The ``oneapi::tbb::info`` namespace interfaces might unexpectedly change the process affinity mask on Windows* OS systems (see https://github.com/open-mpi/hwloc/issues/366 for details) when using hwloc version lower than 2.5.
- Using a hwloc version other than 1.11, 2.0, or 2.5 may cause an undefined behavior on Windows OS. See https://github.com/open-mpi/hwloc/issues/477 for details.
- The NUMA topology may be detected incorrectly on Windows* OS machines where the number of NUMA node threads exceeds the size of 1 processor group.
- On Windows OS on ARM64*, when compiling an application using oneTBB with the Microsoft* Compiler, the compiler issues a warning C4324 that a structure was padded due to the alignment specifier. Consider suppressing the warning by specifying /wd4324 to the compiler command line.
- C++ exception handling mechanism on Windows* OS on ARM64* might corrupt memory if an exception is thrown from any oneTBB parallel algorithm (see Windows* OS on ARM64* compiler issue: https://developercommunity.visualstudio.com/t/ARM64-incorrect-stack-unwinding-for-alig/1544293.
- When CPU resource coordination is enabled, tasks from a lower-priority ``task_arena`` might be executed before tasks from a higher-priority ``task_arena``.
- Using oneTBB on WASM*, may cause applications to run in a single thread. See [Limitations of WASM Support](https://github.com/uxlfoundation/oneTBB/blob/master/WASM_Support.md#limitations).

> **_NOTE:_**  To see known limitations that impact all versions of oneTBB, refer to [oneTBB Documentation](https://uxlfoundation.github.io/oneTBB/main/intro/limitations.html).


## :hammer: Issues Fixed
- Fixed the missed signal for thread request for enqueue operation.
- Significantly improved scalability of ``task_group``, ``flow_graph``, and ``parallel_for_each``.
- Removed usage of ``std::aligned_storage`` deprecated in C++23 (Inspired by Valery Matskevich https://github.com/uxlfoundation/oneTBB/pull/1394).
- Fixed the issue where ``oneapi::tbb::info`` interfaces might interfere with the process affinity mask on the Windows* OS systems with multiple processor groups.


## :octocat: Open-Source Contributions Integrated
- Detect the GNU Binutils version to determine WAITPKG support better. Contributed by Martijn Courteaux (https://github.com/uxlfoundation/oneTBB/pull/1347).
- Fixed the build on non-English locales. Contributed by Vladislav Shchapov (https://github.com/uxlfoundation/oneTBB/pull/1450). 
- Improved Bazel support. Contributed by Julian Amann (https://github.com/uxlfoundation/oneTBB/pull/1434).
