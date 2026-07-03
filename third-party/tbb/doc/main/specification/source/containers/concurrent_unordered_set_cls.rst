.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

========================
concurrent_unordered_set
========================
**[containers.concurrent_unordered_set]**

``oneapi::tbb::concurrent_unordered_set`` is a class template that represents an unordered sequence of unique elements.
It supports concurrent insertion, lookup, and traversal, but does not support concurrent erasure.

Class Template Synopsis
-----------------------

.. code:: cpp

    // Defined in header <oneapi/tbb/concurrent_unordered_set.h>

    namespace oneapi {
        namespace tbb {
            template <typename T,
                      typename Hash = std::hash<Key>,
                      typename KeyEqual = std::equal_to<Key>,
                      typename Allocator = tbb_allocator<std::pair<const Key, T>>>
            class concurrent_unordered_set {
            public:
                using key_type = Key;
                using value_type = Key;

                using size_type = <implementation-defined unsigned integer type>;
                using difference_type = <implementation-defined signed integer type>;

                using hasher = Hash;
                using key_equal = /*See below*/;

                using allocator_type = Allocator;

                using reference = value_type&;
                using const_reference = const value_type&;

                using pointer = typename std::allocator_traits<Allocator>::pointer;
                using const_pointer = typename std::allocator_traits<Allocator>::const_pointer;

                using iterator = <implementation-defined ForwardIterator>;
                using const_iterator = <implementation-defined constant ForwardIterator>;

                using local_iterator = <implementation-defined ForwardIterator>;
                using const_local_iterator = <implementation-defined constant ForwardIterator>;

                using node_type = <implementation-defined node handle>;

                using range_type = <implementation-defined ContainerRange>;
                using const_range_type = <implementation-defined constant ContainerRange>;

                // Construction, destruction, copying
                concurrent_unordered_set();

                explicit concurrent_unordered_set( size_type bucket_count, const hasher& hash = hasher(),
                                                   const key_equal& equal = key_equal(),
                                                   const allocator_type& alloc = allocator_type() );

                concurrent_unordered_set( size_type bucket_count, const allocator_type& alloc );

                concurrent_unordered_set( size_type bucket_count, const hasher& hash,
                                          const allocator_type& alloc );

                explicit concurrent_unordered_set( const allocator_type& alloc );

                template <typename InputIterator>
                concurrent_unordered_set( InputIterator first, InputIterator last,
                                          size_type bucket_count = /*implementation-defined*/,
                                          const hasher& hash = hasher(),
                                          const key_equal& equal = key_equal(),
                                          const allocator_type& alloc = allocator_type() );

                template <typename Inputiterator>
                concurrent_unordered_set( InputIterator first, InputIterator last,
                                          size_type bucket_count, const allocator_type& alloc );

                template <typename InputIterator>
                concurrent_unordered_set( InputIterator first, InputIterator last,
                                          size_type bucket_count, const hasher& hash,
                                          const allocator_type& alloc );

                concurrent_unordered_set( std::initializer_list<value_type> init,
                                          size_type bucket_count = /*implementation-defined*/,
                                          const hasher& hash = hasher(),
                                          const key_equal& equal = key_equal(),
                                          const allocator_type& alloc = allocator_type() );

                concurrent_unordered_set( std::initializer_list<value_type> init,
                                          size_type bucket_count, const allocator_type& alloc );

                concurrent_unordered_set( std::initializer_list<value_type> init,
                                          size_type bucket_count, const hasher& hash,
                                          const allocator_type& alloc );

                concurrent_unordered_set( const concurrent_unordered_set& other );
                concurrent_unordered_set( const concurrent_unordered_set& other,
                                          const allocator_type& alloc );

                concurrent_unordered_set( concurrent_unordered_set&& other );
                concurrent_unordered_set( concurrent_unordered_set&& other,
                                          const allocator_type& alloc );

                ~concurrent_unordered_set();

                concurrent_unordered_set& operator=( const concurrent_unordered_set& other );
                concurrent_unordered_set& operator=( concurrent_unordered_set&& other ) noexcept(/*See details*/);

                concurrent_unordered_set& operator=( std::initializer_list<value_type> init );

                allocator_type get_allocator() const;

                // Iterators
                iterator begin() noexcept;
                const_iterator begin() const noexcept;
                const_iterator cbegin() const noexcept;

                iterator end() noexcept;
                const_iterator end() const noexcept;
                const_iterator cend() const noexcept;

                // Size and capacity
                bool empty() const noexcept;
                size_type size() const noexcept;
                size_type max_size() const noexcept;

                // Concurrently safe modifiers
                std::pair<iterator, bool> insert( const value_type& value );
                iterator insert( const_iterator hint, const value_type& value );

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

                template <typename SrcHash, typename SrcKeyEqual>
                void merge( concurrent_unordered_set<T, SrcHash, SrcKeyEqual, Allocator>& source );

                template <typename SrcHash, typename SrcKeyEqual>
                void merge( concurrent_unordered_set<T, SrcHash, SrcKeyEqual, Allocator>&& source );

                template <typename SrcHash, typename SrcKeyEqual>
                void merge( concurrent_unordered_multiset<T, SrcHash, SrcKeyEqual, Allocator>& source );

                template <typename SrcHash, typename SrcKeyEqual>
                void merge( concurrent_unordered_multiset<T, SrcHash, SrcKeyEqual, Allocator>&& source );

                // Concurrently unsafe modifiers
                void clear() noexcept;

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

                void swap( concurrent_unordered_set& other );

                // Lookup
                size_type count( const key_type& key ) const;

                template <typename K>
                size_type count( const K& key ) const;

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
                std::pair<iterator, iterator> equal_range( const K& key );

                template <typename K>
                std::pair<const_iterator, const_iterator> equal_range( const K& key ) const;

                // Bucket interface
                local_iterator unsafe_begin( size_type n );
                const_local_iterator unsafe_begin( size_type n ) const;
                const_local_iterator unsafe_cbegin( size_type n ) const;

                local_iterator unsafe_end( size_type n );
                const_local_iterator unsafe_end( size_type n ) const;
                const_local_iterator unsafe_cend( size_type n ) const;

                size_type unsafe_bucket_count() const;
                size_type unsafe_max_bucket_bount() const;

                size_type unsafe_bucket_size( size_type n ) const;

                size_type unsafe_bucket( const key_type& key ) const;

                // Hash policy
                float load_factor() const;

                float max_load_factor() const;
                void max_load_factor( float ml );

                void rehash( size_type count );

                void reserve( size_type count );

                // Observers
                hasher hash_function() const;
                key_equal key_eq() const;

                // Parallel iteration
                range_type range();
                const_range_type range() const;
            }; // class concurrent_unordered_set
        } // namespace tbb
    } // namespace oneapi

