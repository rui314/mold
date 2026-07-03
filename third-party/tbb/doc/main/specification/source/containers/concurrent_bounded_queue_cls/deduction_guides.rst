.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

================
Deduction guides
================

If possible, ``concurrent_bounded_queue`` constructors support class template argument deduction (since C++17).
Copy and move constructors, including constructors with an explicit ``allocator_type`` argument,
provide implicitly-generated deduction guides.
In addition, the following explicit deduction guide is provided:

.. code:: cpp

    template <typename InputIterator,
              typename Allocator = tbb::cache_aligned_allocator<iterator_value_t<InputIterator>>
    concurrent_bounded_queue( InputIterator, InputIterator,
                              Allocator = Allocator() )
    -> concurrent_bounded_queue<iterator_value_t<InputIterator>,
                                Allocator>;

Where the type alias ``iterator_value_t`` is defined as follows:

.. code:: cpp

    template <typename InputIterator>
    using iterator_value_t = typename std::iterator_traits<InputIterator>::value_type;

This deduction guides only participate in the overload resolution if the following requirements are met:

* The ``InputIterator`` type meets the ``InputIterator`` requirements described in the [input.iterators] section of the ISO C++ Standard.
* The ``Allocator`` type meets the ``Allocator`` requirements described in the [allocator.requirements] section of the ISO C++ Standard.

**Example**

.. code:: cpp

    #include <oneapi/tbb/concurrent_queue.h>
    #include <vector>
    #include <memory>

    int main() {
        std::vector<int> vec;

        // Deduces cq1 as oneapi::tbb::concurrent_bounded_queue<int>
        oneapi::tbb::concurrent_bounded_queue cq1(vec.begin(), vec.end());

        // Deduces cq2 as oneapi::tbb::concurrent_bounded_queue<int, std::allocator<int>>
        oneapi::tbb::concurrent_bounded_queue cq2(vec.begin(), vec.end(), std::allocator<int>{})
    }
