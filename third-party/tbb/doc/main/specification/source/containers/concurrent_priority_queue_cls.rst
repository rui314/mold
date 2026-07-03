.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=========================
concurrent_priority_queue
=========================
**[containers.concurrent_priority_queue]**

``oneapi::tbb::concurrent_priority_queue`` is a class template for an unbounded priority queue that permits
multiple threads to concurrently push and pop items. Items are popped in a priority order.

Class Template Synopsis
-----------------------

.. code:: cpp

    namespace oneapi {
        namespace tbb {

            template <typename T, typename Compare = std::less<T>,
                      typename Allocator = cache_aligned_allocator<T>>
            class concurrent_priority_queue {
            public:
                using value_type = T;
                using reference = T&;
                using const_reference = const T&;
                using size_type = <implementation-defined unsigned integer type>;
                using difference_type = <implementation-defined signed integer type>;
                using allocator_type = Allocator;

                concurrent_priority_queue();
                explicit concurrent_priority_queue( const allocator_type& alloc );

                explicit concurrent_priority_queue( const Compare& compare,
                                                    const allocator_type& alloc = allocator_type() );
                                                
                explicit concurrent_priority_queue( size_type init_capacity, const allocator_type& alloc = allocator_type() );

                explicit concurrent_priority_queue( size_type init_capacity, const Compare& compare,
                                                    const allocator_type& alloc = allocator_type() );

                template <typename InputIterator>
                concurrent_priority_queue( InputIterator first, InputIterator last,
                                           const allocator_type& alloc = allocator_type() );

                template <typename InputIterator>
                concurrent_priority_queue( InputIterator first, InputIterator last,
                                           const Compare& compare, const allocator_type& alloc = allocator_type() );

                concurrent_priority_queue( std::initializer_list<value_type> init,
                                           const allocator_type& alloc = allocator_type() );

                concurrent_priority_queue( std::initializer_list<value_type> init,
                                           const Compare& compare, const allocator_type& alloc = allocator_type() );

                concurrent_priority_queue( const concurrent_priority_queue& other );
                concurrent_priority_queue( const concurrent_priority_queue& other, const allocator_type& alloc );

                concurrent_priority_queue( concurrent_priority_queue&& other );
                concurrent_priority_queue( concurrent_priority_queue&& other, const allocator_type& alloc );

                ~concurrent_priority_queue();

                concurrent_priority_queue& operator=( const concurrent_priority_queue& other );
                concurrent_priority_queue& operator=( concurrent_priority_queue&& other );
                concurrent_priority_queue& operator=( std::initializer_list<value_type> init );

                template <typename InputIterator>
                void assign( InputIterator first, InputIterator last );

                void assign( std::initializer_list<value_type> init );

                void swap( concurrent_priority_queue& other );

                allocator_type get_allocator() const;

                void clear();

                bool empty() const;
                size_type size() const;

                void push( const value_type& value );
                void push( value_type&& value );

                template <typename... Args>
                void emplace( Args&&... args );

                bool try_pop( value_type& value );
            }; // class concurrent_priority_queue

        }; // namespace tbb
    } // namespace oneapi 

Requirements:

* The type ``T`` must meet the ``Erasable`` requirements from  [container.requirements] ISO C++ 
  Standard section. Member functions can impose stricter requirements depending on the type of the operation.
* The type ``Compare`` must meet the ``Compare`` requirements from [alg.sorting] ISO C++ Standard section. 
* The type ``Allocator`` must meet the ``Allocator`` requirements from [allocator.requirements] ISO C++ Standard section.

Member functions
----------------

.. toctree::
    :maxdepth: 1

    concurrent_priority_queue_cls/construct_destroy_copy.rst
    concurrent_priority_queue_cls/size_and_capacity.rst
    concurrent_priority_queue_cls/safe_modifiers.rst
    concurrent_priority_queue_cls/unsafe_modifiers.rst

Non-member functions
--------------------

These functions provides binary comparison and swap operations on ``oneapi::tbb::concurrent_priority_queue``
objects.

The exact namespace where these functions are defined is unspecified, as long as they may be used in
respective comparison operations. For example, an implementation may define the classes and functions
in the same internal namespace and define ``oneapi::tbb::concurrent_priority_queue`` as a type alias for which
the non-member functions are reachable only via argument-dependent lookup.

.. code:: cpp

    template <typename T, typename Compare, typename Allocator>
    void swap( concurrent_priority_queue<T, Compare, Allocator>& lhs,
               concurrent_priority_queue<T, Compare, Allocator>& rhs );

    template <typename T, typename Compare, typename Allocator>
    bool operator==( const concurrent_priority_queue<T, Compare, Allocator>& lhs,
                     const concurrent_priority_queue<T, Compare, Allocator>& rhs );

    template <typename T, typename Compare, typename Allocator>
    bool operator!=( const concurrent_priority_queue<T, Compare, Allocator>& lhs,
                     const concurrent_priority_queue<T, Compare, Allocator>& rhs );  

.. toctree::
    :maxdepth: 1

    concurrent_priority_queue_cls/non_member_swap.rst
    concurrent_priority_queue_cls/non_member_binary_comparisons.rst

Other
-----

.. toctree::
    :maxdepth: 1

    concurrent_priority_queue_cls/deduction_guides.rst
