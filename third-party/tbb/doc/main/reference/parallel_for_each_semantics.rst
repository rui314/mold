.. _parallel_for_each_semantics:

parallel_for_each Body semantics and requirements
=================================================

.. contents::
    :local:
    :depth: 1

Description
***********

This page clarifies `ParallelForEachBody <https://spec.oneapi.com/versions/latest/elements/oneTBB/source/named_requirements/algorithms/par_for_each_body.html>`_
named requirements for ``tbb::parallel_for_each`` algorithm specification.

.. code:: cpp

    namespace oneapi {
        namespace tbb {

            template <typaname InputIterator, typename Body>
            void parallel_for_each( InputIterator first, InputIterator last, Body body ); // overload (1)
            template <typename InputIterator, typename Body>
            void parallel_for_each( InputIterator first, InputIterator last, Body body, task_group_context& group ); // overload (2)

            template <typename Container, typename Body>
            void parallel_for_each( Container& c, Body body ); // overload (3)
            template <typename Container, typename Body>
            void parallel_for_each( Container& c, Body body, task_group_context& group ); // overload (4)

            template <typename Container, typename Body>
            void parallel_for_each( const Container& c, Body body ); // overload (5)
            template <typename Container, typename Body>
            void parallel_for_each( const Container& c, Body body, task_group_context& group ); // overload (6)

        } // namespace tbb
    } // namespace oneapi

Terms
-----

* ``iterator`` determines the type of the iterator passed into ``parallel_for_each`` algorithm (which is ``InputIterator`` for overloads `(1)` and `(2)`
  and ``decltype(std::begin(c))`` for overloads `(3) - (6)`)
* ``value_type`` - the type ``typename std::iterator_traits<iterator>::value_type``
* ``reference`` -  the type ``typename std::iterator_traits<iterator>::reference``.

Requirements for different iterator types
-----------------------------------------

If the ``iterator`` satisfies `Input iterator` named requirements from [input.iterators] ISO C++ Standard section and do not satisfies
`Forward iterator` named requirements from [forward.iterators] ISO C++ Standard section, ``tbb::parallel_for_each`` requires the execution
of the ``body`` with an object of type ``const value_type&`` or ``value_type&&`` to be well-formed. If both forms are well-formed, an overload with
rvalue reference will be preferred.

.. caution::

  If the ``Body`` only takes non-const lvalue reference to ``value_type``, named requirements above are violated and the program can be ill-formed.

If the ``iterator`` satisfies `Forward iterator` named requirements from [forward.iterators] ISO C++ Standard section, ``tbb::parallel_for_each`` requires the execution of the ``body``
with an object of type ``reference`` to be well-formed.

Requirements for ``Body`` with ``feeder`` argument
--------------------------------------------------

Additional elements submitted into ``tbb::parallel_for_each`` through the ``feeder::add`` passes to the ``Body`` as rvalues and therefore the corresponding
execution of the ``Body`` is required to be well-formed.
