.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

================
Deduction guides
================

If possible, ``concurrent_vector`` constructors support class template argument deduction (since C++17).
The following constructors provide implicitly-generated deduction guides:

* Copy and move constructors, including constructors with explicit ``allocator_type`` argument
* Constructors, accepting ``std::initializer_list`` as an argument

In addition, the following explicit deduction guide is provided:

.. code:: cpp

    template <typename InputIterator,
              typename Allocator = tbb::cache_aligned_allocator<iterator_value_t<InputIterator>>>
    concurrent_vector( InputIterator, InputIterator,
                       Allocator = Allocator() )
    -> concurrent_vector<iterator_value_t<InputIterator>,
                         Allocator>;

Where type alias ``iterator_value_t`` defines as follows:

.. code:: cpp

    template <typename InputIterator>
    using iterator_value_t = typename std::iterator_traits<InputIterator>::value_type;

This deduction guide only participate in the overload resolution if the following requirements are met:

* The ``InputIterator`` type meets the ``InputIterator`` requirements described in the [input.iterators] section of the ISO C++ Standard.
* The ``Allocator`` type meets the ``Allocator`` requirements described in the [allocator.requirements] section of the ISO C++ Standard.

**Example**

.. code:: cpp

    #include <oneapi/tbb/concurrent_vector.h>
    #include <array>
    #include <memory>

    int main() {
        std::array<int, 100> arr;

        // Deduces cv1 as oneapi::tbb::concurrent_vector<int>
        oneapi::tbb::concurrent_vector cv1(arr.begin(), arr.end());

        std::allocator<int> alloc;


        // Deduces cv2 as oneapi::tbb::concurrent_vector<int, std::allocator<int>>
        oneapi::tbb::concurrent_vector cv2(arr.begin(), arr.end(), alloc);
    }
