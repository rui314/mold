.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

================
Deduction guides
================

If possible, ``concurrent_multiset`` constructors support class template argument deduction (since C++17).
Copy and move constructors, including constructors with an explicit ``allocator_type`` argument,
provide implicitly-generated deduction guides.
In addition, the following explicit deduction guides are provided:

.. code:: cpp

    template <typename InputIterator,
              typename Compare = std::less<iterator_value_t<InputIterator>>,
              typename Allocator = tbb::tbb_allocator<iterator_value_t<InputIterator>>>
    concurrent_multiset( InputIterator, InputIterator,
                         Compare = Compare(),
                         Allocator = Allocator() )
    -> concurrent_multiset<iterator_value_t<InputIterator>,
                           Compare,
                           Allocator>;

    template <typename InputIterator,
              typename Allocator>
    concurrent_multiset( InputIterator, InputIterator,
                         Allocator )
    -> concurrent_multiset<iterator_value_t<InputIterator>,
                           std::less<iterator_value_t<InputIterator>>,
                           Allocator>;

    template <typename Key,
              typename Compare = std::less<Key>,
              typename Allocator = tbb::tbb_allocator<Key>>
    concurrent_multiset( std::initializer_list<Key>,
                         Compare = Compare(),
                         Allocator = Allocator() )
    -> concurrent_multiset<Key,
                           Compare,
                           Allocator>;

    template <typename Key,
              typename Allocator>
    concurrent_multiset( std::initializer_list<Key>,
                         Allocator )
    -> concurrent_multiset<Key,
                           std::less<Key>,
                           Allocator>;

Where the type alias ``iterator_value_t`` is defined as follows:

.. code:: cpp

    template <typename InputIterator>
    using iterator_value_t = typename std::iterator_traits<InputIterator>::value_type;

These deduction guides only participate in the overload resolution if the following requirements are met:

* The ``InputIterator`` type meets the ``InputIterator`` requirements described in the [input.iterators] section of the ISO C++ Standard.
* The ``Allocator`` type meets the ``Allocator`` requirements described in the [allocator.requirements] section of the ISO C++ Standard.
* The ``Compare`` type does not meet the ``Allocator`` requirements.

**Example**

.. code:: cpp

    #include <oneapi/tbb/concurrent_set.h>
    #include <vector>

    int main() {
        std::vector<int> v;

        // Deduces cs1 as concurrent_multiset<int>
        oneapi::tbb::concurrent_multiset cs1(v.begin(), v.end());

        // Deduces cs2 as concurrent_multiset<int>
        oneapi::tbb::concurrent_multiset cs2({1, 2, 3});
    }
