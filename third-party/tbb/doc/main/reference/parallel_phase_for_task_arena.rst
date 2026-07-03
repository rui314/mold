.. _parallel_phase_for_task_arena:

``parallel_phase`` Interface for Task Arena
====================================================================

.. note::
    To enable this feature, set ``TBB_PREVIEW_PARALLEL_PHASE`` macro to 1. When available and enabled, the feature-test macro ``TBB_HAS_PARALLEL_PHASE`` is defined.

.. contents::
    :local:
    :depth: 1

Description
***********

By default, oneTBB uses a *delayed thread leave* heuristic: after completing work in an arena,
worker threads remain for an implementation-defined duration, anticipating that new parallel
work will arrive soon. This benefits most workloads by reducing the latency of starting
subsequent parallel computations. However, this behavior can be undesirable, especially if

* parallel tasks are submitted at irregular intervals or with long gaps, and idle threads waste CPU resources;
* oneTBB use is interleaved with another threading, and idle threads cause CPU oversubscription.

For explicit control over worker thread retention, a *leave policy* determines
how fast worker threads leave an arena when no work is available. Additionally, the
*parallel phase* API lets users bracket regions of recurrent parallel work so the scheduler can
retain threads more aggressively during those regions and release them promptly afterward.

This feature extends the :onetbb-spec:`tbb::task_arena specification <task_scheduler/task_arena/task_arena_cls>`
with the following API:

* Adds the ``leave_policy`` enumeration class to ``task_arena``.
* Adds ``leave_policy`` as the last parameter in ``task_arena`` constructors and ``task_arena::initialize`` methods.
  This allows you to inform the scheduler about the preferred policy for worker threads
  when they are about to leave ``task_arena`` due to a lack of available work.
* Adds new ``start_parallel_phase`` and ``end_parallel_phase`` interfaces to the ``task_arena`` class
  and the ``this_task_arena`` namespace. These interfaces work as hints to the scheduler to mark the start and end
  of parallel work submission into the arena, enabling different worker thread retention policies.
* Adds the Resource Acquisition is Initialization (RAII) class ``scoped_parallel_phase`` to ``task_arena``.
* Adds the ``leave_policy`` parameter to the ``global_control`` class, providing application-wide
  control over the default worker thread leave behavior for arenas initialized implicitly or with
  ``leave_policy::automatic``.

API
***

Header
------

.. code:: cpp

    #define TBB_PREVIEW_PARALLEL_PHASE 1
    #include <oneapi/tbb/task_arena.h>
    #include <oneapi/tbb/global_control.h>

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

            class global_control {
            public:
                enum parameter {
                    // ...
                    leave_policy,
                    // ...
                };
            }; // class global_control

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

Global Control Integration
--------------------------

.. cpp:enum:: global_control::leave_policy

**Selection rule**: see below

When the ``leave_policy`` parameter is active on a ``global_control`` object with
the value ``task_arena::leave_policy::fast``, initializing an arena with
``task_arena::leave_policy::automatic`` behaves as if the arena is initialized with
``task_arena::leave_policy::fast``. Arenas that were already initialized (including implicit arenas) are not affected 
by changes to the ``leave_policy`` parameter on a ``global_control`` object.

When multiple ``global_control`` objects exist for the ``leave_policy`` parameter,
their values are combined as follows: the active parameter value equals to
``task_arena::leave_policy::fast`` if any alive ``global_control`` object sets that value,
otherwise it equals to ``task_arena::leave_policy::automatic``.

The following table summarizes the interaction between the per-arena and global leave policies
when an arena is created:

.. table::

    +------------------------+-------------------------+------------------------+
    | Arena ``leave_policy`` | Global ``leave_policy`` | Initial State          |
    +========================+=========================+========================+
    | ``fast``               | any                     | Fast leave             |
    +------------------------+-------------------------+------------------------+
    | ``automatic``          | ``fast``                | Fast leave             |
    +------------------------+-------------------------+------------------------+
    | ``automatic``          | ``automatic`` (default) | System-specific policy |
    +------------------------+-------------------------+------------------------+
.. note::
   The ``global_control::leave_policy`` parameter provides application-wide control,
   while ``task_arena::leave_policy`` and ``parallel_phase`` provide per-arena control.
   After arena initialization, the parallel phase API can modify the thread leave behavior
   for the arena at runtime, regardless of the initial state set by the global control.

Example
*******

.. literalinclude:: ./examples/parallel_phase_example.cpp
   :language: c++
   :start-after: /*begin_parallel_phase_example*/
   :end-before: /*end_parallel_phase_example*/

In this example, ``global_control::leave_policy`` is set to ``task_arena::leave_policy::fast``, enabling fast
leave behavior for the ``task_arena``, which is initialized with ``leave_policy::automatic``. This means that
worker threads are not expected to remain in ``task_arena`` once parallel work is completed.

However, the workflow includes a sequence of parallel work (initializing and sorting data) interceded by serial work (prefix sum).
To hint the start and end of parallel work, ``scoped_parallel_phase`` is used. This provides a hint to the scheduler
that worker threads might need to remain in ``task_arena`` since there is more parallel work to come.
