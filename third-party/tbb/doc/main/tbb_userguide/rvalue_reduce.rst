.. _rvalue_reduce:

Use of r-Values for Reduction
=============================

The functional form of ``parallel_reduce`` supports rvalue references in both the reduction
function and the combine function. This allows efficient use of move operations when
accumulated results are expensive to copy.

The example below merges a collection of ``std::set`` objects into a single set.
Because the accumulator is passed as an rvalue reference, the algorithm can
transfer nodes between sets without copying or moving the underlying data.

.. literalinclude:: ./examples/rvalue_reduce.cpp
    :language: c++
    :start-after: /*begin_rvalue_reduce_example*/
    :end-before: /*end_rvalue_reduce_example*/

.. rubric:: See also

* `oneapi::tbb::parallel_reduce specification <https://oneapi-spec.uxlfoundation.org/specifications/oneapi/latest/elements/onetbb/source/algorithms/functions/parallel_reduce_func>`_
* `ParallelReduceFunc named requirement <https://oneapi-spec.uxlfoundation.org/specifications/oneapi/latest/elements/onetbb/source/named_requirements/algorithms/par_reduce_func>`_
* `ParallelReduceReduction named requirement <https://oneapi-spec.uxlfoundation.org/specifications/oneapi/latest/elements/onetbb/source/named_requirements/algorithms/par_reduce_reduction>`_