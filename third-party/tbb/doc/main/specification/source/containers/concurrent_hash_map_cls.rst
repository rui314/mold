.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

===================
concurrent_hash_map
===================
**[containers.concurrent_hash_map]**

``concurrent_hash_map`` is a class template for an unordered associative container
that holds key-value pairs with unique keys and supports concurrent insertion, lookup, and erasure.

Class Template Synopsis
-----------------------

   .. code:: cpp

      // Defined in header <oneapi/tbb/concurrent_hash_map.h>

      namespace oneapi {
          namespace tbb {

             template <typename Key, typename T,
                       typename HashCompare = tbb_hash_compare<Key>,
                       typename Allocator = tbb_allocator<std::pair<const Key, T>>>
             class concurrent_hash_map {
             public:
                   using key_type = Key;
                   using mapped_type = T;
                   using value_type = std::pair<const Key, T>;

                   using reference = value_type&;
                   using const_reference = const value_type&;
                   using pointer = typename std::allocator_traits<Allocator>::pointer;
                   using const_pointer = typename std::allocator_traits<Allocator>::const_pointer;

                   using hash_compare_type = HashCompare;
                   using allocator_type = Allocator;

                   using size_type = <implementation-defined unsigned integer type>;
                   using difference_type = <implementation-defined signed integer type>;

                   using iterator = <implementation-defined ForwardIterator>;
                   using const_iterator = <implementation-defined constant ForwardIterator>;

                   using range_type = <implementation-defined ContainerRange>;
                   using const_range_type = <implementation-defined constant ContainerRange>;

                   class accessor;
                   class const_accessor;

                   // Construction, destruction, copying
                   concurrent_hash_map();

                   explicit concurrent_hash_map( const hash_compare_type& compare,
                                                 const allocator_type& alloc = allocator_type() );

                   explicit concurrent_hash_map( const allocator_type& alloc );

                   concurrent_hash_map( size_type n, const hash_compare_type& compare,
                                        const allocator_type& alloc = allocator_type() );

                   concurrent_hash_map( size_type n, const allocator_type& alloc = allocator_type() );

                   template <typename InputIterator>
                   concurrent_hash_map( InputIterator first, InputIterator last,
                                        const hash_compare_type& compare,
                                        const allocator_type& alloc = allocator_type() );

                   template <typename InputIterator>
                   concurrent_hash_map( InputIterator first, InputIterator last,
                                        const allocator_type& alloc = allocator_type() );

                   concurrent_hash_map( std::initializer_list<value_type> init,
                                        const hash_compare_type& compare = hash_compare_type(),
                                        const allocator_type& alloc = allocator_type() );

                   concurrent_hash_map( std::initializer_list<value_type> init,
                                        const allocator_type& alloc );

                   concurrent_hash_map( const concurrent_hash_map& other );
                   concurrent_hash_map( const concurrent_hash_map& other,
                                        const allocator_type& alloc );

                   concurrent_hash_map( concurrent_hash_map&& other );
                   concurrent_hash_map( concurrent_hash_map&& other,
                                        const allocator_type& alloc );

                   ~concurrent_hash_map();

                   concurrent_hash_map& operator=( const concurrent_hash_map& other );
                   concurrent_hash_map& operator=( concurrent_hash_map&& other );
                   concurrent_hash_map& operator=( std::initializer_list<value_type> init );

                   allocator_type get_allocator() const;

                   // Concurrently unsafe modifiers
                   void clear();

                   void swap( concurrent_hash_map& other );

                   // Hash policy
                   void rehash( size_type sz = 0 );
                   size_type bucket_count() const;

                   // Size and capacity
                   size_type size() const;
                   bool empty() const;
                   size_type max_size() const;

                   // Lookup
                   bool find( const_accessor& result, const key_type& key ) const;
                   bool find( accessor& result, const key_type& key );

                   template <typename K>
                   bool find( const_accessor& result, const K& key ) const;

                   template <typename K>
                   bool find( accessor& result, const K& key );

                   size_type count( const key_type& key ) const;

                   template <typename K>
                   size_type count( const K& key ) const;

                   // Modifiers
                   bool insert( const_accessor& result, const key_type& key );
                   bool insert( accessor& result, const key_type& key );

                   template <typename K>
                   bool insert( const_accessor& result, const K& key );

                   template <typename K>
                   bool insert( accessor& result, const K& key );

                   bool insert( const_accessor& result, const value_type& value );
                   bool insert( accessor& result, const value_type& value );
                   bool insert( const_accessor& result, value_type&& value );
                   bool insert( accessor& result, value_type&& value );

                   bool insert( const value_type& value );
                   bool insert( value_type&& value );

                   template <typename InputIterator>
                   void insert( InputIterator first, InputIterator last );

                   void insert( std::initializer_list<value_type> init );

                   template <typename... Args>
                   bool emplace( const_accessor& result, Args&&... args );

                   template <typename... Args>
                   bool emplace( accessor& result, Args&&... args );

                   template <typename... Args>
                   bool emplace( Args&&... args );

                   bool erase( const key_type& key );

                   template <typename K>
                   bool erase( const K& key );

                   bool erase( const_accessor& item_accessor );
                   bool erase( accessor& item_accessor );

                   // Iterators
                   iterator begin();
                   const_iterator begin() const;
                   const_iterator cbegin() const;

                   iterator end();
                   const_iterator end() const;
                   const_iterator cend() const;

                   std::pair<iterator, iterator> equal_range( const key_type& key );
                   std::pair<const_iterator, const_iterator> equal_range( const key_type& key ) const;

                   template <typename K>
                   std::pair<iterator, iterator> equal_range( const K& key );

                   template <typename K>
                   std::pair<const_iterator, const_iterator> equal_range( const K& key ) const;

                   // Parallel iteration
                   range_type range( std::size_t grainsize = 1 );
                   const_range_type range( std::size_t grainsize = 1 ) const;
             }; // class concurrent_hash_map

          } // namespace tbb
      } // namespace oneapi

