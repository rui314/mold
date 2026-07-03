.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=======================
reset_flags Enumeration
=======================
**[flow_graph.reset_flags]**

A ``reset_flags`` enumeration represents flags that can be passed to the ``graph::reset()`` function.

.. code:: cpp

    // Defined in header <oneapi/tbb/flow_graph.h>

    namespace oneapi {
    namespace tbb {
    namespace flow {

        enum reset_flags {
            rf_reset_protocol = /*implementation-defined*/,
            rf_reset_bodies = /*implementation-defined*/,
            rf_clear_edges = /*implementation-defined*/
        };

    } // namespace flow
    } // namespace tbb
    } // namespace oneapi

Its enumerated values and their meanings are as follows:

* ``rf_reset_protocol`` - All buffers are emptied, internal state of nodes reinitialized.
  All calls to ``reset()`` perform these actions.

* ``rf_reset_bodies`` - When nodes with bodies are created, the body specified in the constructor
  is copied and preserved. When ``rf_reset_bodies`` is specified, the current body of the node is deleted
  and replaced with a copy of the body saved during construction.

  .. caution::

    If the body contains state which has an external component (such as a file descriptor),
    the node may not behave the same on re-execution of the graph after body replacement. In this
    case, the node should be re-created.

* ``rf_clear_edges`` - All edges are removed from the graph.
