.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

================
Deduction guides
================

If possible, ``concurrent_hash_map`` constructors support class template argument deduction (since C++17).
Copy and move constructors, including constructors with an explicit ``allocator_type`` argument,
provide implicitly-generated deduction guides.
In addition, the following explicit deduction guides are provided:

.. code:: cpp

    template <typename InputIterator,
              typename HashCompare = tbb_hash_compare<iterator_key_t<InputIterator>>,
              typename Allocator = tbb_allocator<iterator_alloc_value_t<InputIterator>>>
    concurrent_hash_map( InputIterator, InputIterator,
                         HashCompare = HashCompare(),
                         Allocator = Allocator() )
    -> concurrent_hash_map<iterator_key_t<InputIterator>,
                           iterator_mapped_t<InputIterator>,
                           HashCompare,
                           Allocator>;

    template <typename InputIterator,
              typename Allocator>
    concurrent_hash_map( InputIterator, InputIterator, Allocator )
    -> concurrent_hash_map<iterator_key_t<InputIterator>,
                           iterator_mapped_t<InputIterator>,
                           tbb_hash_compare<iterator_key_t<InputIterator>>,
                           Allocator>;

    template <typename Key, typename T,
              typename HashCompare = tbb_hash_compare<std::remove_const_t<Key>>,
              typename Allocator = tbb_allocator<std::pair<const Key, T>>>
    concurrent_hash_map( std::initializer_list<std::pair<Key, T>>,
                         HashCompare = HashCompare(),
                         Allocator = Allocator() )
    -> concurrent_hash_map<std::remove_const_t<Key>,
                           T,
                           HashCompare,
                           Allocator>;

    template <typename Key, typename T,
              typename Allocator>
    concurrent_hash_map( std::initializer_list<std::pair<Key, T>>,
                         Allocator )
    -> concurrent_hash_map<std::remove_const_t<Key>,
                           T,
                           tbb_hash_compare<std::remove_const_t<Key>>,
                           Allocator>;

Where the type aliases ``iterator_key_t``, ``iterator_mapped_t``, and ``iterator_alloc_value_t``
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
* The ``HashCompare`` type does not meet the ``Allocator`` requirements.

**Example**

.. code:: cpp

    #include <oneapi/tbb/concurrent_hash_map.h>
    #include <vector>

    int main() {
        std::vector<std::pair<const int, float>> v;

        // Deduces chmap1 as oneapi::tbb::concurrent_hash_map<int, float>
        oneapi::tbb::concurrent_hash_map chmap1(v.begin(), v.end());

        std::allocator<std::pair<const int, float>> alloc;
        // Deduces chmap2 as oneapi::tbb::concurrent_hash_map<int, float,
        //                                            tbb_hash_compare<int>,
        //                                            std::allocator<std::pair<const int, float>>>
        oneapi::tbb::concurrent_hash_map chmap2(v.begin(), v.end(), alloc);
    }
