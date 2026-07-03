.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

================
Deduction guides
================

If possible, ``concurrent_unordered_set`` constructors support class template argument deduction (since C++17).
Copy and move constructors, including constructors with an explicit ``allocator_type`` argument,
provide implicitly-generated deduction guides.
In addition, the following explicit deduction guides are provided:

.. code:: cpp

    template <typename InputIterator,
              typename Hash = std::hash<iterator_value_t<InputIterator>>,
              typename KeyEqual = std::equal_to<iterator_value_t<InputIterator>>,
              typename Allocator = tbb::tbb_allocator<iterator_value_t<InputIterator>>>
    concurrent_unordered_set( InputIterator, InputIterator,
                              set_size_type = {},
                              Hash = Hash(),
                              KeyEqual = KeyEqual(),
                              Allocator = Allocator() )
    -> concurrent_unordered_set<iterator_value_t<InputIterator>,
                                Hash,
                                KeyEqual,
                                Allocator>;

    template <typename InputIterator,
              typename Allocator>
    concurrent_unordered_set( InputIterator, InputIterator,
                              set_size_type,
                              Allocator )
    -> concurrent_unordered_set<iterator_value_t<InputIterator>,
                                std::hash<iterator_value_t<InputIterator>>,
                                std::equal_to<iterator_value_t<InputIterator>>,
                                Allocator>;

    template <typename T,
              typename Hash = std::hash<T>,
              typename KeyEqual = std::equal_to<T>,
              typename Allocator = tbb::tbb_allocator<T>>
    concurrent_unordered_set( std::initializer_list<T>,
                              set_size_type = {},
                              Hash = Hash(),
                              KeyEqual = KeyEqual(),
                              Allocator = Allocator() )
    -> concurrent_unordered_set<T,
                                Hash,
                                KeyEqual,
                                Allocator>;

    template <typename T,
              typename Allocator>
    concurrent_unordered_set( std::initializer_list<T>,
                              set_size_type,
                              Allocator )
    -> concurrent_unordered_set<T,
                                std::hash<T>,
                                std::equal_to<T>,
                                Allocator>;

    template <typename T,
              typename Allocator>
    concurrent_unordered_set( std::initializer_list<T>,
                              Allocator )
    -> concurrent_unordered_set<T,
                                std::hash<T>,
                                std::equal_to<T>,
                                Allocator>;

    template <typename T,
              typename Hash,
              typename Allocator>
    concurrent_unordered_set( std::initializer_list<T>,
                              set_size_type,
                              Hash,
                              Allocator )
    -> concurrent_unordered_set<T,
                                Hash,
                                std::equal_to<T>,
                                Allocator>;

Where the ``set_size_type`` type refers to the ``size_type`` member type of the deduced ``concurrent_unordered_set``
and the type alias ``iterator_value_t`` is defined as follows:

.. code:: cpp

    template <typename InputIterator>
    using iterator_value_t = typename std::iterator_traits<InputIterator>::value_type;

These deduction guides only participate in the overload resolution if the following requirements are met:

* The ``InputIterator`` type meets the ``InputIterator`` requirements described in the [input.iterators] section of the ISO C++ Standard.
* The ``Allocator`` type meets the ``Allocator`` requirements described in the [allocator.requirements] section of the ISO C++ Standard.
* The ``Hash`` type does not meet the ``Allocator`` requirements.
* The ``KeyEqual`` type does not meet the ``Allocator`` requirements.

**Example**

.. code:: cpp

    #include <oneapi/tbb/concurrent_unordered_set.h>
    #include <vector>
    #include <functional>

    struct CustomHasher {...};

    int main() {
        std::vector<int> v;

        // Deduces s1 as concurrent_unordered_set<int>
        oneapi::tbb::concurrent_unordered_set s1(v.begin(), v.end());

        // Deduces s2 as concurrent_unordered_set<int, CustomHasher>;
        oneapi::tbb::concurrent_unordered_set s2(v.begin(), v.end(), CustomHasher{});
    }
