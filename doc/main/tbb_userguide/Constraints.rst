.. _Constraints:

Constrained APIs
================

Starting from C++20, most of |full_name| APIs are constrained to
enforce `named requirements <https://spec.oneapi.com/versions/latest/elements/oneTBB/source/named_requirements.html>`_ on
template arguments types.

The violations of these requirements are detected at a compile time during the template instantiation.

.. rubric:: Example

.. code:: cpp

    // Call for body(oneapi::tbb::blocked_range) is ill-formed
    // oneapi::tbb::parallel_for call results in constraint failure
    auto body = [](const int& r) { /*...*/ };
    oneapi::tbb::parallel_for(oneapi::tbb::blocked_range{1, 10}, body);

    // Error example:
    // error: no matching function to call to oneapi::tbb::parallel_for
    // note: constraints not satisfied
    // note: the required expression 'body(range)' is invalid
            body(range);

.. caution::

    The code that violates named requirements but compiles successfully until C++20,
    may not compile in C++20 mode due to early and strict constraints diagnostics.
