.. _feature_test_macros:

Feature-test Macros
===================

.. contents::
    :local:
    :depth: 2

Description
***********

|short_name| defines a set of preprocessor macros corresponding to the features
provided by the library. They are intended for detecting the presence of these features.

Each of these macros is defined in the header ``<oneapi/tbb/version.h>`` and in the feature headers
specified in the table below.

For preview features, the feature test macro is only defined if the feature is enabled by defining its
preview macro. You cannot use a feature test macro to guard setting of the feature preview macro.
For example:

.. code:: cpp

    // Wrong
    #include <oneapi/tbb/version.h>
    #if TBB_HAS_FEATURE_X
    #define TBB_PREVIEW_FEATURE_X 1 // Never reached
    #include <oneapi/tbb/feature_header.h>
    #endif
    // Correct
    #define TBB_PREVIEW_FEATURE_X 1
    #include <oneapi/tbb/version.h>
    #if TBB_HAS_FEATURE_X
    #include <oneapi/tbb/feature_header.h>
    #endif

Each macro value follows the pattern ``YYYYMM``, where ``YYYY`` is a year, and ``MM`` is a month when
the corresponding feature was introduced or updated. These values can be increased if the capabilities of given features
are extended. The table below contains only the most recent values.

.. container:: tablenoborder

    .. list-table::
        :header-rows: 1

        * -    Feature
          -    Macro Name
          -    Value
          -    Header(s)
        * -    :ref:`Resource Limiting in the Flow Graph<fg_resource_limiting>`
          -    ``TBB_HAS_FLOW_GRAPH_RESOURCE_LIMITING``
          -    ``202603``
          -    ``<oneapi/tbb/flow_graph.h>``
        * -    :ref:`parallel_phase Interface for Task Arena<parallel_phase_for_task_arena>`
          -    ``TBB_HAS_PARALLEL_PHASE``
          -    ``202603``
          -    ``<oneapi/tbb/task_arena.h>``
        * -    :ref:`Core Type Selector for Task Arena Constraints<core_type_selector>`
          -    ``TBB_HAS_TASK_ARENA_CORE_TYPE_SELECTOR``
          -    ``202603``
          -    | ``<oneapi/tbb/task_arena.h>``
               | ``<oneapi/tbb/info.h>``
        * -    :ref:`task_group Dynamic Dependencies<dynamic_dependencies>`
          -    ``TBB_HAS_TASK_GROUP_DEPENDENCIES``
          -    ``202603``
          -    | ``<oneapi/tbb/task_group.h>``
               | ``<oneapi/tbb/task_arena.h>``
        * -    :ref:`Waiting for Individual Tasks in task_group<wait_single_task>`
          -    ``TBB_HAS_TASK_GROUP_WAIT_FOR_SINGLE_TASK``
          -    ``202603``
          -    | ``<oneapi/tbb/task_group.h>``
               | ``<oneapi/tbb/task_arena.h>``

Example
-------

The following example uses a feature-test macro to conditionally enable ``parallel_phase``
hints when supported by the library:

.. literalinclude:: ./examples/feature_test_macros.cpp
    :language: c++
    :start-after: /*begin_feature_test_macros_example*/
    :end-before: /*end_feature_test_macros_example*/
