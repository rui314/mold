.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=================
concurrent_vector
=================
**[containers.concurrent_vector]**

``concurrent_vector`` is a class template for a vector that can be concurrently grown and accessed.

Class Template Synopsis
-----------------------

.. code:: cpp

    // Defined in header <oneapi/tbb/concurrent_vector.h>

    namespace oneapi {
        namespace tbb {

            template <typename T,
                      typename Allocator = cache_aligned_allocator<T>>
            class concurrent_vector {
                using value_type = T;
                using allocator_type = Allocator;

                using size_type = <implementation-defined unsigned integer type>;
                using difference_type = <implementation-defined signed integer type>;

                using reference = value_type&;
                using const_reference = const value_type&;

                using pointer = typename std::allocator_traits<allocator_type>::pointer;
                using const_pointer = typename std::allocator_traits<allocator_type>::const_pointer;

                using iterator = <implementation-defined RandomAccessIterator>;
                using const_iterator = <implementation-defined constant RandomAccessIterator>;

                using reverse_iterator = std::reverse_iterator<iterator>;
                using const_reverse_iterator = std::reverse_iterator<const_iterator>;

                using range_type = <implementation-defined ContainerRange>;
                using const_range_type = <implementation-defined constant ContainerRange>;

                // Construction, destruction, copying
                concurrent_vector();
                explicit concurrent_vector( const allocator_type& alloc ) noexcept;

                explicit concurrent_vector( size_type count, const value_type& value,
                                            const allocator_type& alloc = allocator_type() );

                explicit concurrent_vector( size_type count,
                                            const allocator_type& alloc = allocator_type() );

                template <typename InputIterator>
                concurrent_vector( InputIterator first, InputIterator last,
                                   const allocator_type& alloc = allocator_type() );

                concurrent_vector( std::initializer_list<value_type> init,
                                   const allocator_type& alloc = allocator_type() );

                concurrent_vector( const concurrent_vector& other );
                concurrent_vector( const concurrent_vector& other, const allocator_type& alloc );

                concurrent_vector( concurrent_vector&& other ) noexcept;
                concurrent_vector( concurrent_vector&& other, const allocator_type& alloc );

                ~concurrent_vector();

                concurrent_vector& operator=( const concurrent_vector& other );

                concurrent_vector& operator=( concurrent_vector&& other ) noexcept(/*See details*/);

                concurrent_vector& operator=( std::initializer_list<value_type> init );

                void assign( size_type count, const value_type& value );

                template <typename InputIterator>
                void assign( InputIterator first, InputIterator last );

                void assign( std::initializer_list<value_type> init );

                // Concurrent growth
                iterator grow_by( size_type delta );
                iterator grow_by( size_type delta, const value_type& value );

                template <typename InputIterator>
                iterator grow_by( InputIterator first, InputIterator last );

                iterator grow_by( std::initializer_list<value_type> init );

                iterator grow_to_at_least( size_type n );
                iterator grow_to_at_least( size_type n, const value_type& value );

                iterator push_back( const value_type& value );
                iterator push_back( value_type&& value );

                template <typename... Args>
                iterator emplace_back( Args&&... args );

                // Element access
                value_type& operator[]( size_type index );
                const value_type& operator[]( size_type index ) const;

                value_type& at( size_type index );
                const value_type& at( size_type index ) const;

                value_type& front();
                const value_type& front() const;

                value_type& back();
                const value_type& back() const;

                // Iterators
                iterator begin();
                const_iterator begin() const;
                const_iterator cbegin() const;

                iterator end();
                const_iterator end() const;
                const_iterator cend() const;

                reverse_iterator rbegin();
                const_reverse_iterator rbegin() const;
                const_reverse_iterator crbegin() const;

                reverse_iterator rend();
                const_reverse_iterator rend() const;
                const_reverse_iterator crend() const;

                // Size and capacity
                size_type size() const noexcept;

                bool empty() const noexcept;

                size_type max_size() const noexcept;

                size_type capacity() const noexcept;

                // Concurrently unsafe operations
                void reserve( size_type n );

                void resize( size_type n );
                void resize( size_type n, const value_type& value );

                void shrink_to_fit();

                void swap( concurrent_vector& other ) noexcept(/*See details*/);

                void clear();

                allocator_type get_allocator() const;

                // Parallel iteration
                range_type range( size_type grainsize = 1 );
                const_range_type range( size_type grainsize = 1 ) const;
            }; // class concurrent_vector

        } // namespace tbb
    } // namespace oneapi

Requirements
------------

