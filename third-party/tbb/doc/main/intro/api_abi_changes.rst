.. _api_abi_changes:

API and ABI Change Log
======================

This document tracks API and ABI changes across oneTBB releases. Each release may introduce new APIs, 
modify existing ones, or add new symbols to the binary interface. Generally, we strive to maintain backward 
compatibility, but some releases may include changes that require attention when upgrading. If a release adds
new entry points to the binary library, this is a backwards compatible change, since applications compiled 
with an older set of headers will not be affected by the presence of new symbols in the library. However, if 
a release includes layout changes to classes defined in the headers, this can lead to compatibility issues
for applications that are partially recompiled with a mix of use of new and old headers. In such cases, 
it is recommended to recompile the entire application with the new version of oneTBB to ensure compatibility.

Through the most recent oneTBB release, we have maintained backwards compatibility. However, there have been
layout changes in the 2023.0.0 and 2022.0.0 releases, which can lead to compatibility issues for applications
that are partially recompiled with different versions of the library headers. We therefore increased the major
version number for those releases to highlight the potential for compatibility issues and to encourage users to
recompile their entire application and any plugins when upgrading to those versions.

Summary Table
-------------

.. list-table::
   :header-rows: 1
   :widths: auto

   * - Release Version
     - Binary Version / Interface Version
     - Date
     - Release Notes
     - API Changes
     - ABI Changes

   * - :ref:`2023.0.0 <version-2023.0.0>`
     - 12.18 / 12180
     - April 2026
     - `Release Notes <https://github.com/uxlfoundation/oneTBB/blob/master/RELEASE_NOTES.md#onetbb-20230-release-notes>`_
     - Yes
     - Layout changes

   * - :ref:`2022.3.0 <version-2022.3.0>`
     - 12.17 / 12170
     - Oct 2025
     - `Release Notes <https://github.com/uxlfoundation/oneTBB/blob/master/RELEASE_NOTES.md#onetbb-20223-release-notes>`_
     - Yes
     - New entry points

   * - :ref:`2022.2.0 <version-2022.2.0>`
     - 12.16 / 12160
     - Jun 2025
     - `Release Notes <https://github.com/uxlfoundation/oneTBB/blob/master/RELEASE_NOTES.md#onetbb-20222-release-notes>`_
     - No
     - No

   * - :ref:`2022.1.0 <version-2022.1.0>`
     - 12.15 / 12150
     - Mar 2025
     - `Release Notes <https://github.com/uxlfoundation/oneTBB/blob/master/RELEASE_NOTES.md#onetbb-20221-release-notes>`_
     - Yes
     - New entry points

   * - :ref:`2022.0.0 <version-2022.0.0>`
     - 12.14 / 12140
     - Oct 2024
     - `Release Notes <https://github.com/uxlfoundation/oneTBB/blob/master/RELEASE_NOTES.md#onetbb-20220-release-notes>`_
     - Yes
     - New entry points and layout changes

   * - :ref:`2021.13.0 <version-2021.13.0>`
     - 12.13 / 12130
     - Jun 2024
     - `Release Notes <https://github.com/uxlfoundation/oneTBB/blob/master/RELEASE_NOTES.md#onetbb-202113-release-notes>`_
     - Yes
     - No

   * - :ref:`2021.12.0 <version-2021.12.0>`
     - 12.12 / 12120
     - Apr 2024
     - `Release Notes <https://github.com/uxlfoundation/oneTBB/blob/master/RELEASE_NOTES.md#onetbb-202112-release-notes>`_
     - No
     - No

   * - :ref:`2021.11.0 <version-2021.11.0>`
     - 12.11 / 12110
     - Nov 2023
     - `Release Notes <https://github.com/uxlfoundation/oneTBB/blob/master/RELEASE_NOTES.md#onetbb-202111-release-notes>`_
     - No
     - No

   * - :ref:`2021.10.0 <version-2021.10.0>`
     - 12.10 / 12100
     - Jul 2023
     - `Release Notes <https://github.com/uxlfoundation/oneTBB/blob/master/RELEASE_NOTES.md#onetbb-202110-release-notes>`_
     - Yes
     - No

   * - :ref:`2021.9.0 <version-2021.9.0>`
     - 12.9 / 12090
     - Apr 2023
     - `Release Notes <https://github.com/uxlfoundation/oneTBB/blob/master/RELEASE_NOTES.md#onetbb-20219-release-notes>`_
     - Yes
     - No

   * - :ref:`2021.8.0 <version-2021.8.0>`
     - 12.8 / 12080
     - Feb 2023
     - `Release Notes <https://github.com/uxlfoundation/oneTBB/blob/master/RELEASE_NOTES.md#onetbb-20218-release-notes>`_
     - Yes
     - No

   * - :ref:`2021.7.0 <version-2021.7.0>`
     - 12.7 / 12070
     - Oct 2022
     - `Release Notes <https://github.com/uxlfoundation/oneTBB/blob/master/RELEASE_NOTES.md#onetbb-20217-release-notes>`_
     - No
     - No

   * - :ref:`2021.6.0 <version-2021.6.0>`
     - 12.6 / 12060
     - Sep 2022
     - `Release Notes <https://github.com/uxlfoundation/oneTBB/blob/master/RELEASE_NOTES.md#onetbb-20216-release-notes>`_
     - Yes
     - No

   * - :ref:`2021.5.0 <version-2021.5.0>`
     - 12.5 / 12050
     - Dec 2021
     - `Release Notes <https://github.com/uxlfoundation/oneTBB/blob/master/RELEASE_NOTES.md#onetbb-20215-release-notes>`_
     - Yes
     - No

   * - :ref:`2021.4.0 <version-2021.4.0>`
     - 12.4 / 12040
     - Oct 2021
     - `Release Notes <https://github.com/uxlfoundation/oneTBB/blob/master/RELEASE_NOTES.md#onetbb-20214-release-notes>`_
     - Yes
     - New entry points

   * - :ref:`2021.3.0 <version-2021.3.0>`
     - 12.3 / 12030
     - Jun 2021
     - `Release Notes <https://www.intel.com/content/www/us/en/developer/articles/release-notes/intel-oneapi-threading-building-blocks-release-notes.html>`_
     - Yes
     - New entry points

   * - :ref:`2021.2.0 <version-2021.2.0>`
     - 12.2 / 12020
     - Apr 2021
     - `Release Notes <https://www.intel.com/content/www/us/en/developer/articles/release-notes/intel-oneapi-threading-building-blocks-release-notes.html>`_
     - Yes
     - New entry points

   * - :ref:`2021.1.1 <version-2021.1.1>`
     - 12.1 / 12010
     - Dec 2020
     - `Release Notes <https://www.intel.com/content/www/us/en/developer/articles/release-notes/intel-oneapi-threading-building-blocks-release-notes.html>`_
     - Initial oneTBB API
     - Initial oneTBB ABI

