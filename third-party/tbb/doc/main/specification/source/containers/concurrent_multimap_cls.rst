.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

===================
concurrent_multimap
===================
**[containers.concurrent_multimap]**

``oneapi::tbb::concurrent_multimap`` is a class template that represents a sorted associative container.
It supports concurrent insertion, lookup, and traversal, but does not support concurrent erasure.
In this container, multiple elements with equal keys can be stored.

Class Template Synopsis
-----------------------

.. code:: cpp

    namespace oneapi {
        namespace tbb {

            template <typename Key,
                      typename T,
                      typename Compare = std::less<Key>,
                      typename Allocator = tbb_allocator<std::pair<const Key, T>>
            class concurrent_multimap {
            public:
                using key_type = Key;
                using mapped_type = T;
                using value_type = std::pair<const Key, T>;

                using size_type = <implementation-defined unsigned integer type>;
                using difference_type = <implementation-defined signed integer type>;

                using key_compare = Compare;
                using allocator_type = Allocator;

                using reference = value_type&;
                using const_reference = const value_type&;
                using pointer = std::allocator_traits<Allocator>::pointer;
                using const_pointer = std::allocator_traits<Allocator>::const_pointer;

                using iterator = <implementation-defined ForwardIterator>;
                using const_iterator = <implementation-defined constant ForwardIterator>;

                using node_type = <implementation-defined node handle>;

                using range_type = <implementation-defined range>;
                using const_range_type = <implementation-defined constant node handle>;

                class value_compare;

                // Construction, destruction, copying
                concurrent_multimap();
                explicit concurrent_multimap( const key_compare& comp,
                                              const allocator_type& alloc = allocator_type() );

                explicit concurrent_multimap( const allocator_type& alloc );

                template <typename InputIterator>
                concurrent_multimap( InputIterator first, InputIterator last,
                                     const key_compare& comp = key_compare(),
                                     const allocator_type& alloc = allocator_type() );

                template <typename InputIterator>
                concurrent_multimap( InputIterator first, InputIterator last,
                                     const allocator_type& alloc );

                concurrent_multimap( std::initializer_list<value_type> init,
                                     const key_compare& comp = key_compare(),
                                     const allocator_type& alloc = allocator_type() );

                concurrent_multimap( std::initializer_list<value_type> init, const allocator_type& alloc );

                concurrent_multimap( const concurrent_multimap& other );
                concurrent_multimap( const concurrent_multimap& other,
                                const allocator_type& alloc );

                concurrent_multimap( concurrent_multimap&& other );
                concurrent_multimap( concurrent_multimap&& other,
                                const allocator_type& alloc );

                ~concurrent_multimap();

                concurrent_multimap& operator=( const concurrent_multimap& other );
                concurrent_multimap& operator=( concurrent_multimap&& other );
                concurrent_multimap& operator=( std::initializer_list<value_type> init );

                allocator_type get_allocator() const;

                // Iterators
                iterator begin();
                const_iterator begin() const;
                const_iterator cbegin() const;

                iterator end();
                const_iterator end() const;
                const_iterator cend() const;

                // Size and capacity
                bool empty() const;
                size_type size() const;
                size_type max_size() const;

                // Concurrently safe modifiers
                std::pair<iterator, bool> insert( const value_type& value );

                iterator insert( const_iterator hint, const value_type& value );

                template <typename P>
                std::pair<iterator, bool> insert( P&& value );

                template <typename P>
                iterator insert( const_iterator hint, P&& value );

                std::pair<iterator, bool> insert( value_type&& value );

                iterator insert( const_iterator hint, value_type&& value );

                template <typename InputIterator>
                void insert( InputIterator first, InputIterator last );

                void insert( std::initializer_list<value_type> init );

                std::pair<iterator, bool> insert( node_type&& nh );
                iterator insert( const_iterator hint, node_type&& nh );

                template <typename... Args>
                std::pair<iterator, bool> emplace( Args&&... args );

                template <typename... Args>
                iterator emplace_hint( const_iterator hint, Args&&... args );

                template <typename SrcCompare>
                void merge( concurrent_map<Key, T, SrcCompare, Allocator>& source );

                template <typename SrcCompare>
                void merge( concurrent_map<Key, T, SrcCompare, Allocator>&& source );

                template <typename SrcCompare>
                void merge( concurrent_multimap<Key, T, SrcCompare, Allocator>& source );

                template <typename SrcCompare>
                void merge( concurrent_multimap<Key, T, SrcCompare, Allocator>&& source );

                // Concurrently unsafe modifiers
                void clear();

                iterator unsafe_erase( const_iterator pos );
                iterator unsafe_erase( iterator pos );

                iterator unsafe_erase( const_iterator first, const_iterator last );

                size_type unsafe_erase( const key_type& key );

                template <typename K>
                size_type unsafe_erase( const K& key );

                node_type unsafe_extract( const_iterator pos );
                node_type unsafe_extract( iterator pos );

                node_type unsafe_extract( const key_type& key );

                template <typename K>
                node_type unsafe_extract( const K& key );

                void swap( concurrent_multimap& other );

                // Lookup
                size_type count( const key_type& key );

                template <typename K>
                size_type count( const K& key );

                iterator find( const key_type& key );
                const_iterator find( const key_type& key ) const;

                template <typename K>
                iterator find( const K& key );

                template <typename K>
                const_iterator find( const K& key ) const;

                bool contains( const key_type& key ) const;

                template <typename K>
                bool contains( const K& key ) const;

                std::pair<iterator, iterator> equal_range( const key_type& key );
                std::pair<const_iterator, const_iterator> equal_range( const key_type& key ) const;

                template <typename K>
                std::pair<iterator, iterator>  equal_range( const K& key );
                std::pair<const_iterator, const_iterator> equal_range( const K& key ) const;

                iterator lower_bound( const key_type& key );
                const_iterator lower_bound( const key_type& key ) const;

                template <typename K>
                iterator lower_bound( const K& key );

                template <typename K>
                const_iterator lower_bound( const K& key ) const;

                iterator upper_bound( const key_type& key );
                const_iterator upper_bound( const key_type& key ) const;

                template <typename K>
                iterator upper_bound( const K& key );

                template <typename K>
                const_iterator upper_bound( const K& key ) const;

                // Observers
                key_compare key_comp() const;

                value_compare value_comp() const;

                // Parallel iteration
                range_type range();
                const_range_type range() const;
            }; // class concurrent_multimap

        } // namespace tbb
    } // namespace oneapi

