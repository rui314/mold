.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

================
Deduction guides
================

If possible, ``concurrent_unordered_map`` constructors support class template argument deduction (since C++17).
Copy and move constructors, including constructors with an explicit ``allocator_type`` argument,
provide implicitly-generated deduction guides.
In addition, the following explicit deduction guides are provided:

.. code:: cpp

    template <typename InputIterator,
              typename Hash = std::hash<iterator_key_t<InputIterator>>,
              typename KeyEqual = std::equal_to<iterator_key_t<InputIterator>>,
              typename Allocator = tbb::tbb_allocator<iterator_alloc_value_t<InputIterator>>>
    concurrent_unordered_map( InputIterator, InputIterator,
                              map_size_type = {},
                              Hash = Hash(),
                              KeyEqual = KeyEqual(),
                              Allocator = Allocator() )
    -> concurrent_unordered_map<iterator_key_t<InputIterator>,
                                iterator_mapped_t<InputIterator>,
                                Hash,
                                KeyEqual,
                                Allocator>;

    template <typename InputIterator,
              typename Hash,
              typename Allocator>
    concurrent_unordered_map( InputIterator, InputIterator,
                              map_size_type,
                              Hash,
                              Allocator )
    -> concurrent_unordered_map<iterator_key_t<InputIterator>,
                                iterator_mapped_t<InputIterator>,
                                Hash,
                                std::equal_to<iterator_key_t<InputIterator>>,
                                Allocator>;

    template <typename InputIterator,
              typename Allocator>
    concurrent_unordered_map( InputIterator, InputIterator,
                              map_size_type,
                              Allocator )
    -> concurrent_unordered_map<iterator_key_t<InputIterator>,
                                iterator_mapped_t<InputIterator>,
                                std::hash<iterator_key_t<InputIterator>>,
                                std::equal_to<iterator_key_t<InputIterator>>,
                                Allocator>;

    template <typename Key,
              typename T,
              typename Hash = std::hash<std::remove_const_t<Key>>,
              typename KeyEqual = std::equal_to<std::remove_const_t<Key>>,
              typename Allocator = tbb::tbb_allocator<std::pair<const Key, T>>>
    concurrent_unordered_map( std::initializer_list<std::pair<Key, T>>,
                              map_size_type = {},
                              Hash = Hash(),
                              KeyEqual = KeyEqual(),
                              Allocator = Allocator() )
    -> concurrent_unordered_map<std::remove_const_t<Key>,
                                T,
                                Hash,
                                KeyEqual,
                                Allocator>;

    template <typename Key,
              typename T,
              typename Allocator>
    concurrent_unordered_map( std::initializer_list<std::pair<Key, T>>,
                              map_size_type,
                              Allocator )
    -> concurrent_unordered_map<std::remove_const_t<Key>,
                                T,
                                std::hash<std::remove_const_t<Key>>,
                                std::equal_to<std::remove_const_t<Key>>,
                                Allocator>;

    template <typename Key,
              typename T,
              typename Allocator>
    concurrent_unordered_map( std::initializer_list<std::pair<Key, T>>,
                              Allocator )
    -> concurrent_unordered_map<std::remove_const_t<Key>,
                                T,
                                std::hash<std::remove_const_t<Key>>,
                                std::equal_to<std::remove_const_t<Key>>,
                                Allocator>;

    template <typename Key,
              typename T,
              typename Hash,
              typename Allocator>
    concurrent_unordered_map( std::initializer_list<std::pair<Key, T>>,
                              map_size_type,
                              Hash,
                              Allocator )
    -> concurrent_unordered_map<std::remove_const_t<Key>,
                                T,
                                Hash,
                                std::equal_to<std::remove_const_t<Key>>,
                                Allocator>;

Where the ``map_size_type`` type refers to the ``size_type`` member type of the deduced ``concurrent_unordered_map``
and the type aliases ``iterator_key_t``, ``iterator_mapped_t``, and ``iterator_alloc_value_t``
are defined as follows:

.. code:: cpp

    template <typename InputIterator>
    using iterator_key_t = std::remove_const_t<typename std::iterator_traits<InputIterator>::value_type::first_type>;

    template <typename InputIterator>
    using iterator_mapped_t = typename std::iterator_traits<InputIterator>::value_type::second_type;

    template <typename InputIterator>
    using iterator_alloc_value_t = std::pair<std::add_const_t<iterator_key_t<InputIterator>,
                                             iterator_mapped_t<InputIterator>>>;

These deduction guides only participate in the overload resolution if the following requirements are met:

* The ``InputIterator`` type meets the ``InputIterator`` requirements described in the [input.iterators] section of the ISO C++ Standard.
* The ``Allocator`` type meets the ``Allocator`` requirements described in the [allocator.requirements] section of the ISO C++ Standard.
* The ``Hash`` type does not meet the ``Allocator`` requirements.
* The ``KeyEqual`` type does not meet the ``Allocator`` requirements.

**Example**

.. code:: cpp

    #include <oneapi/tbb/concurrent_unordered_map.h>
    #include <vector>
    #include <functional>

    struct CustomHasher {...};

    int main() {
        std::vector<std::pair<int, float>> v;

        // Deduces m1 as concurrent_unordered_map<int, float>
        oneapi::tbb::concurrent_unordered_map m1(v.begin(), v.end());

        // Deduces m2 as concurrent_unordered_map<int, float, CustomHasher>;
        oneapi::tbb::concurrent_unordered_map m2(v.begin(), v.end(), CustomHasher{});
    }