Release Details
---------------

.. _version-2023.0.0:

2023.0.0
~~~~~~~~

**API Changes:**

- :onetbb-spec:`Function create_numa_task_arenas introduced to create a set of NUMA bound arenas <task_scheduler/task_arena/task_arena_cls>`
- :doc:`Additional deduction guides for flow graph <../tbb_userguide/fg_ctad>`
- :onetbb-spec:`Additional deduction guides for blocked_range_nd <algorithms/blocked_ranges/blocked_nd_range_cls>`
- flow graph :onetbb-spec:`indexer_node <flow_graph/indexer_node_cls>` and :onetbb-spec:`join_node <flow_graph/join_node_cls>` now support 10 or more input ports 
- :doc:`Preview Feature: wait for single task in task_group <../reference/task_group_ext/wait_single_task>`
- :doc:`Preview Feature: resource_limited_node and resource_limiter classes <../reference/fg_resource_limiting>`
- :doc:`Preview Feature: advanced core-type selection <../reference/core_type_selector>`
- :doc:`Preview Feature: global control parameter for default block time behavior <../reference/parallel_phase_for_task_arena>`

**ABI Changes:**

- ordered container layout changes for scalability improvements that impact concurrent_map, concurrent_multimap, concurrent_set, 
  and concurrent_multiset

