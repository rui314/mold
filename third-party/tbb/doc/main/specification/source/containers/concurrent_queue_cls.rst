.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

================
concurrent_queue
================
**[containers.concurrent_queue]**

``oneapi::tbb::concurrent_queue`` is a class template for an unbounded first-in-first-out data structure
that permits multiple threads to concurrently push and pop items.

Class Template Synopsis
-----------------------

.. code:: cpp

    // Defined in header <oneapi/tbb/concurrent_queue.h>

    namespace oneapi {
        namespace tbb {

            template <typename T, typename Allocator = cache_aligned_allocator<T>>
            class concurrent_queue {
            public:
                using value_type = T;
                using reference = T&;
                using const_reference = const T&;
                using pointer = typename std::allocator_traits<Allocator>::pointer;
                using const_pointer = typename std::allocator_traits<Allocator>::const_pointer;
                using allocator_type = Allocator;

                using size_type = <implementation-defined unsigned integer type>;
                using difference-type = <implementation-defined signed integer type>;

                using iterator = <implementation-defined ForwardIterator>;
                using const_iterator = <implementation-defined constant ForwardIterator>;

                // Construction, destruction, copying
                concurrent_queue();

                explicit concurrent_queue( const allocator_type& alloc );

                template <typename InputIterator>
                concurrent_queue( InputIterator first, InputIterator last,
                                  const allocator_type& alloc = allocator_type() );

                concurrent_queue( std::initializer_list<value_type> init,
                                  const allocator_type& alloc = allocator_type() );

                concurrent_queue( const concurrent_queue& other );
                concurrent_queue( const concurrent_queue& other, const allocator_type& alloc );

                concurrent_queue( concurrent_queue&& other );
                concurrent_queue( concurrent_queue&& other, const allocator_type& alloc );

                ~concurrent_queue();

                concurrent_queue& operator=( const concurrent_queue& other );
                concurrent_queue& operator=( concurrent_queue&& other );
                concurrent_queue& operator=( std::initializer_list<value_type> init );

                template <typename InputIterator>
                void assign( InputIterator first, InputIterator last );

                void assign( std::initializer_list<value_type> init );

                void swap( concurrent_queue& other );

                void push( const value_type& value );
                void push( value_type&& value );

                template <typename... Args>
                void emplace( Args&&... args );

                bool try_pop( value_type& result );

                allocator_type get_allocator() const;

                size_type unsafe_size() const;
                bool empty() const;

                void clear();

                iterator unsafe_begin();
                const_iterator unsafe_begin() const;
                const_iterator unsafe_cbegin() const;

                iterator unsafe_end();
                const_iterator unsafe_end() const;
                const_iterator unsafe_cend() const;
            }; // class concurrent_queue

        } // namespace tbb
    } // namespace oneapi

Requirements:

* The type ``T`` must meet the ``Erasable`` requirements from the [container.requirements] ISO C++
  Standard section. Member functions can impose stricter requirements depending on the type of the operation.
* The type ``Allocator`` must meet the ``Allocator`` requirements from the [allocator.requirements] ISO C++ Standard section.

Member functions
----------------

.. toctree::
    :maxdepth: 1

    concurrent_queue_cls/construct_destroy_copy.rst
    concurrent_queue_cls/safe_member_functions.rst
    concurrent_queue_cls/unsafe_member_functions.rst
    concurrent_queue_cls/iterators.rst

Non-member functions
--------------------

These functions provides binary comparison and swap operations on ``oneapi::tbb::concurrent_queue``
objects.

The exact namespace where this function is defined is unspecified, as long as it may be used in
respective operation. For example, an implementation may define the classes and functions
in the same internal namespace and define ``oneapi::tbb::concurrent_queue`` as a type alias for which
the non-member functions are reachable only via argument-dependent lookup.

.. code:: cpp

    template <typename T, typename Allocator>
    void swap( concurrent_queue<T, Allocator>& lhs,
               concurrent_queue<T, Allocator>& rhs );

    template <typename T, typename Allocator>
    bool operator==( const concurrent_queue<T, Allocator>& lhs,
                     const concurrent_queue<T, Allocator>& rhs );

    template <typename T, typename Allocator>
    bool operator!=( const concurrent_queue<T, Allocator>& lhs,
                     const concurrent_queue<T, Allocator>& rhs );  

.. toctree::
    :maxdepth: 1

    concurrent_queue_cls/non_member_swap.rst
    concurrent_queue_cls/non_member_binary_comparisons.rst

Other
-----

.. toctree::
    :maxdepth: 1

    concurrent_queue_cls/deduction_guides.rst
