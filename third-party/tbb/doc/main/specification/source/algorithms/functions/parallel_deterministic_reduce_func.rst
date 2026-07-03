.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=============================
parallel_deterministic_reduce
=============================
**[algorithms.parallel_deterministic_reduce]**

Function template that computes reduction over a range, with deterministic split/join behavior.

.. code:: cpp

    // Defined in header <oneapi/tbb/parallel_reduce.h>

    namespace oneapi {
        namespace tbb {

            template<typename Range, typename Value, typename Func, typename Reduction>
            Value parallel_deterministic_reduce( const Range& range, const Value& identity, const Func& func, const Reduction& reduction, /* see-below */ partitioner, task_group_context& context);
            template<typename Range, typename Value, typename Func, typename Reduction>
            Value parallel_deterministic_reduce( const Range& range, const Value& identity, const Func& func, const Reduction& reduction, /* see-below */ partitioner);
            template<typename Range, typename Value, typename Func, typename Reduction>
            Value parallel_deterministic_reduce( const Range& range, const Value& identity, const Func& func, const Reduction& reduction, task_group_context& context);
            template<typename Range, typename Value, typename Func, typename Reduction>
            Value parallel_deterministic_reduce( const Range& range, const Value& identity, const Func& func, const Reduction& reduction);

            template<typename Range, typename Body>
            void parallel_deterministic_reduce( const Range& range, Body& body, /* see-below */ partitioner, task_group_context& context);
            template<typename Range, typename Body>
            void parallel_deterministic_reduce( const Range& range, Body& body, /* see-below */ partitioner);
            template<typename Range, typename Body>
            void parallel_deterministic_reduce( const Range& range, Body& body, task_group_context& context);
            template<typename Range, typename Body>
            void parallel_deterministic_reduce( const Range& range, Body& body);

        } // namespace tbb
    } // namespace oneapi

A ``partitioner`` type may be one of the following entities:

* ``const simple_partitioner&``
* ``const static_partitioner&``

The function template ``parallel_deterministic_reduce`` is very similar to the ``parallel_reduce`` template.
It also has the functional and imperative forms and has :ref:`similar requirements <par_reduce_requirements>`.

Unlike ``parallel_reduce``, ``parallel_deterministic_reduce`` has deterministic behavior
with regard to splits of both ``Body`` and ``Range`` and joins of the bodies.
For the functional form, ``Func`` is applied to a deterministic set of ``Ranges``, and ``Reduction`` merges partial results in a deterministic order.
To achieve that, ``parallel_deterministic_reduce`` uses a ``simple_partitioner`` or a ``static_partitioner`` only
because other partitioners react to random work stealing behavior.

.. caution::

    Since ``simple_partitioner`` does not automatically coarsen ranges, make sure to specify an appropriate grain size.
    See :ref:`Partitioners section <Partitioners>` for more information.

``parallel_deterministic_reduce`` always invokes the ``Body`` splitting constructor for each range split.

As a result, ``parallel_deterministic_reduce`` recursively splits a range until it is no longer divisible,
and creates a new body (by calling the ``Body`` splitting constructor) for each new subrange.
Like ``parallel_reduce``, for each body split the method ``join`` is invoked in order to merge the results from the bodies.

Therefore, for given arguments, ``parallel_deterministic_reduce`` executes the same set of split and join operations
no matter how many threads participate in execution and how tasks are mapped to the threads.
If the user-provided functions are also deterministic (that is, different runs with the same input result in the same output),
multiple calls to ``parallel_deterministic_reduce`` produce the same result.
Note however that the result might differ from that obtained with an equivalent sequential (linear) algorithm.

**Complexity**

If the range and body take *O(1)* space, and the range splits into nearly equal pieces,
the space complexity is *O(P log(N))*, where *N* is the size of the range and *P* is the number of threads.

See also:

* :doc:`parallel_reduce <parallel_reduce_func>`
* :ref:`Partitioners <Partitioners>`