Requirements:

* The expression ``std::allocator_type<Allocator>::destroy(m, val)``, where ``m`` is an object
  of the type ``Allocator`` and ``val`` is an object of type ``value_type``, must be well-formed.
  Member functions can impose stricter requirements depending on the type of the operation.
* The type ``Hash`` must meet the ``Hash`` requirements from the [hash] ISO C++ Standard section.
* The type ``KeyEqual`` must meet the ``BinaryPredicate`` requirements from the [algorithms.general] ISO C++ Standard section.
* The type ``Allocator`` must meet the ``Allocator`` requirements from the [allocator.requirements] ISO C++ Standard section.

Description
-----------

``oneapi::tbb::concurrent_unordered_set`` is an unordered sequence, which elements are organized into
buckets. The value of the hash function ``Hash`` for ``Key`` object determines the number of the bucket
in which the corresponding element will be placed.

If the qualified-id ``Hash::transparent_key_equal`` is valid and denotes a type, the member type
``concurrent_unordered_set::key_equal`` is defined as the value of this qualified-id.
In this case, the program is ill-formed if any of the following conditions are met:

* The template parameter ``KeyEqual`` is different from ``std::equal_to<Key>``.
* Qualified-id ``Hash::transparent_key_equal::is_transparent`` is not valid or does not denote a type.

Otherwise, the member type ``concurrent_unordered_set::key_equal`` is defined as the value of the
template parameter ``KeyEqual``.

Member functions
----------------

.. toctree::
    :maxdepth: 1

    concurrent_unordered_set_cls/construction_destruction_copying.rst
    concurrent_unordered_set_cls/iterators.rst
    concurrent_unordered_set_cls/size_and_capacity.rst
    concurrent_unordered_set_cls/safe_modifiers.rst
    concurrent_unordered_set_cls/unsafe_modifiers.rst
    concurrent_unordered_set_cls/lookup.rst
    concurrent_unordered_set_cls/bucket_interface.rst
    concurrent_unordered_set_cls/hash_policy.rst
    concurrent_unordered_set_cls/observers.rst
    concurrent_unordered_set_cls/parallel_iteration.rst

Non-member functions
--------------------

These functions provide binary comparison and swap operations
on ``oneapi::tbb::concurrent_unordered_set`` objects.

The exact namespace where these functions are defined is unspecified, as long as they may be used in
respective comparison operations. For example, an implementation may define the classes and functions
in the same internal namespace and define ``oneapi::tbb::concurrent_unordered_set`` as a type alias for which
the non-member functions are reachable only via argument-dependent lookup.

.. code:: cpp

    template <typename T, typename Hash,
              typename KeyEqual, typename Allocator>
    void swap( concurrent_unordered_set<T, Hash, KeyEqual, Allocator>& lhs,
               concurrent_unordered_set<T, Hash, KeyEqual, Allocator>& rhs );

    template <typename T, typename Hash,
              typename KeyEqual, typename Allocator>
    bool operator==( const concurrent_unordered_set<T, Hash, KeyEqual, Allocator>& lhs,
                     const concurrent_unordered_set<T, Hash, KeyEqual, Allocator>& rhs );

    template <typename T, typename Hash,
              typename KeyEqual, typename Allocator>
    bool operator==( const concurrent_unordered_set<T, Hash, KeyEqual, Allocator>& lhs,
                     const concurrent_unordered_set<T, Hash, KeyEqual, Allocator>& rhs );

.. toctree::
    :maxdepth: 1

    concurrent_unordered_set_cls/non_member_swap.rst
    concurrent_unordered_set_cls/non_member_binary_comparisons.rst

Other
-----

.. toctree::
    :maxdepth: 1

    concurrent_unordered_set_cls/deduction_guides.rst
