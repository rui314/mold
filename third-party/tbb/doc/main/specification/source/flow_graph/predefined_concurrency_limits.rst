.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=============================
Predefined Concurrency Limits
=============================
**[flow_graph.concurrency_limits]**

Predefined constants that can be used as ``function_node``, ``multifunction_node``, and ``async_node``
constructors arguments to define concurrency limit.

.. code:: cpp

    // Defined in header <oneapi/tbb/flow_graph.h>

    namespace oneapi {
    namespace tbb {
    namespace flow {

        std::size_t unlimited = /*implementation-defined*/;
        std::size_t serial = /*implementation-defined*/;

    } // namespace flow
    } // namespace tbb
    } // namespace oneapi

``unlimited`` concurrency allows an unlimited number of invocations of the body to execute concurrently.

``serial`` concurrency allows only a single call of body to execute concurrently.
