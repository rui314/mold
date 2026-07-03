.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=================
parallel_for_each
=================
**[algorithms.parallel_for_each]**

Function template that processes work items in parallel.

.. code:: cpp

    // Defined in header <oneapi/tbb/parallel_for_each.h>

    namespace oneapi {
        namespace tbb {

            template<typename InputIterator, typename Body>
            void parallel_for_each( InputIterator first, InputIterator last, Body body );
            template<typename InputIterator, typename Body>
            void parallel_for_each( InputIterator first, InputIterator last, Body body, task_group_context& context );

            template<typename Container, typename Body>
            void parallel_for_each( Container& c, Body body );
            template<typename Container, typename Body>
            void parallel_for_each( Container& c, Body body, task_group_context& context );

            template<typename Container, typename Body>
            void parallel_for_each( const Container& c, Body body );
            template<typename Container, typename Body>
            void parallel_for_each( const Container& c, Body body, task_group_context& context );

        } // namespace tbb
    } // namespace oneapi

Requirements:

* The ``Body`` type must meet the :doc:`ParallelForEachBody requirements <../../named_requirements/algorithms/par_for_each_body>`.
  Since C++17, ``Body`` may also be a pointer to a member function in ``std::iterator_traits<InputIterator>::value_type``.
* The ``InputIterator`` type must meet the `Input Iterator` requirements from the [input.iterators] section of the ISO C++ Standard.
* If ``InputIterator`` type does not meet the `Forward Iterator` requirements from the [forward.iterators] section of the ISO C++ Standard,
  the ``std::iterator_traits<InputIterator>::value_type`` type must be constructible from ``std::iterator_traits<InputIterator>::reference``.
* The ``Container`` type must meet the :doc:`ContainerBasedSequence requirements <../../named_requirements/algorithms/container_based_sequence>`.
* The type returned by ``std::begin`` and ``std::end`` applied to a ``Container`` object
  must meet the same requirements as the ``InputIterator`` type above.

The ``parallel_for_each`` template has two forms.

The sequence form ``parallel_for_each(first, last, body)`` applies a function object ``body`` over a
sequence [``first,last``). Items may be processed in parallel.

The container form ``parallel_for_each(c, body)`` is equivalent to ``parallel_for_each(std::begin(c), std::end(c), body)``.

All overloads can accept a :doc:`task_group_context <../../task_scheduler/scheduling_controls/task_group_context_cls>` object
so that the algorithm’s tasks are executed in this context. By default, the algorithm is executed in a bound context of its own.

feeder Class
------------

Additional work items can be added by ``body`` if it has a second argument of type ``feeder``.
The function terminates when ``body(x)`` returns for all items ``x`` that were in the input
sequence or added by method ``feeder::add``.

.. toctree::
    :titlesonly:

    feeder.rst


Example
-------

The following code sketches a body with the two-argument form of ``operator()``.

.. code:: cpp

    struct MyBody {
        void operator()(item_t item, parallel_do_feeder<item_t>& feeder ) {
            for each new piece of work implied by item do {
                item_t new_item = initializer;
                feeder.add(new_item);
            }
        }
    };