* The type ``T`` must meet the following requirements:

    - Requirements of ``Erasable`` from the [container.requirements] ISO C++ Standard section.
    - Its destructor must not throw an exception.
    - If its default constructor can throw an exception,
      the destructor must be non-virtual and work correctly on zero-filled memory.
    - Member functions can impose stricter requirements depending on the type of the operation.

* The type ``Allocator`` must meet the ``Allocator`` requirements from the [allocator.requirements] ISO C++ section.

Description
-----------

``oneapi::tbb::concurrent_vector`` is a class template that represents a sequence container with the following features:

* Multiple threads can concurrently grow the container and append new elements.
* Random access by index. The index of the first element is zero.
* Growing the container does not invalidate any existing iterators or indices.

Exception Safety
----------------

Concurrent growing is fundamentally incompatible with ideal exception safety. Nonetheless,
``oneapi::tbb::concurrent_vector`` offers a practical level of exception safety.

Growth and vector assignment append a sequence of elements to a vector. If an exception
occurs, the impact on the vector depends on the cause of the exception:

* If the exception is thrown by the constructor of an element, all subsequent
  elements in the appended sequence will be zero-filled.
* Otherwise, the exception is thrown by the vector allocator. The vector becomes
  broken. Each element in the appended sequence will be in one of three states:

  * constructed
  * zero-filled
  * unallocated in memory

Once a vector becomes broken, note the following when accessing it:

* Accessing an unallocated element with the method ``at`` causes an exception
  ``std::range_error``. Accessing an unallocated element using any other
  method has undefined behavior.
* The values of ``capacity()`` and ``size()`` may be less than
  expected.
* Access to a broken vector via ``back()`` has undefined behavior.

However, the following guarantees hold for broken or unbroken vectors:

* Let ``k`` be an index of an unallocated element. Then
  ``size() <= capacity() <= k``.
* Growth operations never cause ``size()`` or ``capacity()`` to
  decrease.

If a concurrent growth operation successfully completes, the appended sequence remains valid
and accessible even if a subsequent growth operations fails.

Member functions
----------------

.. toctree::
    :maxdepth: 1

    concurrent_vector_cls/construct_destroy_copy.rst
    concurrent_vector_cls/concurrent_growth.rst
    concurrent_vector_cls/element_access.rst
    concurrent_vector_cls/iterators.rst
    concurrent_vector_cls/size_and_capacity.rst
    concurrent_vector_cls/unsafe_operations.rst
    concurrent_vector_cls/parallel_iteration.rst

Non-member functions
--------------------

These functions provide binary and lexicographical comparison and swap operations
on ``oneapi::tbb::concurrent_vector`` objects.

The exact namespace where these functions are defined is unspecified, as long as they can be used in
respective comparison operations. For example, an implementation can define the classes and functions
in the same internal namespace and define ``oneapi::tbb::concurrent_vector`` as a type alias, for which
the non-member functions are reachable only via argument-dependent lookup.

.. code:: cpp

    template <typename T, typename Allocator>
    bool operator==( const concurrent_vector<T, Allocator>& lhs,
                     const concurrent_vector<T, Allocator>& rhs );

    template <typename T, typename Allocator>
    bool operator!=( const concurrent_vector<T, Allocator>& lhs,
                     const concurrent_vector<T, Allocator>& rhs );

    template <typename T, typename Allocator>
    bool operator<( const concurrent_vector<T, Allocator>& lhs,
                    const concurrent_vector<T, Allocator>& rhs );

    template <typename T, typename Allocator>
    bool operator<=( const concurrent_vector<T, Allocator>& lhs,
                     const concurrent_vector<T, Allocator>& rhs );

    template <typename T, typename Allocator>
    bool operator>( const concurrent_vector<T, Allocator>& lhs,
                    const concurrent_vector<T, Allocator>& rhs );

    template <typename T, typename Allocator>
    bool operator>=( const concurrent_vector<T, Allocator>& lhs,
                     const concurrent_vector<T, Allocator>& rhs );

    template <typename T, typename Allocator>
    void swap( concurrent_vector<T, Allocator>& lhs,
               concurrent_vector<T, Allocator>& rhs );

.. toctree::
    :maxdepth: 1

    concurrent_vector_cls/non_member_binary_comparisons.rst
    concurrent_vector_cls/non_member_lexicographical_comparisons.rst
    concurrent_vector_cls/non_member_swap.rst

Other
-----

.. toctree::
    :maxdepth: 1

    concurrent_vector_cls/deduction_guides.rst