Requirements:

* The expression ``std::allocator_type<Allocator>::destroy(m, val)``, where ``m`` is an object
  of the type ``Allocator`` and ``val`` is an object of type ``value_type``, must be well-formed.
  Member functions can impose stricter requirements depending on the type of the operation.
* The type ``HashCompare`` must meet the :doc:`HashCompare requirements <../named_requirements/containers/hash_compare>`.
* The type ``Allocator`` must meet the ``Allocator`` requirements from the [allocator.requirements] ISO C++ Standard section.


Member classes
--------------

.. toctree::
    :maxdepth: 1

    concurrent_hash_map_cls/accessors.rst

Member functions
----------------

.. toctree::
    :maxdepth: 1

    concurrent_hash_map_cls/construct_destroy_copy.rst
    concurrent_hash_map_cls/unsafe_modifiers.rst
    concurrent_hash_map_cls/hash_policy.rst
    concurrent_hash_map_cls/size_and_capacity.rst
    concurrent_hash_map_cls/lookup.rst
    concurrent_hash_map_cls/modifiers.rst
    concurrent_hash_map_cls/iterators.rst
    concurrent_hash_map_cls/parallel_iteration.rst

Non-member functions
--------------------

These functions provide binary comparison and swap operations on ``oneapi::tbb::concurrent_hash_map``
objects.

The exact namespace where these functions are defined is unspecified, as long as they may be used in
respective comparison operations. For example, an implementation may define the classes and functions
in the same internal namespace and define ``oneapi::tbb::concurrent_hash_map`` as a type alias for which
the non-member functions are reachable only via argument-dependent lookup.

.. code:: cpp

   template <typename Key, typename T, typename HashCompare, typename Allocator>
   bool operator==( const concurrent_hash_map<Key, T, HashCompare, Allocator>& lhs,
                    const concurrent_hash_map<Key, T, HashCompare, Allocator>& rhs );

   template <typename Key, typename T, typename HashCompare, typename Allocator>
   bool operator!=( const concurrent_hash_map<Key, T, HashCompare, Allocator>& lhs,
                    const concurrent_hash_map<Key, T, HashCompare, Allocator>& rhs );

   template <typename Key, typename T, typename HashCompare, typename Allocator>
   void swap( concurrent_hash_map<Key, T, HashCompare, Allocator>& lhs,
              concurrent_hash_map<Key, T, HashCompare, Allocator>& rhs );

.. toctree::
    :maxdepth: 1

    concurrent_hash_map_cls/non_member_swap.rst
    concurrent_hash_map_cls/non_member_binary_comparisons.rst

Other
-----

.. toctree::
    :maxdepth: 1

    concurrent_hash_map_cls/deduction_guides.rst