**Notes:**

The ABI is backwards compatible but issues can arise for partial recomplilation cases when objects with modified layouts are passed 
across compilation units built against headers with the older layout. 

.. _version-2022.3.0:

2022.3.0
~~~~~~~~

**API Changes:**

- :onetbb-spec:`task_arena::enqueue and task_arena::wait_for to enqueue to and wait for specific task_group <task_scheduler/task_arena/task_arena_cls>`
- :doc:`custom assertion handler support <../reference/assertion_handler>`
- :doc:`Preview Feature: dynamic task graph <../reference/task_group_dynamic_dependencies>`

**ABI Changes:**

- set/get_assertion_handler
- current_task_ptr

**Notes:**

set/get_assertion_handler symbols are used by custom assertion handler support, current_task_ptr is used by preview of task_group dependencies

.. _version-2022.2.0:

2022.2.0
~~~~~~~~

No API or ABI changes in this release.

.. _version-2022.1.0:

2022.1.0
~~~~~~~~

**API Changes:**

- `Added explicit deduction guides for blocked_nd_range <https://github.com/uxlfoundation/oneTBB/tree/v2022.1.0/rfcs/experimental/blocked_nd_range_ctad>`_
- `preview of parallel phase <https://github.com/uxlfoundation/oneTBB/tree/v2022.1.0/rfcs/experimental/parallel_phase_for_task_arena>`_

**ABI Changes:**

- enter/exit_parallel_phase

**Notes:**

enter/exit_parallel_phase is only used by preview of parallel phase. **WARNING:** there was temporary, inadvertent change that made the `unsafe_wait <https://oneapi-spec.uxlfoundation.org/specifications/oneapi/latest/elements/onetbb/source/task_scheduler/scheduling_controls/task_scheduler_handle_cls>`_ exception local for this release only.

.. _version-2022.0.0:

2022.0.0
~~~~~~~~

**API Changes:**

- `Preview of flow graph try_put_and_wait <https://github.com/uxlfoundation/oneTBB/pull/1513>`_

**ABI Changes:**

- get_thread_reference_vertex
- execution_slot

**Notes:**

`The layouts of task_group and flow::graph were changed to improve scalability. The binary library is backwards compatible
but issues can arise for partial recomplilation cases (see linked discussion) <https://github.com/uxlfoundation/oneTBB/discussions/1371>`_. get_thread_reference_vertex and execution_slot added for scalability improvements.

.. _version-2021.13.0:

2021.13.0
~~~~~~~~~

**API Changes:**

- `Better rvalues support for parallel_reduce and parallel_deterministic_reduce functional API <https://github.com/uxlfoundation/oneTBB/pull/1307>`_

**ABI Changes:**

No ABI changes in this release.

.. _version-2021.12.0:

2021.12.0
~~~~~~~~~

No API or ABI changes in this release.

.. _version-2021.11.0:

2021.11.0
~~~~~~~~~

No API or ABI changes in this release.

**Notes:**

Thread Composability Manager support introduced. It can be enabled by setting "TCM_ENABLE" environmental variable to 1

.. _version-2021.10.0:

2021.10.0
~~~~~~~~~

**API Changes:**

- `parallel algorithms and Flow Graph nodes allowed to accept pointers to the member functions and member objects as the user-provided callables <https://github.com/uxlfoundation/oneTBB/pull/880>`_
- `Added missed member functions, such as assignment operators and swap function, to the concurrent_queue and concurrent_bounded_queue containers <https://github.com/uxlfoundation/oneTBB/pull/1033>`_

