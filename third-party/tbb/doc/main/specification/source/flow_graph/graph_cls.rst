.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=====
graph
=====
**[flow_graph.graph]**

Class that serves as a handle to a flow graph of nodes and edges.

.. code:: cpp

    // Defined in header <oneapi/tbb/flow_graph.h>

    namespace oneapi {
    namespace tbb {
    namespace flow {

        class graph {
        public:
            graph();
            graph(task_group_context& context);
            ~graph();

            void wait_for_all();

            void reset(reset_flags f = rf_reset_protocol);
            void cancel();
            bool is_cancelled();
            bool exception_thrown();
        };

    } // namespace flow
    } // namespace tbb
    } // namespace oneapi

reset_flags enumeration
-----------------------

.. toctree::
    :titlesonly:

    reset_flags_enum.rst

Member functions
----------------

.. cpp:function:: graph(task_group_context& context )

    Constructs a graph with no nodes. If ``context`` is specified, the
    graph tasks are executed in this context. By default, the graph is
    executed in a bound context of its own.

.. cpp:function:: ~graph()

    Calls ``wait_for_all()`` on the graph, then destroys the graph.

.. cpp:function:: void wait_for_all()

    Blocks execution until all tasks associated with the graph have completed or cancelled.

.. cpp:function:: void reset(reset_flags f = rf_reset_protocol)

    Resets the graph according to the specified flags.
    Flags to ``reset()`` can be combined with bitwise-``or``.

    .. note::

        ``reset()`` is a thread-unsafe operation, don't call it concurrently.

.. cpp:function:: void cancel()

    Cancels all tasks in the graph.

.. cpp:function:: bool is_cancelled()

    Returns: ``true`` if the graph was cancelled during the last call to
    ``wait_for_all()``; ``false``, otherwise.

.. cpp:function:: bool exception_thrown()

    Returns: ``true`` if during the last call to ``wait_for_all()`` an
    exception was thrown; ``false``, otherwise.
