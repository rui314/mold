.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

============
parallel_for
============
**[algorithms.parallel_for]**

Function template that performs parallel iteration over a range of values.

.. code:: cpp

    // Defined in header <oneapi/tbb/parallel_for.h>

    namespace oneapi {
        namespace tbb {

            template<typename Index, typename Func>
            void parallel_for(Index first, Index last, const Func& f, /* see-below */ partitioner, task_group_context& context);
            template<typename Index, typename Func>
            void parallel_for(Index first, Index last, const Func& f, task_group_context& context);
            template<typename Index, typename Func>
            void parallel_for(Index first, Index last, const Func& f, /* see-below */ partitioner);
            template<typename Index, typename Func>
            void parallel_for(Index first, Index last, const Func& f);

            template<typename Index, typename Func>
            void parallel_for(Index first, Index last, Index step, const Func& f, /* see-below */ partitioner, task_group_context& context);
            template<typename Index, typename Func>
            void parallel_for(Index first, Index last, Index step, const Func& f, task_group_context& context);
            template<typename Index, typename Func>
            void parallel_for(Index first, Index last, Index step, const Func& f, /* see-below */ partitioner);
            template<typename Index, typename Func>
            void parallel_for(Index first, Index last, Index step, const Func& f);

            template<typename Range, typename Body>
            void parallel_for(const Range& range, const Body& body, /* see-below */ partitioner, task_group_context& context);
            template<typename Range, typename Body>
            void parallel_for(const Range& range, const Body& body, task_group_context& context);
            template<typename Range, typename Body>
            void parallel_for(const Range& range, const Body& body, /* see-below */ partitioner);
            template<typename Range, typename Body>
            void parallel_for(const Range& range, const Body& body);

        } // namespace tbb
    } // namespace oneapi

A ``partitioner`` type may be one of the following entities:

* ``const auto_partitioner&``
* ``const simple_partitioner&``
* ``const static_partitioner&``
* ``affinity_partitioner&``

Requirements:

* The ``Range`` type must meet the :doc:`Range requirements <../../named_requirements/algorithms/range>`.
* The ``Body`` type must meet the :doc:`ParallelForBody requirements <../../named_requirements/algorithms/par_for_body>`.
* The ``Index`` type must meet the :doc:`ParallelForIndex requirements <../../named_requirements/algorithms/par_for_index>`.
* The ``Func`` type must meet the :doc:`ParallelForFunc requirements <../../named_requirements/algorithms/par_for_func>`.

The ``oneapi::tbb::parallel_for(first, last, step, f)`` overload represents parallel execution of the loop:

.. code:: cpp

    for (auto i = first; i < last; i += step) f(i);

The loop must not wrap around. The step value must be positive. If omitted, it is implicitly 1. 
There is no guarantee that the iterations run in parallel. A deadlock may occur if a lesser 
iteration waits for a greater iteration. The partitioning strategy is ``auto_partitioner`` when 
the parameter is not specified.

The ``parallel_for(range,body,partitioner)`` overload provides a more general form of parallel
iteration. It represents parallel execution of ``body`` over each value
in ``range``. The optional ``partitioner`` parameter specifies a partitioning strategy.

``parallel_for`` recursively splits the range into subranges to the point such that ``is_divisible()``
is false for each subrange, and makes copies of the body for each of these subranges.
For each such body/subrange pair, it invokes ``Body::operator()``.

Some of the copies of the range and body may be destroyed after ``parallel_for`` returns.
This late destruction is not an issue in typical usage, but is something to be aware of
when looking at execution traces or writing range or body objects with complex side effects.

``parallel_for`` may execute iterations in non-deterministic order.
Do not rely on any particular execution order for correctness. However, for efficiency, do expect
``parallel_for`` to tend towards operating on consecutive runs of values.

In case of serial execution, ``parallel_for`` performs iterations from left to right in the following sense.

All overloads can accept a :doc:`task_group_context <../../task_scheduler/scheduling_controls/task_group_context_cls>` object
so that the algorithm’s tasks are executed in this context. By default, the algorithm is executed in a bound context of its own.

**Complexity**

If the range and body take *O(1)* space, and the range splits into nearly equal pieces,
the space complexity is *O(P log(N))*, where *N* is the size of the range and *P* is the number of threads.

See also:

* :ref:`Partitioners <Partitioners>`

