.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==================================
Construction, destruction, copying
==================================

Empty container constructors
----------------------------

    .. code:: cpp

        concurrent_vector();

        explicit concurrent_vector( const allocator_type& alloc );

    Constructs an empty ``concurrent_vector``.

    If provided, uses the allocator ``alloc`` to allocate the memory.

Constructors from the sequence of elements
------------------------------------------

    .. code:: cpp

        explicit concurrent_vector( size_type count, const value_type& value,
                                    const allocator_type& alloc = allocator_type() );

    Constructs a ``concurrent_vector`` containing ``count`` copies of the ``value`` using the allocator ``alloc``.

---------------------------------------------

    .. code:: cpp

        explicit concurrent_vector( size_type count,
                                    const allocator_type& alloc = allocator_type() );

    Constructs a ``concurrent_vector`` containing ``n`` default constructed in-place elements using the allocator ``alloc``.

---------------------------------------------

    .. code:: cpp

        template <typename InputIterator>
        concurrent_vector( InputIterator first, InputIterator last,
                           const allocator_type& alloc = allocator_type() );

    Constructs a ``concurrent_vector`` contains all elements from the half-open interval ``[first, last)``
    using the allocator ``alloc``.

    **Requirements**: the type ``InputIterator`` must meet the requirements of ``InputIterator``
    from the [input.iterators] ISO C++ Standard section.

---------------------------------------------

    .. code:: cpp

        concurrent_vector( std::initializer_list<value_type> init,
                           const allocator_type& alloc = allocator_type() );

    Equivalent to ``concurrent_vector(init.begin(), init.end(), alloc)``.

Copying constructors
--------------------

    .. code:: cpp

        concurrent_vector( const concurrent_vector& other );

        concurrent_vector( const concurrent_vector& other,
                           const allocator_type& alloc );

    Constructs a copy of ``other``.

    If the allocator argument is not provided, it is obtained by calling
    ``std::allocator_traits<allocator_type>::select_on_container_copy_construction(other.get_allocator())``.

    The behavior is undefined in case of concurrent operations with ``other``.

Moving constructors
-------------------

    .. code:: cpp

        concurrent_vector( concurrent_vector&& other );

        concurrent_vector( concurrent_vector&& other,
                           const allocator_type& alloc );

    Constructs a ``concurrent_vector`` with the contents of ``other`` using move semantics.

    ``other`` is left in a valid, but unspecified state.

    If the allocator argument is not provided, it is obtained by calling ``std::move(other.get_allocator())``.

    The behavior is undefined in case of concurrent operations with ``other``.

Destructor
----------

    .. code:: cpp

        ~concurrent_vector();

    Destroys the ``concurrent_vector``. Calls destructors of the stored elements and
    deallocates the used storage.

    The behavior is undefined in case of concurrent operations with ``*this``.

Assignment operators
--------------------

    .. code:: cpp

        concurrent_vector& operator=( const concurrent_vector& other );

    Replaces all elements in ``*this`` by the copies of the elements in ``other``.

    Copy-assigns allocators if ``std::allocator_traits<allocator_type>::propagate_on_container_copy_assignment::value``
    is ``true``.

    The behavior is undefined in case of concurrent operations with ``*this`` and ``other``.

    **Returns**: a reference to ``*this``.

---------------------------------------------

    .. code:: cpp

        concurrent_vector& operator=( concurrent_vector&& other ) noexcept(/*See below*/);

    Replaces all elements in ``*this`` by the elements in ``other`` using move semantics.

    ``other`` is left in a valid, but unspecified state.

    Move assigns allocators if ``std::allocator_traits<allocator_type>::propagate_on_container_move_assignment::value``
    is ``true``.

    The behavior is undefined in case of concurrent operations with ``*this`` and ``other``.

    **Returns**: a reference to ``*this``.

    **Exceptions**: ``noexcept`` specification:

        .. code:: cpp

            noexcept(std::allocator_traits<allocator_type>::propagate_on_container_move_assignment::value ||
                     std::allocator_traits<allocator_type>::is_always_equal::value)

---------------------------------------------

    .. code:: cpp

        concurrent_vector& operator=( std::initializer_list<value_type> init );

    Replaces all elements in ``*this`` by the elements in ``init``.

    The behavior is undefined in case of concurrent operations with ``*this``.

    **Returns**: a reference to ``*this``.

assign
------

    .. code:: cpp

        void assign( size_type count, const value_type& value );

    Replaces all elements in ``*this`` by ``count`` copies of ``value``.

---------------------------------------------

    .. code:: cpp

        template <typename InputIterator>
        void assign( InputIterator first, InputIterator last );

    Replaces all elements in ``*this`` by the elements from the half-open interval ``[first, last)``.

    This overload only participates in overload resolution if the type ``InputIterator``
    meets the requirements of `InputIterator` from the [input.iterators] ISO C++ Standard section.

---------------------------------------------

    .. code:: cpp

        void assign( std::initializer_list<value_type> init );

    Equivalent to ``assign(init.begin(), init.end())``.

get_allocator
-------------

    .. code:: cpp

        allocator_type get_allocator() const;

    **Returns**: a copy of the allocator associated with ``*this``.
