.. SPDX-FileCopyrightText: 2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==================
task_arena::attach
==================
**[deprecated.task_arena_attach_tag]**

    .. caution::

        Deprecated in oneTBB Specification 1.1.

A set of methods for constructing a ``task_arena`` with ``attach``.

.. code:: cpp

    // Defined in header <oneapi/tbb/task_arena.h>

    namespace oneapi {
        namespace tbb {

            class task_arena {
            public:
                // ...
                struct attach {};

                explicit task_arena(task_arena::attach);
                void initialize(task_arena::attach);
                // ...
            };

        } // namespace tbb
    } // namespace oneapi


Member types and constants
--------------------------

.. cpp:struct:: attach

    A tag for constructing a ``task_arena`` with ``attach``.

Member functions
----------------

.. cpp:function:: explicit task_arena(task_arena::attach)

    Creates an instance of ``task_arena`` that is connected to the internal task arena representation currently used by the calling thread.
    If no such arena exists yet, creates a ``task_arena`` with default parameters.

    .. note::

        Unlike other ``task_arena`` constructors, this one automatically initializes
        the new ``task_arena`` when connecting to an already existing arena.


.. cpp:function:: void initialize(task_arena::attach)

    If an internal task arena representation currently used by the calling thread, the method ignores arena
    parameters and connects ``task_arena`` to that internal task arena representation.
    The method has no effect when called for an already initialized ``task_arena``.

See also:

* :doc:`attach <../task_scheduler/attach_tag_type>`
