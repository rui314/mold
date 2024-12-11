.. _rvalue_reduce:

Parallel Reduction for rvalues
==============================

.. contents::
    :local:
    :depth: 1

Description
***********

|full_name| implementation extends the `ParallelReduceFunc <https://oneapi-spec.uxlfoundation.org/specifications/oneapi/latest/elements/onetbb/source/named_requirements/algorithms/par_reduce_func>`_ and
`ParallelReduceReduction <https://oneapi-spec.uxlfoundation.org/specifications/oneapi/latest/elements/onetbb/source/named_requirements/algorithms/par_reduce_reduction>`_
to optimize operating with ``rvalues`` using functional form of ``tbb::parallel_reduce`` and ``tbb::parallel_deterministic_reduce`` algorithms.

API
***

Header
------

.. code:: cpp

    #include <oneapi/tbb/parallel_reduce.h>

ParallelReduceFunc Requirements: Pseudo-Signature, Semantics
------------------------------------------------------------

.. cpp:function:: Value Func::operator()(const Range& range, Value&& x) const

or

.. cpp:function:: Value Func::operator()(const Range& range, const Value& x) const

    Accumulates the result for a subrange, starting with initial value ``x``. The ``Range`` type must meet the 
    `Range requirements <https://oneapi-spec.uxlfoundation.org/specifications/oneapi/latest/elements/onetbb/source/named_requirements/algorithms/range>`_. 
    The ``Value`` type must be the same as a corresponding template parameter for the `parallel_reduce algorithm <https://oneapi-spec.uxlfoundation.org/specifications/oneapi/latest/elements/onetbb/source/algorithms/functions/parallel_reduce_func>`_.

    If both ``rvalue`` and ``lvalue`` forms are provided, the ``rvalue`` is preferred.

ParallelReduceReduction Requirements: Pseudo-Signature, Semantics
-----------------------------------------------------------------

.. cpp:function:: Value Reduction::operator()(Value&& x, Value&& y) const

or

.. cpp:function:: Value Reduction::operator()(const Value& x, const Value& y) const

    Combines the ``x`` and ``y`` results. The ``Value`` type must be the same as a corresponding template parameter for the `parallel_reduce algorithm <https://oneapi-spec.uxlfoundation.org/specifications/oneapi/latest/elements/onetbb/source/algorithms/functions/parallel_reduce_func>`_.

    If both ``rvalue`` and ``lvalue`` forms are provided, the ``rvalue`` is preferred.

Example
*******

.. code:: cpp
    
    // C++17
    #include <oneapi/tbb/parallel_reduce.h>
    #include <oneapi/tbb/blocked_range.h>
    #include <vector>
    #include <set>

    int main() {
        std::vector<std::set<int>> sets = ...;

        oneapi::tbb::parallel_reduce(oneapi::tbb::blocked_range<size_t>(0, sets.size()),
                                     std::set<int>{}, // identity element - empty set
                                     [&](const oneapi::tbb::blocked_range<size_t>& range, std::set<int>&& value) {
                                         for (size_t i = range.begin(); i < range.end(); ++i) {
                                             // Having value as a non-const rvalue reference allows to efficiently
                                             // transfer nodes from sets[i] without copying/moving the data
                                             value.merge(std::move(sets[i]));
                                         }
                                         return value;
                                     },
                                     [&](std::set<int>&& x, std::set<int>&& y) {
                                         x.merge(std::move(y));
                                         return x;
                                     }
                                     );
    }

.. rubric:: See also

* `oneapi::tbb::parallel_reduce specification <https://oneapi-spec.uxlfoundation.org/specifications/oneapi/latest/elements/onetbb/source/algorithms/functions/parallel_reduce_func>`_
* `oneapi::tbb::parallel_deterministic_reduce specification <https://oneapi-spec.uxlfoundation.org/specifications/oneapi/latest/elements/onetbb/source/algorithms/functions/parallel_deterministic_reduce_func>`_
* `ParallelReduceFunc specification <https://oneapi-spec.uxlfoundation.org/specifications/oneapi/latest/elements/onetbb/source/named_requirements/algorithms/par_reduce_func>`_
* `ParallelReduceReduction specification <https://oneapi-spec.uxlfoundation.org/specifications/oneapi/latest/elements/onetbb/source/named_requirements/algorithms/par_reduce_reduction>`_
