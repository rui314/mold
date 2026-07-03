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

# oneTBB 2023.0 Release Notes

## :rocket: Preview Features
- Introduced ability to wait for a single task in a ``task_group`` instead of waiting for all tasks to finish. This increases reactivity and decreases latency in key user workloads.
- Introduced ``flow::resource_limited_node`` and ``flow::resource_limiter`` classes. These nodes only execute when they can successfully acquire the necessary resources from the resource limiters associated with the node. This feature is used to guard access to shared resources, while maximizing available parallelism in the graph.
- Introduced ``task_arena`` core type selector to better support hybrid architectures with several core types. Users use this new flexible API to more tightly constrain execution to set preferences for the specific core types that match their workload.
- Added global control parameter to set default block time behavior on server HW. This allows developers to revert to older blocking behavior if their applications are not able to fully utilize all cores, thereby reducing idle spinning.


## :tada: New Features
- Added new API to create a set of NUMA bound task arenas, simplifying common patterns used to optimize for NUMA architectures.
- Extended Flow Graph functional node deduction guides to support non-static member function and member object pointers as a node bodies.
- The Flow Graph join_node and indexer_node now support 10 or more input ports.
- Explicit deduction guides for ``blocked_nd_range`` are now a fully supported feature.
- Added native WASM exception handling support.
- Added support for Python 3.14.