Requirements:

* The expression ``std::allocator_traits<Allocator>::destroy(m, val)``, where ``m`` is an object
  of the type ``Allocator`` and ``val`` is an object of the type ``value_type``, must be well-formed.
  Member functions can impose stricter requirements depending on the type of the operation.
* The type ``Compare`` must meet the ``Compare`` requirements from the [alg.sorting] ISO C++ Standard section.
* The type ``Allocator`` must meet the ``Allocator`` requirements from the [allocator.requirements] ISO C++ Standard section.

Member classes
--------------

.. toctree::
    :maxdepth: 1

    concurrent_multimap_cls/value_compare_cls.rst

Member functions
----------------

.. toctree::
    :maxdepth: 1

    concurrent_multimap_cls/construction_destruction_copying.rst
    concurrent_multimap_cls/iterators.rst
    concurrent_multimap_cls/size_and_capacity.rst
    concurrent_multimap_cls/safe_modifiers.rst
    concurrent_multimap_cls/unsafe_modifiers.rst
    concurrent_multimap_cls/lookup.rst
    concurrent_multimap_cls/observers.rst
    concurrent_multimap_cls/parallel_iteration.rst

Non-member functions
--------------------

These functions provide binary and lexicographical comparison and swap operations
on ``oneapi::tbb::concurrent_multimap`` objects.

The exact namespace where these functions are defined is unspecified, as long as they may be used in
respective comparison operations. For example, an implementation may define the classes and functions
in the same internal namespace and define ``oneapi::tbb::concurrent_multimap`` as a type alias for which
the non-member functions are reachable only via argument-dependent lookup.

.. code:: cpp

    template <typename Key, typename T, typename Compare, typename Allocator>
    void swap( concurrent_multimap<Key, T, Compare, Allocator>& lhs,
               concurrent_multimap<Key, T, Compare, Allocator>& rhs );

    template <typename Key, typename T, typename Compare, typename Allocator>
    bool operator==( const concurrent_multimap<Key, T, Compare, Allocator>& lhs,
                     const concurrent_multimap<Key, T, Compare, Allocator>& rhs );

    template <typename Key, typename T, typename Compare, typename Allocator>
    bool operator!=( const concurrent_multimap<Key, T, Compare, Allocator>& lhs,
                     const concurrent_multimap<Key, T, Compare, Allocator>& rhs );

    template <typename Key, typename T, typename Compare, typename Allocator>
    bool operator<( const concurrent_multimap<Key, T, Compare, Allocator>& lhs,
                    const concurrent_multimap<Key, T, Compare, Allocator>& rhs );

    template <typename Key, typename T, typename Compare, typename Allocator>
    bool operator>( const concurrent_multimap<Key, T, Compare, Allocator>& lhs,
                    const concurrent_multimap<Key, T, Compare, Allocator>& rhs );

    template <typename Key, typename T, typename Compare, typename Allocator>
    bool operator<=( const concurrent_multimap<Key, T, Compare, Allocator>& lhs,
                     const concurrent_multimap<Key, T, Compare, Allocator>& rhs );

    template <typename Key, typename T, typename Compare, typename Allocator>
    bool operator>=( const concurrent_multimap<Key, T, Compare, Allocator>& lhs,
                     const concurrent_mutlimap<Key, T, Compare, Allocator>& rhs );

.. toctree::
    :maxdepth: 1

    concurrent_multimap_cls/non_member_swap.rst
    concurrent_multimap_cls/non_member_binary_comparisons.rst
    concurrent_multimap_cls/non_member_lexicographical_comparisons.rst

Other
-----

.. toctree::
    :maxdepth: 1

    concurrent_multimap_cls/deduction_guides.rst
