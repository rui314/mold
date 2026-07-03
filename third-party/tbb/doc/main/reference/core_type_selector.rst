.. _core_type_selector:

Core Type Selector for Task Arena Constraints
=============================================

.. note::
    To enable this feature, set the ``TBB_PREVIEW_TASK_ARENA_CORE_TYPE_SELECTOR`` macro to 1. When available and enabled, the feature-test macro ``TBB_HAS_TASK_ARENA_CORE_TYPE_SELECTOR`` is defined.

.. contents::
    :local:
    :depth: 2

Description
***********

On a system with hybrid CPU cores, it is generally best to allow the OS to schedule threads across
all core types. However, advanced users may occasionally need to constrain the scheduling.
The ``constraints::set_core_type(core_type_id)`` API supports only a single core type, which may not allow selecting a suitable subset on processors with more than two core types.

The core type selector addresses this by allowing a callable object (*selector*) to rank every
available core type. The selector is called once for each core type returned by
``tbb::info::core_types()``. Positive scores indicate core types that should be used, negative
scores exclude core types, and a score of zero means "use only if multi-core-type constraints are
not supported" (see `Selector Requirements`_ for details).

This feature extends the :onetbb-spec:`tbb::task_arena <task_scheduler/task_arena/task_arena_cls>`
and the :onetbb-spec:`tbb::info <info_namespace>` specifications with the following API:

* Adds the ``selectable`` constant to ``task_arena``.
* Adds a new ``task_arena`` constructor template that accepts a ``constraints`` object together with
  a selector.
* Adds a new ``task_arena::initialize`` template that accepts a ``constraints`` object together with
  a selector.
* Adds a new ``info::default_concurrency`` template that returns the effective concurrency for given
  constraints and selector.

API
***

Header
------

.. code:: cpp

    #define TBB_PREVIEW_TASK_ARENA_CORE_TYPE_SELECTOR 1
    #include <oneapi/tbb/task_arena.h>
    #include <oneapi/tbb/info.h>

Synopsis
--------

.. code:: cpp

    namespace oneapi {
        namespace tbb {

            class task_arena {
            public:
                static constexpr int selectable = /* unspecified */;

                template <typename Selector>
                task_arena(constraints a_constraints, Selector a_selector,
                           unsigned reserved_slots = 1,
                           priority a_priority = priority::normal);

                template <typename Selector>
                void initialize(constraints a_constraints, Selector a_selector,
                                unsigned reserved_slots = 1,
                                priority a_priority = priority::normal);
            }; // class task_arena

            namespace info {
                template <typename Selector>
                int default_concurrency(task_arena::constraints c, Selector a_selector);
            } // namespace info

        } // namespace tbb
    } // namespace oneapi

Member Constants
----------------

.. cpp:var:: static constexpr int task_arena::selectable

A special value for ``constraints::core_type`` that indicates the core type(s) should be
determined by the selector provided to the constructor or ``initialize``.

Member Functions
----------------

.. cpp:function:: template <typename Selector> \
                  task_arena::task_arena(constraints a_constraints, Selector a_selector, \
                                        unsigned reserved_slots = 1, \
                                        priority a_priority = priority::normal)

**Requirements**: ``Selector`` must be a callable type whose call operator accepts a
``std::tuple<tbb::core_type_id, std::size_t, std::size_t>`` and returns a value convertible to ``int``.
See `Selector Requirements`_ for the full contract.

Constructs a ``task_arena`` whose core type constraint is resolved by calling ``a_selector``
on every available core type. The ``a_constraints.core_type`` field should be set to
``task_arena::selectable``; other constraint fields (``numa_id``, ``max_concurrency``,
``max_threads_per_core``) are applied as usual.

If ``a_constraints.core_type`` is not ``selectable``, the selector is ignored and the
constructor behaves identically to the existing constraints-based constructor.

.. cpp:function:: template <typename Selector> \
                  void task_arena::initialize(constraints a_constraints, Selector a_selector, \
                                              unsigned reserved_slots = 1, \
                                              priority a_priority = priority::normal)

Overrides the arena settings and forces initialization. Core type resolution follows the same
rules as the constructor above.

Functions
---------

.. cpp:function:: template <typename Selector> \
                  int info::default_concurrency(task_arena::constraints c, Selector a_selector)

**Requirements**: ``Selector`` must be a callable type whose call operator accepts a
``std::tuple<tbb::core_type_id, std::size_t, std::size_t>`` and returns a value convertible to ``int``.
See `Selector Requirements`_ for the full contract.

Returns the number of threads that would be available for a ``task_arena`` created with the
given constraints and selector. If ``c.core_type`` is ``selectable``, the selector is applied
to resolve the core type constraint before computing concurrency; otherwise, the selector is
ignored and the function behaves identically to the single-argument ``info::default_concurrency``.

Selector Requirements
*********************

A selector is a callable object whose call operator has the following effective signature:

.. code:: cpp

    int operator()(std::tuple<tbb::core_type_id, std::size_t, std::size_t> core_type_info) const;

The tuple elements are:

.. list-table::
    :header-rows: 1
    :widths: 15 20 65

    * - Index
      - Type
      - Description
    * - 0
      - ``tbb::core_type_id``
      - The core type identifier, as returned by ``tbb::info::core_types()``.
    * - 1
      - ``std::size_t``
      - Zero-based position of this core type in the vector returned by
        ``tbb::info::core_types()`` (ordered from least to most performant).
    * - 2
      - ``std::size_t``
      - Total number of available core types (``tbb::info::core_types().size()``).

The selector is called once for each core type. Its return value is interpreted as follows:

.. list-table::
    :header-rows: 1
    :widths: 20 80

    * - Score
      - Meaning
    * - Positive
      - The core type is selected for use by the arena. A higher score indicates a
        stronger preference; if the implementation can only use a single core type,
        it selects the one with the highest score.
    * - Zero
      - The core type is used **only** if multi-core-type constraints are not supported
        by the implementation. When there are positive scores and the rest are zero,
        the zeros act as a fallback to no constraint (rather than falling back
        to the single best-scored type).
    * - Negative
      - The core type is excluded from use by the arena.

If all scores are negative, no core type constraint is applied. That is equivalent to
setting ``constraints::core_type`` to ``automatic``.

Example
*******

.. literalinclude:: ./examples/core_type_selector.cpp
    :language: c++
    :start-after: /*begin_core_type_selector_example*/
    :end-before: /*end_core_type_selector_example*/

In this example, a selector excludes the least performant core type (index 0) and ranks the
remaining types by their position. The ``task_arena`` is created with
``constraints::core_type`` set to ``selectable``, so the selector is invoked to determine
which core types the arena may use. The ``info::default_concurrency`` overload with a selector
is used to query the effective concurrency before creating the arena.
