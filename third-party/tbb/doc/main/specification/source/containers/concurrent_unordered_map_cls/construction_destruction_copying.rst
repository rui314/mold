.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==================================
Construction, destruction, copying
==================================

Empty container constructors
----------------------------

    .. code:: cpp

        concurrent_unordered_map();

        explicit concurrent_unordered_map( const allocator_type& alloc );

    Constructs an empty ``concurrent_unordered_map``. The initial number of
    buckets is unspecified.

    If provided, uses the allocator ``alloc`` to allocate the memory.

---------------------------------------------------------------------------------------------

    .. code:: cpp

        explicit concurrent_unordered_map( size_type bucket_count,
                                           const hasher& hash = hasher(),
                                           const key_equal& equal = key_equal(),
                                           const allocator_type& alloc = allocator_type() );

        concurrent_unordered_map( size_type bucket_count, const allocator_type& alloc );

        concurrent_unordered_map( size_type bucket_count, const hasher& hash,
                                  const allocator_type& alloc );

    Constructs an empty ``concurrent_unordered_map`` with ``bucket_count`` buckets.

    If provided, uses the hash function ``hasher``, predicate ``equal`` to compare ``key_type``
    objects for equality, and the allocator ``alloc`` to allocate the memory.

Constructors from the sequence of elements
------------------------------------------

    .. code:: cpp

        template <typename InputIterator>
        concurrent_unordered_map( InputIterator first, InputIterator last,
                                  size_type bucket_count = /*implementation-defined*/,
                                  const hasher& hash = hasher(),
                                  const key_equal& equal = key_equal(),
                                  const allocator_type& alloc = allocator_type() );

        template <typename Inputiterator>
        concurrent_unordered_map( InputIterator first, InputIterator last,
                                  size_type bucket_count, const allocator_type& alloc );

        template <typename InputIterator>
        concurrent_unordered_map( InputIterator first, InputIterator last,
                                  size_type bucket_count, const hasher& hash,
                                  const allocator_type& alloc );

    Constructs the ``concurrent_unordered_map`` that contains the elements from the half-open
    interval ``[first, last)``.

    If the range ``[first, last)`` contains multiple elements with equal keys, it is unspecified which
    element would be inserted.

    If provided, uses the hash function ``hasher``, predicate ``equal`` to compare ``key_type``
    objects for equality, and the allocator ``alloc`` to allocate the memory.

    **Requirements**: the type ``InputIterator`` must meet the requirements of ``InputIterator``
    from the [input.iterators] ISO C++ Standard section.

---------------------------------------------------------------------------------------------

    .. code:: cpp

        concurrent_unordered_map( std::initializer_list<value_type> init,
                                  size_type bucket_count = /*implementation-defined*/,
                                  const hasher& hash = hasher(),
                                  const key_equal& equal = key_equal(),
                                  const allocator_type& alloc = allocator_type() );

    Equivalent to ``concurrent_unordered_map(init.begin(), init.end(), bucket_count, hash, equal, alloc)``.

---------------------------------------------------------------------------------------------

    .. code:: cpp

        concurrent_unordered_map( std::initializer_list<value_type> init,
                                  size_type bucket_count, const allocator_type& alloc );

    Equivalent to ``concurrent_unordered_map(init.begin(), init.end(), bucket_count, alloc)``.

---------------------------------------------------------------------------------------------

    .. code:: cpp

        concurrent_unordered_map( std::initializer_list<value_type> init,
                                  size_type bucket_count, const hasher& hash,
                                  const allocator_type& alloc );

    Equivalent to ``concurrent_unordered_map(init.begin(), init.end(), bucket_count, hash, alloc)``.

Copying constructors
--------------------

    .. code:: cpp

        concurrent_unordered_map( const concurrent_unordered_map& other );

        concurrent_unordered_map( const concurrent_unordered_map& other,
                                  const allocator_type& alloc );

    Constructs a copy of ``other``.

    If the allocator argument is not provided, it is obtained by calling
    ``std::allocator_traits<allocator_type>::select_on_container_copy_construction(other.get_allocator())``.

    The behavior is undefined in case of concurrent operations with ``other``.

Moving constructors
-------------------

    .. code:: cpp

        concurrent_unordered_map( concurrent_unordered_map&& other );

        concurrent_unordered_map( concurrent_unordered_map&& other,
                                  const allocator_type& alloc );

    Constructs a ``concurrent_unordered_map`` with the contents of ``other`` using move semantics.

    ``other`` is left in a valid, but unspecified state.

    If the allocator argument is not provided, it is obtained by calling ``std::move(other.get_allocator())``.

    The behavior is undefined in case of concurrent operations with ``other``.

Destructor
----------

    .. code:: cpp

        ~concurrent_unordered_map();

    Destroys the ``concurrent_unordered_map``. Calls destructors of the stored elements and
    deallocates the used storage.

    The behavior is undefined in case of concurrent operations with ``*this``.

Assignment operators
--------------------

    .. code:: cpp

        concurrent_unordered_map& operator=( const concurrent_unordered_map& other );

    Replaces all elements in ``*this`` by the copies of the elements in ``other``.

    Copy-assigns allocators if ``std::allocator_traits<allocator_type>::propagate_on_container_copy_assignment::value``
    is ``true``.

    The behavior is undefined in case of concurrent operations with ``*this`` and ``other``.

    **Returns**: a reference to ``*this``.

---------------------------------------------------------------------------------------------

    .. code:: cpp

        concurrent_unordered_map& operator=( concurrent_unordered_map&& other ) noexcept(/*See below*/);

    Replaces all elements in ``*this`` by the elements in ``other`` using move semantics.

    ``other`` is left in a valid, but unspecified state.

    Move-assigns allocators if ``std::allocator_traits<allocator_type>::propagate_on_container_move_assignment::value``
    is ``true``.

    The behavior is undefined in case of concurrent operations with ``*this`` and ``other``.

    **Returns**: a reference to ``*this``.

    **Exceptions**: ``noexcept`` specification:

        .. code:: cpp

            noexcept(std::allocator_traits<allocator_type>::is_always_equal::value &&
                     std::is_nothrow_move_assignable<hasher>::value &&
                     std::is_nothrow_move_assignable<key_equal>::value)

---------------------------------------------------------------------------------------------

    .. code:: cpp

        concurrent_unordered_map& operator=( std::initializer_list<value_type> init );

    Replaces all elements in ``*this`` by the elements in ``init``.

    If ``init`` contains multiple elements with equal keys, it is unspecified which element would be inserted.

    The behavior is undefined in case of concurrent operations with ``*this``.

    **Returns**: a reference to ``*this``.