**ABI Changes:**

No ABI changes in this release.

.. _version-2021.9.0:

2021.9.0
~~~~~~~~

**API Changes:**

- `Hybrid core type constraints are fully supported and no longer guarded by preview macro <https://oneapi-spec.uxlfoundation.org/specifications/oneapi/v1.2-rev-1/elements/onetbb/source/task_scheduler/task_arena/task_arena_cls#_CPPv4N11constraints9core_typeE>`_

**ABI Changes:**

No ABI changes in this release.

**Notes:**

Hybrid CPU support is now production features, including use of symbols introduced in 2021.2.0

.. _version-2021.8.0:

2021.8.0
~~~~~~~~

**API Changes:**

- `Fixed concurrent_bounded_queue return type to match specification <https://github.com/uxlfoundation/oneTBB/issues/807>`_

**ABI Changes:**

No ABI changes in this release.

.. _version-2021.7.0:

2021.7.0
~~~~~~~~

No API or ABI changes in this release.

.. _version-2021.6.0:

2021.6.0
~~~~~~~~

**API Changes:**

- `Improved support and use of the latest C++ standards for parallel_sort that allows using this algorithm with user-defined and standard library-defined objects with modern semantics <https://oneapi-spec.uxlfoundation.org/specifications/oneapi/v1.2-rev-1/elements/onetbb/source/algorithms/functions/parallel_sort_func>`_
- The following features are now fully functional: `task_arena extensions <https://oneapi-spec.uxlfoundation.org/specifications/oneapi/v1.2-rev-1/elements/onetbb/source/task_scheduler/task_arena/task_arena_cls>`_, `collaborative_call_once <https://oneapi-spec.uxlfoundation.org/specifications/oneapi/latest/elements/onetbb/source/algorithms/functions/collaborative_call_once_func>`_, heterogeneous overloads for concurrent_hash_map, and `task_scheduler_handle <https://oneapi-spec.uxlfoundation.org/specifications/oneapi/v1.2-rev-1/elements/onetbb/source/task_scheduler/scheduling_controls/task_scheduler_handle_cls>`_

**ABI Changes:**

No ABI changes in this release.

.. _version-2021.5.0:

2021.5.0
~~~~~~~~

**API Changes:**

- Preview of task_group interface with a new run_and_wait overload to accept task_handle

**ABI Changes:**

No ABI changes in this release.

.. _version-2021.4.0:

2021.4.0
~~~~~~~~

**API Changes:**

- Preview of collaborative_call_once algorithm

**ABI Changes:**

- notify_waiters

.. _version-2021.3.0:

2021.3.0
~~~~~~~~

**API Changes:**

- Extended the high-level task API to simplify migration from TBB to oneTBB
- Added mutex and rw_mutex that are suitable for long critical sections and resistant to high contention
- Added ability to customize the concurrent_hash_map mutex type
- Added heterogeneous lookup, erase, and insert operations to concurrent_hash_map

**ABI Changes:**

- enqueue(d1::task&, d1::task_group_context&, d1::task_arena_base*)
- is_writer for queuing_rw_mutex
- wait_on_address
- notify_by_address/address_all/address_one

.. _version-2021.2.0:

2021.2.0
~~~~~~~~

**API Changes:**

- Three-way comparison operators for concurrent ordered containers and concurrent_vector
- Preview of Hybrid core type constraints

**ABI Changes:**

- core_type_count
- fill_core_type_indices
- constraints_threads_per_core
- constraints_default_concurrency

**Notes:**

New symbols used by preview of Hybrid CPU support (entered production in 2021.9).

.. _version-2021.1.1:

2021.1.1
~~~~~~~~

**API Changes:**

- `Initial modernized oneTBB API <https://oneapi-spec.uxlfoundation.org/specifications/oneapi/v1.0-rev-3/elements/onetbb/source/nested-index#onetbb-section>`_

**ABI Changes:**

- Initial oneTBB ABI