## :rotating_light: Known Limitations
- The ``oneapi::tbb::info`` namespace interfaces might unexpectedly change the process affinity mask on Windows* OS systems (see https://github.com/open-mpi/hwloc/issues/366 for details) when using hwloc version lower than 2.5.
- Using a hwloc version other than 1.11, 2.0, or 2.5 may cause an undefined behavior on Windows OS. See https://github.com/open-mpi/hwloc/issues/477 for details.
- The NUMA topology may be detected incorrectly on Windows* OS machines where the number of NUMA node threads exceeds the size of 1 processor group.
- On Windows OS on ARM64*, when compiling an application using oneTBB with the Microsoft* Compiler, the compiler issues a warning C4324 that a structure was padded due to the alignment specifier. Consider suppressing the warning by specifying /wd4324 to the compiler command line.
- When CPU resource coordination is enabled by setting the TCM_ENABLE environment variable to 1, tasks from a lower-priority ``task_arena`` might be executed before tasks from a higher-priority ``task_arena``.
- Using oneTBB on WASM* may cause applications to run in a single thread. See [Limitations of WASM Support](https://github.com/uxlfoundation/oneTBB/blob/master/WASM_Support.md#limitations).

> **_NOTE:_**  To see known limitations that impact all versions of oneTBB, refer to [oneTBB Documentation](https://uxlfoundation.github.io/oneTBB/main/intro/limitations.html).


## :hammer: Issues Fixed
- Significantly improved scalability for concurrent ordered containers on systems with many threads. This change results in greater than 3x performance for some use cases.
- Fixed ODR violations when public inline functions expose entities with internal linkage.

## :octocat: Open-Source Contributions Integrated
- Updated tbb4py installation approach to be aligned with PEP517. Contributed by Ben Greiner (https://github.com/uxlfoundation/oneTBB/pull/1941).
- Improved usage of <windows.h> in oneTBB code. Contributed by Aras Pranckevi&#269;ius (https://github.com/uxlfoundation/oneTBB/pull/1932).
- Fixed CMake build issue with Clang on Windows*. Contributed by JustinZhu (https://github.com/uxlfoundation/oneTBB/pull/1936).


# oneTBB 2022.3 Release Notes

## :rocket: Preview Features
- Introduced API for setting dynamic task dependencies in task_group. This allows successor tasks to execute only after all their predecessors have completed.


## :tada: New Features
- Extended task_arena with API support for enqueuing functions into a task_group and waiting for the task_group to complete.
- Introduced API for setting and getting the assertion handler. This allows applications to set their own assertion handling functions.


## :rotating_light: Known Limitations
- The ``oneapi::tbb::info`` namespace interfaces might unexpectedly change the process affinity mask on Windows* OS systems (see https://github.com/open-mpi/hwloc/issues/366 for details) when using hwloc version lower than 2.5.
- Using a hwloc version other than 1.11, 2.0, or 2.5 may cause an undefined behavior on Windows OS. See https://github.com/open-mpi/hwloc/issues/477 for details.
- The NUMA topology may be detected incorrectly on Windows* OS machines where the number of NUMA node threads exceeds the size of 1 processor group.
- On Windows OS on ARM64*, when compiling an application using oneTBB with the Microsoft* Compiler, the compiler issues a warning C4324 that a structure was padded due to the alignment specifier. Consider suppressing the warning by specifying /wd4324 to the compiler command line.
- C++ exception handling mechanism on Windows* OS on ARM64* might corrupt memory if an exception is thrown from any oneTBB parallel algorithm (see Windows* OS on ARM64* compiler issue: https://developercommunity.visualstudio.com/t/ARM64-incorrect-stack-unwinding-for-alig/1544293.
- When CPU resource coordination is enabled by setting the TCM_ENABLE environment variable to 1, tasks from a lower-priority ``task_arena`` might be executed before tasks from a higher-priority ``task_arena``.
- When CPU resource coordination is enabled, cgroups CPU quota and period limitations settings are ignored.
- Using oneTBB on WASM* may cause applications to run in a single thread. See [Limitations of WASM Support](https://github.com/uxlfoundation/oneTBB/blob/master/WASM_Support.md#limitations).

> **_NOTE:_**  To see known limitations that impact all versions of oneTBB, refer to [oneTBB Documentation](https://uxlfoundation.github.io/oneTBB/main/intro/limitations.html).


## :hammer: Issues Fixed
- Fixed overwrite_node and write_once_node to prevent premature trigger of continue_node successors.
- Fixed keys duplication in concurrent_hash_map when the container is initialized using a pair of iterators, or std::initializer_list (https://github.com/uxlfoundation/oneTBB/issues/1764).
- Improved support for profiling tools by allowing the oneTBB notifications to be ignored by tools and by better timing of task completion notifications.
- Fixed incorrect deallocation of tasks in task_group (https://github.com/uxlfoundation/oneTBB/issues/1834).
- Improved performance scalability of spin_mutex::lock, spin_mutex::try_lock and queuing_mutex::scoped_lock::try_acquire.
- Fixed potential oversubscription issue by respecting CPU quota and period limitations provided via cgroups settings on Linux* (https://github.com/uxlfoundation/oneTBB/issues/190, https://github.com/uxlfoundation/oneTBB/issues/1760).

## :octocat: Open-Source Contributions Integrated
- Fixed CMake build issue on some architectures. Contributed by lmarz (https://github.com/uxlfoundation/oneTBB/pull/1768).
- Fixed stack size assertion failure when running with ASAN. Contributed by omegacoleman (https://github.com/uxlfoundation/oneTBB/pull/1782).
- Improved Bazel support on non-x86 architectures. Contributed by Caleb Zulawski (https://github.com/uxlfoundation/oneTBB/pull/1790).
- Fixed potential crash caused by nullptr dereference for the tag_matching join_node. Contributed by Federico Ficarelli (https://github.com/uxlfoundation/oneTBB/pull/1800).
- Fixed false positive GCC* warnings. Contributed by Zizheng Guo  (https://github.com/uxlfoundation/oneTBB/pull/1752).
- Fixed ASAN detection for Clang*. Contributed by Federico Ficarelli (https://github.com/uxlfoundation/oneTBB/pull/1842).


# oneTBB 2022.2 Release Notes

## :tada: New Features
- Improved Hybrid CPU and NUMA Platforms API Support: Enhanced API availability for better compatibility with Hybrid CPU and NUMA platforms.
- Added support for verifying signatures of dynamic dependencies at runtime. To enable this feature, specify
``-DTBB_VERIFY_DEPENDENCY_SIGNATURE=ON`` when invoking CMake.
- Added support for printing warning messages about issues in dynamic dependency loading. To see these messages in the console, build the library with the ``TBB_DYNAMIC_LINK_WARNING`` macro defined.
- Added a Natvis file for custom visualization of TBB containers when debugging with Microsoft* Visual Studio.
- Refined Environment Setup: Replaced CPATH with ``C_INCLUDE_PATH and CPLUS_INCLUDE_PATH`` in environment setup to avoid unintended compiler warnings caused by globally applied include paths. 


## :rotating_light: Known Limitations
- The ``oneapi::tbb::info`` namespace interfaces might unexpectedly change the process affinity mask on Windows* OS systems (see https://github.com/open-mpi/hwloc/issues/366 for details) when using hwloc version lower than 2.5.
- Using a hwloc version other than 1.11, 2.0, or 2.5 may cause an undefined behavior on Windows OS. See https://github.com/open-mpi/hwloc/issues/477 for details.
- The NUMA topology may be detected incorrectly on Windows* OS machines where the number of NUMA node threads exceeds the size of 1 processor group.
- On Windows OS on ARM64*, when compiling an application using oneTBB with the Microsoft* Compiler, the compiler issues a warning C4324 that a structure was padded due to the alignment specifier. Consider suppressing the warning by specifying /wd4324 to the compiler command line.
- C++ exception handling mechanism on Windows* OS on ARM64* might corrupt memory if an exception is thrown from any oneTBB parallel algorithm (see Windows* OS on ARM64* compiler issue: https://developercommunity.visualstudio.com/t/ARM64-incorrect-stack-unwinding-for-alig/1544293.
- When CPU resource coordination is enabled, tasks from a lower-priority ``task_arena`` might be executed before tasks from a higher-priority ``task_arena``.
- Using oneTBB on WASM* may cause applications to run in a single thread. See [Limitations of WASM Support](https://github.com/uxlfoundation/oneTBB/blob/master/WASM_Support.md#limitations).

> **_NOTE:_**  To see known limitations that impact all versions of oneTBB, refer to [oneTBB Documentation](https://uxlfoundation.github.io/oneTBB/main/intro/limitations.html).


## :octocat: Open-Source Contributions Integrated
- Fixed a CMake configuration error on systems with non-English locales. Contributed by moritz-h (https://github.com/uxlfoundation/oneTBB/pull/1606).
- Made the install destination of import libraries on Windows* configurable. Contributed by Bora Yalçıner (https://github.com/uxlfoundation/oneTBB/pull/1613).
- Resolved an in-source CMake build error. Contributed by Dmitrii Golovanov (https://github.com/uxlfoundation/oneTBB/pull/1670).
- Migrated the build system to Bazel* version 8.1.1. Contributed by Julian Amann (https://github.com/uxlfoundation/oneTBB/pull/1694).
- Fixed build errors on MinGW* and FreeBSD*. Contributed by John Ericson (https://github.com/uxlfoundation/oneTBB/pull/1696).
- Addressed build errors on macOS* when using the GCC compiler. Contributed by Oleg Butakov (https://github.com/uxlfoundation/oneTBB/pull/1603).

# oneTBB 2022.1 Release Notes

## :tada: New Features
- The oneTBB repository migrated to the new [UXL Foundation](https://github.com/uxlfoundation/oneTBB) organization.
- ``blocked_nd_range`` is now a fully supported feature.
- Introduced the ``ONETBB_SPEC_VERSION`` macro to specify the version of oneAPI specification implemented by the current version of the library.


## :rocket: Preview Features
- Added the explicit deduction guides to ``blocked_nd_range`` to support C++17 Class Template Argument Deduction.
- Extended ``task_arena`` API to select TBB workers leave policy and to hint the start and the end of parallel computations.


## :rotating_light: Known Limitations
- The ``oneapi::tbb::info`` namespace interfaces might unexpectedly change the process affinity mask on Windows* OS systems (see https://github.com/open-mpi/hwloc/issues/366 for details) when using hwloc version lower than 2.5.
- Using a hwloc version other than 1.11, 2.0, or 2.5 may cause an undefined behavior on Windows OS. See https://github.com/open-mpi/hwloc/issues/477 for details.
- The NUMA topology may be detected incorrectly on Windows* OS machines where the number of NUMA node threads exceeds the size of 1 processor group.
- On Windows OS on ARM64*, when compiling an application using oneTBB with the Microsoft* Compiler, the compiler issues a warning C4324 that a structure was padded due to the alignment specifier. Consider suppressing the warning by specifying /wd4324 to the compiler command line.
- C++ exception handling mechanism on Windows* OS on ARM64* might corrupt memory if an exception is thrown from any oneTBB parallel algorithm (see Windows* OS on ARM64* compiler issue: https://developercommunity.visualstudio.com/t/ARM64-incorrect-stack-unwinding-for-alig/1544293.
- When CPU resource coordination is enabled, tasks from a lower-priority ``task_arena`` might be executed before tasks from a higher-priority ``task_arena``.
- Using oneTBB on WASM* may cause applications to run in a single thread. See [Limitations of WASM Support](https://github.com/uxlfoundation/oneTBB/blob/master/WASM_Support.md#limitations).

> **_NOTE:_**  To see known limitations that impact all versions of oneTBB, refer to [oneTBB Documentation](https://uxlfoundation.github.io/oneTBB/main/intro/limitations.html).


## :hammer: Issues Fixed
- Fixed deadlock when using `tbb::concurrent_vector::grow_by()` (https://github.com/uxlfoundation/oneTBB/issues/1531).
- Fixed assertion in the Debug version of oneTBB on systems with multiple processor groups.
- Fixed issues with Flow Graph priorities when using limited concurrency nodes (https://github.com/uxlfoundation/oneTBB/issues/1595).
- Improved support of ``tbb::task_arena::constraints`` functionality on Windows* systems with multiple processor groups.
- Fixed ``concurrent_queue`` and ``concurrent_bounded_queue`` capacity preserving on copying, moving, and swapping (https://github.com/uxlfoundation/oneTBB/issues/1598).
- Fixed ``parallel_for_each`` compilation issues on GCC 9 in C++20 mode (https://github.com/uxlfoundation/oneTBB/issues/1552).


## :octocat: Open-Source Contributions Integrated
- Fixed linkage errors when the application is built with the hidden symbols visibility. Contributed by Vladislav Shchapov (https://github.com/uxlfoundation/oneTBB/pull/1114).
- On Linux* OS, for external thread, determined stack size using POSIX* API instead of relying on the stack size of a worker thread. Contributed by bongkyu7-kim (https://github.com/uxlfoundation/oneTBB/pull/1485).
- Added a CMake option to use relative paths instead of full paths in debug information. Contributed by Fang Xu (https://github.com/uxlfoundation/oneTBB/pull/1401).
- Improved OpenBSD* support by removing the use of direct syscalls. Contributed by Brad Smith (https://github.com/uxlfoundation/oneTBB/pull/1499).
- Fixed build issues on ARM64* when using Bazel. Contributed by snadampal (https://github.com/uxlfoundation/oneTBB/pull/1571).
- Suppressed deprecation warnings for CMake versions earlier than 3.10 when using the latest CMake. Contributed by Vladislav Shchapov (https://github.com/uxlfoundation/oneTBB/pull/1585).

# oneTBB 2022.0 Release Notes

## :rocket: Preview Features
- Extended the Flow Graph receiving nodes with a new ``try_put_and_wait`` API that submits a message to the graph and waits for its completion.

## :rotating_light: Known Limitations
- The ``oneapi::tbb::info`` namespace interfaces might unexpectedly change the process affinity mask on Windows* OS systems (see https://github.com/open-mpi/hwloc/issues/366 for details) when using hwloc version lower than 2.5.
- Using a hwloc version other than 1.11, 2.0, or 2.5 may cause an undefined behavior on Windows OS. See https://github.com/open-mpi/hwloc/issues/477 for details.
- The NUMA topology may be detected incorrectly on Windows* OS machines where the number of NUMA node threads exceeds the size of 1 processor group.
- On Windows OS on ARM64*, when compiling an application using oneTBB with the Microsoft* Compiler, the compiler issues a warning C4324 that a structure was padded due to the alignment specifier. Consider suppressing the warning by specifying /wd4324 to the compiler command line.
- C++ exception handling mechanism on Windows* OS on ARM64* might corrupt memory if an exception is thrown from any oneTBB parallel algorithm (see Windows* OS on ARM64* compiler issue: https://developercommunity.visualstudio.com/t/ARM64-incorrect-stack-unwinding-for-alig/1544293.
- When CPU resource coordination is enabled, tasks from a lower-priority ``task_arena`` might be executed before tasks from a higher-priority ``task_arena``.
- Using oneTBB on WASM*, may cause applications to run in a single thread. See [Limitations of WASM Support](https://github.com/oneapi-src/oneTBB/blob/master/WASM_Support.md#limitations).

> **_NOTE:_**  To see known limitations that impact all versions of oneTBB, refer to [oneTBB Documentation](https://oneapi-src.github.io/oneTBB/main/intro/limitations.html).


## :hammer: Issues Fixed
- Fixed the missed signal for thread request for enqueue operation.
- Significantly improved scalability of ``task_group``, ``flow_graph``, and ``parallel_for_each``.
- Removed usage of ``std::aligned_storage`` deprecated in C++23 (Inspired by Valery Matskevich https://github.com/oneapi-src/oneTBB/pull/1394).
- Fixed the issue where ``oneapi::tbb::info`` interfaces might interfere with the process affinity mask on the Windows* OS systems with multiple processor groups.


## :octocat: Open-Source Contributions Integrated
- Detect the GNU Binutils version to determine WAITPKG support better. Contributed by Martijn Courteaux (https://github.com/oneapi-src/oneTBB/pull/1347).
- Fixed the build on non-English locales. Contributed by Vladislav Shchapov (https://github.com/oneapi-src/oneTBB/pull/1450).
- Improved Bazel support. Contributed by Julian Amann (https://github.com/oneapi-src/oneTBB/pull/1434).

# oneTBB 2021.13 Release Notes

## :tada: New Features
- Extended the parallel_reduce and parallel_deterministic_reduce functional form API to better support rvalues reduction (https://github.com/uxlfoundation/oneTBB/pull/1299).

## :rotating_light: Known Limitations
- The ``oneapi::tbb::info`` namespace interfaces might unexpectedly change the process affinity mask on Windows* OS systems (see https://github.com/open-mpi/hwloc/issues/366 for details) when using hwloc version lower than 2.5.
- Using a hwloc version other than 1.11, 2.0, or 2.5 may cause an undefined behavior on Windows OS. See https://github.com/open-mpi/hwloc/issues/477 for details.
- The NUMA topology may be detected incorrectly on Windows* OS machines where the number of NUMA node threads exceeds the size of 1 processor group.
- On Windows OS on ARM64*, when compiling an application using oneTBB with the Microsoft* Compiler, the compiler issues a warning C4324 that a structure was padded due to the alignment specifier. Consider suppressing the warning by specifying /wd4324 to the compiler command line.
- C++ exception handling mechanism on Windows* OS on ARM64* might corrupt memory if an exception is thrown from any oneTBB parallel algorithm (see Windows* OS on ARM64* compiler issue: https://developercommunity.visualstudio.com/t/ARM64-incorrect-stack-unwinding-for-alig/1544293.
- When CPU resource coordination is enabled, tasks from a lower-priority ``task_arena`` might be executed before tasks from a higher-priority ``task_arena``.
- Using oneTBB on WASM* may cause applications to run in a single thread. See [Limitations of WASM Support](https://github.com/uxlfoundation/oneTBB/blob/master/WASM_Support.md#limitations).

> **_NOTE:_**  To see known limitations that impact all versions of oneTBB, refer to [oneTBB Documentation](https://uxlfoundation.github.io/oneTBB/main/intro/limitations.html).

## :hammer: Issues Fixed
- Improved security by excluding CWD from search of load-time dependencies.
- Improved performance on Apple* platforms by aligning block-time behavior with other OS.
- Improved performance on non-hybrid CPU hardware by reworking block-time behavior.
- Improved performance on ARM64 platform by fixing backoff behavior for spin loops.
- Improved performance when constraints API is used on server CPU HW with multiple NUMA nodes.

## :octocat: Open-Source Contributions Integrated
- Improved WASM support by fixing a segmentation fault happening if global tbb::task_scheduler_observer is used. Contributed by Santiago Ospina De Los Ríos https://github.com/oneapi-src/oneTBB/pull#1346).

# oneTBB 2021.12 Release Notes

## :rotating_light: Known Limitations
- The ``oneapi::tbb::info`` namespace interfaces might unexpectedly change the process affinity mask on Windows* OS systems (see https://github.com/open-mpi/hwloc/issues/366 for details) when using hwloc version lower than 2.5.
- Using a hwloc version other than 1.11, 2.0, or 2.5 may cause an undefined behavior on Windows OS. See https://github.com/open-mpi/hwloc/issues/477 for details.
- The NUMA topology may be detected incorrectly on Windows* OS machines where the number of NUMA node threads exceeds the size of 1 processor group.
- On Windows OS on ARM64*, when compiling an application using oneTBB with the Microsoft* Compiler, the compiler issues a warning C4324 that a structure was padded due to the alignment specifier. Consider suppressing the warning by specifying /wd4324 to the compiler command line.
- C++ exception handling mechanism on Windows* OS on ARM64* might corrupt memory if an exception is thrown from any oneTBB parallel algorithm (see Windows* OS on ARM64* compiler issue: https://developercommunity.visualstudio.com/t/ARM64-incorrect-stack-unwinding-for-alig/1544293.
- When CPU resource coordination is enabled, tasks from a lower-priority ``task_arena`` might be executed before tasks from a higher-priority ``task_arena``.

> **_NOTE:_**  To see known limitations that impact all versions of oneTBB, refer to [oneTBB Documentation](https://oneapi-src.github.io/oneTBB/main/intro/limitations.html).


## :hammer: Issues Fixed
- Fixed ``parallel_for_each`` algorithm behavior for iterators defining ``iterator_concept`` trait instead of ``iterator_category``.
- Fixed the redefinition issue for ``std::min`` and ``std::max`` on Windows* OS ([GitHub* #832](https://github.com/oneapi-src/oneTBB/issues/832)).
- Fixed the incorrect binary search order in ``TBBConfig.cmake``.
- Enabled the oneTBB library search using the pkg-config tool in Conda packages.

## :octocat: Open-Source Contributions Integrated
- Fixed the compiler warning for missing virtual destructor. Contributed by Elias Engelbert Plank (https://github.com/oneapi-src/oneTBB/pull/1215).

# oneTBB 2021.11 Release Notes

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


## :hammer: Issues Fixed
- Fixed ``tbb::this_task_arena()`` behavior for specific ``tbb::task_arena{1,0}``.
- Restored performance on systems with a high number of CPU cores that support ``_tpause``.

# oneTBB 2021.10 Release Notes

## :tada: New Features
- Since C++17, parallel algorithms and Flow Graph nodes are allowed to accept pointers to the member functions and member objects as the user-provided callables.
- Added missed member functions, such as assignment operators and swap function, to the ``concurrent_queue`` and ``concurrent_bounded_queue`` containers.

## :rotating_light: Known Limitations
- A static assert will cause compilation failures in oneTBB headers when compiling with clang 12.0.0 or newer if using the LLVM standard library with ``-ffreestanding`` and C++11/14 compiler options. 
- An application using Parallel STL algorithms in libstdc++ versions 9 and 10 may fail to compile due to incompatible interface changes between earlier versions of Threading Building Blocks (TBB) and oneAPI Threading Building Blocks (oneTBB). Disable support for Parallel STL algorithms by defining ``PSTL_USE_PARALLEL_POLICIES`` (in libstdc++ 9) or ``_GLIBCXX_USE_TBB_PAR_BACKEND`` (in libstdc++ 10) macro to zero before inclusion of the first standard header file in each translation unit.
- On Linux* OS, if oneAPI Threading Building Blocks (oneTBB) or Threading Building Blocks (TBB) are installed in a system folder like ``/usr/lib64``, the application may fail to link due to the order in which the linker searches for libraries. Use the ``-L`` linker option to specify the correct location of oneTBB library. This issue does not affect the program execution.
- The ``oneapi::tbb::info`` namespace interfaces might unexpectedly change the process affinity mask on Windows* OS systems (see https://github.com/open-mpi/hwloc/issues/366 for details) when using hwloc* version lower than 2.5.
- Using a hwloc* version other than 1.11, 2.0, or 2.5 may cause an undefined behavior on Windows* OS. See https://github.com/open-mpi/hwloc/issues/477 for details.
- The NUMA* topology may be detected incorrectly on Windows* OS machines where the number of NUMA* node threads exceeds the size of 1 processor group.
- On Windows* OS on ARM64*, when compiling an application using oneTBB with the Microsoft* Compiler, the compiler issues a warning C4324 that a structure was padded due to the alignment specifier. Consider suppressing the warning by specifying ``/wd4324`` to the compiler command line.
- oneTBB does not support ``fork()``, to work-around the issue, consider using task_scheduler_handle to join oneTBB worker threads before using fork().
- C++ exception handling mechanism on Windows* OS on ARM64* might corrupt memory if an exception is thrown from any oneTBB parallel algorithm (see Windows* OS on ARM64* compiler issue: https://developercommunity.visualstudio.com/t/ARM64-incorrect-stack-unwinding-for-alig/1544293).

## :hammer: Issues Fixed
- Fixed the hang in the reserve method of concurrent unordered containers ([GitHub* #1056](http://github.com/oneapi-src/oneTBB/issues/1056)).
- Fixed the C++20 three-way comparison feature detection ([GitHub* #1093](http://github.com/oneapi-src/oneTBB/issues/1093)).
- Fixed oneTBB integration with CMake* in the Conda* environment.

# oneTBB 2021.9 Release Notes

## :tada: New Features
- Hybrid CPU support is now a fully supported feature.

## :rotating_light: Known Limitations
- A static assert will cause compilation failures in oneTBB headers when compiling with clang 12.0.0 or newer if using the LLVM standard library with -ffreestanding and C++11/14 compiler options. 
- An application using Parallel STL algorithms in libstdc++ versions 9 and 10 may fail to compile due to incompatible interface changes between earlier versions of Threading Building Blocks (TBB) and oneAPI Threading Building Blocks (oneTBB). Disable support for Parallel STL algorithms by defining PSTL_USE_PARALLEL_POLICIES (in libstdc++ 9) or _GLIBCXX_USE_TBB_PAR_BACKEND (in libstdc++ 10) macro to zero before inclusion of the first standard header file in each translation unit.
- On Linux* OS, if oneAPI Threading Building Blocks (oneTBB) or Threading Building Blocks (TBB) are installed in a system folder like /usr/lib64, the application may fail to link due to the order in which the linker searches for libraries. Use the -L linker option to specify the correct location of oneTBB library. This issue does not affect the program execution.
- The oneapi::tbb::info namespace interfaces might unexpectedly change the process affinity mask on Windows* OS systems (see https://github.com/open-mpi/hwloc/issues/366 for details) when using hwloc version lower than 2.5.
- Using a hwloc version other than 1.11, 2.0, or 2.5 may cause an undefined behavior on Windows OS. See https://github.com/open-mpi/hwloc/issues/477 for details.
- The NUMA topology may be detected incorrectly on Windows OS machines where the number of NUMA node threads exceeds the size of 1 processor group.
- On Windows OS on ARM64*, when compiling an application using oneTBB with the Microsoft* Compiler, the compiler issues a warning C4324 that a structure was padded due to the alignment specifier. Consider suppressing the warning by specifying /wd4324 to the compiler command line.
- oneTBB does not support fork(), to work-around the issue, consider using task_scheduler_handle to join oneTBB worker threads before using fork().
- C++ exception handling mechanism on Windows* OS on ARM64* might corrupt memory if an exception is thrown from any oneTBB parallel algorithm (see Windows* OS on ARM64* compiler issue: https://developercommunity.visualstudio.com/t/ARM64-incorrect-stack-unwinding-for-alig/1544293).

## :hammer: Issues Fixed
- Improved robustness of thread creation algorithm on Linux* OS.
- Enabled full support of Thread Sanitizer on macOS*
- Fixed the issue with destructor calls for uninitialized objects in oneapi::tbb::parallel_for_each algorithm (GitHub* #691)
- Fixed the issue with tbb::concurrent_lru_cache when items history capacity is zero (GitHub* #265)
- Fixed compilation issues on modern GCC* versions

## :octocat: Open-Source Contributions Integrated
- Fixed the issue reported by the Address Sanitizer. Contributed by Rui Ueyama (https://github.com/oneapi-src/oneTBB/pull/959).
- Fixed the input_type alias exposed by flow_graph::join_node. Contributed by Deepan (https://github.com/oneapi-src/oneTBB/pull/868).

# oneTBB 2021.8 Release Notes

## :rotating_light: Known Limitations
- A static assert causes compilation failures in oneTBB headers when compiling with Clang* 12.0.0 or newer if using the LLVM* standard library with -ffreestanding and C++11/14 compiler options.
- An application using Parallel STL algorithms in libstdc++ versions 9 and 10 may fail to compile due to incompatible interface changes between earlier versions of Threading Building Blocks (TBB) and oneAPI Threading Building Blocks (oneTBB). Disable support for Parallel STL algorithms by defining PSTL_USE_PARALLEL_POLICIES (in libstdc++ 9) or _GLIBCXX_USE_TBB_PAR_BACKEND (in libstdc++ 10) macro to zero before inclusion of the first standard header file in each translation unit.
- On Linux* OS, if oneAPI Threading Building Blocks (oneTBB) or Threading Building Blocks (TBB) are installed in a system folder like /usr/lib64, the application may fail to link due to the order in which the linker searches for libraries. Use the -L linker option to specify the correct location of oneTBB library. This issue does not affect the program execution.
- The oneapi::tbb::info namespace interfaces might unexpectedly change the process affinity mask on Windows* OS systems (see https://github.com/open-mpi/hwloc/issues/366 for details) when using hwloc version lower than 2.5.
- Using a hwloc version other than 1.11, 2.0, or 2.5 may cause an undefined behavior on Windows* OS. See https://github.com/open-mpi/hwloc/issues/477 for details.
- The NUMA topology may be detected incorrectly on Windows OS machines where the number of NUMA node threads exceeds the size of 1 processor group.
- On Windows OS on ARM64*, when compiling an application using oneTBB with the Microsoft* Compiler, the compiler issues a warning C4324 that a structure was padded due to the alignment specifier. Consider suppressing the warning by specifying /wd4324 to the compiler command line.
- oneTBB does not support fork(), to work-around the issue, consider using task_scheduler_handle to join oneTBB worker threads before using fork().
- C++ exception handling mechanism on Windows* OS on ARM64* might corrupt memory if an exception is thrown from any oneTBB parallel algorithm (see Windows* OS on ARM64* compiler issue: https://developercommunity.visualstudio.com/t/ARM64-incorrect-stack-unwinding-for-alig/1544293).

## :hammer: Issues Fixed
- Fixed oneapi::tbb::concurrent_bounded_queue::pop return type (GitHub* [#807](https://github.com/oneapi-src/oneTBB/issues/807)).
- Fixed oneapi::tbb::concurrent_queue and oneapi::tbb::concurrent_bounded_queue with non-default constructible value types (GitHub* [#885](https://github.com/oneapi-src/oneTBB/issues/885)).
- Fixed incorrect splitting of iteration space in case there is no support for proportional splitting in custom ranges.

## :octocat: Open-Source Contributions Integrated
- Fix for full LTO* build, library and tests, on UNIX* OS systems. Contributed by Vladislav Shchapov (https://github.com/oneapi-src/oneTBB/pull/798).

# oneTBB 2021.7 Release Notes

## :rotating_light: Known Limitations
- A static assert causes compilation failures in oneTBB headers when compiling with Clang* 12.0.0 or newer if using the LLVM* standard library with -ffreestanding and C++11/14 compiler options.
- An application using Parallel STL algorithms in libstdc++ versions 9 and 10 may fail to compile due to incompatible interface changes between earlier versions of Threading Building Blocks (TBB) and oneAPI Threading Building Blocks (oneTBB). Disable support for Parallel STL algorithms by defining PSTL_USE_PARALLEL_POLICIES (in libstdc++ 9) or _GLIBCXX_USE_TBB_PAR_BACKEND (in libstdc++ 10) macro to zero before inclusion of the first standard header file in each translation unit.
- On Linux* OS, if oneAPI Threading Building Blocks (oneTBB) or Threading Building Blocks (TBB) are installed in a system folder like /usr/lib64, the application may fail to link due to the order in which the linker searches for libraries. Use the -L linker option to specify the correct location of oneTBB library. This issue does not affect the program execution.
- The oneapi::tbb::info namespace interfaces might unexpectedly change the process affinity mask on Windows* OS systems (see https://github.com/open-mpi/hwloc/issues/366 for details) when using hwloc version lower than 2.5.
- Using a hwloc version other than 1.11, 2.0, or 2.5 may cause an undefined behavior on Windows* OS. See https://github.com/open-mpi/hwloc/issues/477 for details.
- The NUMA topology may be detected incorrectly on Windows OS machines where the number of NUMA node threads exceeds the size of 1 processor group.
- On Windows OS on ARM64*, when compiling an application using oneTBB with the Microsoft* Compiler, the compiler issues a warning C4324 that a structure was padded due to the alignment specifier. Consider suppressing the warning by specifying /wd4324 to the compiler command line.
- oneTBB does not support fork(), to work-around the issue, consider using task_scheduler_handle to join oneTBB worker threads before using fork().
- C++ exception handling mechanism on Windows* OS on ARM64* might corrupt memory if an exception is thrown from any oneTBB parallel algorithm (see Windows* OS on ARM64* compiler issue: https://developercommunity.visualstudio.com/t/ARM64-incorrect-stack-unwinding-for-alig/1544293).

## :hammer: Issues Fixed
- Memory allocator crash when allocating ~1TB on 64-bit systems (GitHub* [#838](https://github.com/oneapi-src/oneTBB/issues/838)).
- Fixed thread distribution over NUMA nodes on Windows* OS systems.
- For oneapi::tbb::suspend, it is guaranteed that the user-specified callable object is executed by the calling thread.

## :octocat: Open-Source Contributions Integrated
- Fix for full LTO* build, library and tests, on UNIX* OS systems. Contributed by Vladislav Shchapov (https://github.com/oneapi-src/oneTBB/pull/798).

# oneTBB 2021.6 Release Notes

## :tada: New Features
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

## :hammer: Issues Fixed
- Memory allocator crash on a system with an incomplete /proc/meminfo (GitHub* [#584](https://github.com/oneapi-src/oneTBB/issues/584)).
- Incorrect blocking of task stealing (GitHub* #[478](https://github.com/oneapi-src/oneTBB/issues/478)).
- Hang due to incorrect decrement of a limiter_node (GitHub* [#634](https://github.com/oneapi-src/oneTBB/issues/634)).
- Memory corruption in some rare cases when passing big messages in a flow graph (GitHub* [#639](https://github.com/oneapi-src/oneTBB/issues/639)).
- Possible deadlock in a throwable flow graph node with a lightweight policy. The lightweight policy is now ignored for functors that can throw exceptions (GitHub* [#420](https://github.com/oneapi-src/oneTBB/issues/420)).
- Crash when obtaining a range from empty ordered and unordered containers (GitHub* [#641](https://github.com/oneapi-src/oneTBB/issues/641)).
- Deadlock in a concurrent_vector resize() that could happen when the new size is less than the previous size (GitHub* [#733](https://github.com/oneapi-src/oneTBB/issues/733)).

## :octocat: Open-Source Contributions Integrated
- Improved aligned memory allocation. Contributed by Andrey Semashev (https://github.com/oneapi-src/oneTBB/pull/671).
- Optimized usage of atomic_fence on IA-32 and Intel(R) 64 architectures. Contributed by Andrey Semashev (https://github.com/oneapi-src/oneTBB/pull/328).
- Fixed incorrect definition of the assignment operator in containers. Contributed by Andrey Semashev (https://github.com/oneapi-src/oneTBB/issues/372).

# oneTBB 2021.5 Release Notes

## :rocket: Preview Features
- Extended task_group interface with a new run_and_wait overload to accept task_handle.

## :tada: New Features
- Enabled Microsoft Visual Studio* 2022 and Python 3.9 support.

## :rotating_light: Known Limitations
- An application using Parallel STL algorithms in libstdc++ versions 9 and 10 may fail to compile due to incompatible interface changes between earlier versions of Threading Building Blocks (TBB) and oneAPI Threading Building Blocks (oneTBB). Disable support for Parallel STL algorithms by defining PSTL_USE_PARALLEL_POLICIES (in libstdc++ 9) or _GLIBCXX_USE_TBB_PAR_BACKEND (in libstdc++ 10) macro to zero before inclusion of the first standard header file in each translation unit.
- On Linux* OS, if oneAPI Threading Building Blocks (oneTBB) or Threading Building Blocks (TBB) are installed in a system folder like /usr/lib64, the application may fail to link due to the order in which the linker searches for libraries. Use the -L linker option to specify the correct location of oneTBB library. This issue does not affect the program execution.
- The oneapi::tbb::info namespace interfaces might unexpectedly change the process affinity mask on Windows* OS systems (see https://github.com/open-mpi/hwloc/issues/366 for details) when using hwloc version lower than 2.5.
- Using a hwloc version other than 1.11, 2.0, or 2.5 may cause an undefined behavior on Windows OS. See https://github.com/open-mpi/hwloc/issues/477 for details.
- The NUMA topology may be detected incorrectly on Windows OS machines where the number of NUMA node threads exceeds the size of 1 processor group.
- On Windows OS on ARM64*, when compiling an application using oneTBB with the Microsoft* Compiler, the compiler issues a warning C4324 that a structure was padded due to the alignment specifier. Consider suppressing the warning by specifying /wd4324 to the compiler command line.
- oneTBB does not support fork(), to work-around the issue, consider using task_scheduler_handle to join oneTBB worker threads before using fork().
- C++ exception handling mechanism on Windows* OS on ARM64* might corrupt memory if an exception is thrown from any oneTBB parallel algorithm (see Windows* OS on ARM64* compiler issue: https://developercommunity.visualstudio.com/t/ARM64-incorrect-stack-unwinding-for-alig/1544293).

## :hammer: Issues Fixed
- Reworked synchronization mechanism to reduce contention when multiple task_arena's are used concurrently.
- Fixed possible correctness issue in queuing_rw_mutex on non-Intel platforms.
- Fixed GCC* 11 warnings.
- Fixed sporadic memory corruption.

# oneTBB 2021.4 Release Notes

## :rocket: Preview Features
- Introduced the collaborative_call_once algorithm that executes the callable object exactly once, but allows other threads to join oneTBB parallel construction used inside this callable object. Inspired by Ben FrantzDale and Henry Heffan.

## :rotating_light: Known Limitations
- An application using Parallel STL algorithms in libstdc++ versions 9 and 10 may fail to compile due to incompatible interface changes between earlier versions of Threading Building Blocks (TBB) and oneAPI Threading Building Blocks (oneTBB). Disable support for Parallel STL algorithms by defining PSTL_USE_PARALLEL_POLICIES (in libstdc++ 9) or _GLIBCXX_USE_TBB_PAR_BACKEND (in libstdc++ 10) macro to zero before inclusion of the first standard header file in each translation unit.
- On Linux* OS, if oneAPI Threading Building Blocks (oneTBB) or Threading Building Blocks (TBB) are installed in a system folder like /usr/lib64, the application may fail to link due to the order in which the linker searches for libraries. Use the -L linker option to specify the correct location of oneTBB library. This issue does not affect the program execution.
- The oneapi::tbb::info namespace interfaces might unexpectedly change the process affinity mask on Windows* OS systems (see https://github.com/open-mpi/hwloc/issues/366 for details) when using hwloc version lower than 2.5.
- Using a hwloc version other than 1.11, 2.0, or 2.5 may cause an undefined behavior on Windows OS. See https://github.com/open-mpi/hwloc/issues/477 for details.
- The NUMA topology may be detected incorrectly on Windows OS machines where the number of NUMA node threads exceeds the size of 1 processor group.
- On Windows OS on ARM64*, when compiling an application using oneTBB with the Microsoft* Compiler, the compiler issues a warning C4324 that a structure was padded due to the alignment specifier. Consider suppressing the warning by specifying /wd4324 to the compiler command line.
- oneTBB does not support fork(), to work-around the issue, consider using task_scheduler_handle to join oneTBB worker threads before using fork().

## :hammer: Issues Fixed
- Enabled full support of Address Sanitizer and Thread Sanitizer.
- Fixed a race condition in tbbmalloc that may cause a crash in realloc() when using tbbmalloc_proxy.
- Enabled GCC* 11 support.
- Fixed limiter_node behavior when an integral type is used as an argument for the DecrementType template parameter.
- Fixed a possible memory leak when the static or affinity partitioners are used.
- Fixed possible system oversubscription when using a process mask and an explicit number of threads in the task arena.

## :octocat: Open-Source Contributions Integrated
- Enabled PowerPC* Linux* OS support. Contributed by Mircho Rodozov (https://github.com/oneapi-src/oneTBB/pull/461)
- Improved UNIX* system support and enabled QNX Neutrino* RTOS support. Contributed by Pablo Romero (https://github.com/oneapi-src/oneTBB/pull/440)
- Enabled experimental Bazel* build system support. Contributed by Julian Amann (https://github.com/oneapi-src/oneTBB/pull/442)
- Enabled oneTBB build for Windows* OS on ARM64*. Contributed by Michael Vitrano (https://github.com/oneapi-src/oneTBB/pull/507)
- Added MinGW* and export attributes support. Contributed by Long Nguyen (https://github.com/oneapi-src/oneTBB/pull/351)
