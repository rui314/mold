.. _parallel_phase_for_task_arena:

``parallel_phase`` Interface for Task Arena
====================================================================

.. note::
    To enable this feature, set ``TBB_PREVIEW_PARALLEL_PHASE`` macro to 1.

.. contents::
    :local:
    :depth: 1

Description
***********

This feature extends the `tbb::task_arena specification <https://oneapi-spec.uxlfoundation.org/specifications/oneapi/latest/elements/onetbb/source/task_scheduler/task_arena/task_arena_cls>`_
with the following API:

* Adds the ``leave_policy`` enumeration class to ``task_arena``.
* Adds ``leave_policy`` as the last parameter in ``task_arena`` constructors and ``task_arena::initialize`` methods.
  This allows you to inform the scheduler about the preferred policy for worker threads
  when they are about to leave `task_arena` due to a lack of available work.
* Adds new ``start_parallel_phase`` and ``end_parallel_phase`` interfaces to the ``task_arena`` class
  and the ``this_task_arena`` namespace. These interfaces work as hints to the scheduler to mark the start and end
  of parallel work submission into the arena, enabling different worker thread retention policies.
* Adds the Resource Acquisition is Initialization (RAII) class ``scoped_parallel_phase`` to ``task_arena``.

More details about motivation, semantics and conditions for becoming fully supported functionality can be found in the corresponding
`Request For Comments document for parallel_phase <https://github.com/uxlfoundation/oneTBB/tree/master/rfcs/experimental/parallel_phase_for_task_arena>`_.

API
***

Header
------

.. code:: cpp

    #define TBB_PREVIEW_PARALLEL_PHASE 1
    #include <oneapi/tbb/task_arena.h>

Synopsis
--------

.. code:: cpp

    namespace oneapi {
        namespace tbb {

            class task_arena {
            public:

                enum class leave_policy : /* unspecified type */ {
                    automatic = /* unspecifed */,
                    fast = /* unspecifed */,
                };

                task_arena(int max_concurrency = automatic, unsigned reserved_for_masters = 1,
                           priority a_priority = priority::normal,
                           leave_policy a_leave_policy = leave_policy::automatic);

                task_arena(const constraints& constraints_, unsigned reserved_for_masters = 1,
                           priority a_priority = priority::normal,
                           leave_policy a_leave_policy = leave_policy::automatic);

                void initialize(int max_concurrency, unsigned reserved_for_masters = 1,
                                priority a_priority = priority::normal,
                                leave_policy a_leave_policy = leave_policy::automatic);

                void initialize(constraints a_constraints, unsigned reserved_for_masters = 1,
                                priority a_priority = priority::normal,
                                leave_policy a_leave_policy = leave_policy::automatic);

                void start_parallel_phase();
                void end_parallel_phase(bool with_fast_leave = false);

                class scoped_parallel_phase {
                public:
                    scoped_parallel_phase(task_arena& ta, bool with_fast_leave = false);
                };
            }; // class task_arena

            namespace this_task_arena {
                void start_parallel_phase();
                void end_parallel_phase(bool with_fast_leave = false);
            } // namespace this_task_arena

        } // namespace tbb
    } // namespace oneapi

Member Types
----------------

.. cpp:enum:: leave_policy::automatic

When passed to a constructor or the ``initialize`` method, the initialized ``task_arena`` has
the default (possibly system specific) policy for how quickly worker threads leave the arena
when there is no more work available in the arena and when the arena is not in a parallel phase.

.. note:: Worker threads in ``task_arena`` might be retained based on internal heuristics.

.. cpp:enum:: leave_policy::fast

When passed to a constructor or the ``initialize`` method, the initialized ``task_arena``
has policy that allows worker threads to more quickly leave the arena when there is no more work
available in the arena and when the arena is not in a parallel phase.

.. cpp:class:: scoped_parallel_phase

The RAII class to map a parallel phase to a code scope.

.. cpp:function:: scoped_parallel_phase::scoped_parallel_phase(task_arena& ta, bool with_fast_leave = false)

Constructs a ``scoped_parallel_phase`` object that starts a parallel phase in the specified ``task_arena``.
If ``with_fast_leave`` is ``true``, the worker threads leave policy is temporarily set to ``fast``.

.. note:: For ``task_arena`` initialized with ``leave_policy::fast``, ``with_fast_leave`` setting has no effect.

.. note::
   When worker threads enter the arena with no active parallel phases,
   the leave policy is reset to the value set during the initialization of the arena.

Member Functions
----------------

.. cpp:function:: task_arena(const task_arena&)

Copies settings from ``task_arena`` instance including the ``leave_policy``.

.. cpp:function:: void start_parallel_phase()

Indicates a point from where the scheduler can use a hint to keep threads in the arena for longer.

.. note:: This function can also be a warm-up hint for the scheduler. It allows the scheduler to wake up worker threads in advance.

.. cpp:function:: void end_parallel_phase(bool with_fast_leave = false)

Indicates the point when the scheduler may drop a hint and no longer retain threads in the arena.
If ``with_fast_leave`` is ``true``, worker threads leave policy is temporarily set to ``fast``.

.. note:: For ``task_arena`` initialized with ``leave_policy::fast``, ``with_fast_leave`` setting has no effect.

.. note::
   When worker threads enter the arena with no active parallel phases,
   the leave policy is reset to the value set during the initialization of the arena.

Functions
---------

.. cpp:function:: void this_task_arena::start_parallel_phase()

Indicates the start of the parallel phase in the current ``task_arena``.

.. cpp:function:: void this_task_arena::end_parallel_phase(bool with_fast_leave = false)

Indicates the end of the parallel phase in the current ``task_arena``.
If ``with_fast_leave`` is ``true``, worker threads leave policy is temporarily set to ``fast``.

Example
*******

.. literalinclude:: .examples/parallel_phase_example.cpp
   :language: c++
   :start-after: /*begin_parallel_phase_example*/
   :end-before: /*end_parallel_phase_example*/

In this example, ``task_arena`` is created with ``leave_policy::fast``. It means that
worker threads are not expected to remain in ``task_arena`` once parallel work is completed.

However, the workflow includes a sequence of parallel work (initializing and sorting data) interceded by serial work (prefix sum).
To hint the start and end of parallel work, ``scoped_parallel_phase`` is used. This provides a hint to the scheduler
that worker threads might need to remain in ``task_arena`` since there is more parallel work to come.
