.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==================================
Construction, destruction, copying
==================================

Empty container constructors
----------------------------

    .. code:: cpp

        concurrent_hash_map();

        explicit concurrent_hash_map( const hash_compare_type& compare,
                                      const allocator_type& alloc = allocator_type() );

        explicit concurrent_hash_map( const allocator_type& alloc );

    Constructs an empty ``concurrent_hash_map``. The initial number of
    buckets is unspecified.

    If provided, uses the comparator ``compare`` to calculate hash codes and compare ``key_type`` objects
    for equality and the allocator ``alloc`` to allocate the memory.

-------------------------------

    .. code:: cpp

        concurrent_hash_map( size_type n, const hash_compare_type& compare,
                             const allocator_type& alloc = allocator_type() );

        concurrent_hash_map( size_type n, const allocator_type& alloc = allocator_type() );

    Constructs an empty ``concurrent_hash_map`` with ``n`` preallocated buckets.

    If provided, uses the comparator ``compare`` to calculate hash codes and compare ``key_type`` objects
    for equality and the allocator ``alloc`` to allocate the memory.

Constructors from the sequence of elements
------------------------------------------

    .. code:: cpp

        template <typename InputIterator>
        concurrent_hash_map( InputIterator first, InputIterator last,
                             const hash_compare_type& compare,
                             const allocator_type& alloc = allocator_type() );

        template <typename InputIterator>
        concurrent_hash_map( InputIterator first, InputIterator last,
                             const allocator_type& alloc = allocator_type() );

    Constructs the ``concurrent_hash_map`` which contains the elements from the half-open
    interval ``[first, last)``.

    If the range ``[first, last)`` contains multiple elements with equal keys, it is unspecified which
    element would be inserted.

    If provided, uses the comparator ``compare`` to calculate hash codes and compare ``key_type`` objects
    for equality and the allocator ``alloc`` to allocate the memory.

    **Requirements**: the type ``InputIterator`` must meet the requirements of ``InputIterator``
    from the [input.iterators] ISO C++ Standard section.


-------------------------------

    .. code:: cpp

        concurrent_hash_map( std::initializer_list<value_type> init,
                             const hash_compare_type& compare = hash_compare_type(),
                             const allocator_type& alloc = allocator_type() );

    Equivalent to ``concurrent_hash_map(init.begin(), init.end(), compare, alloc)``.

-------------------------------

    .. code:: cpp

        concurrent_hash_map( std::initializer_list<value_type> init,
                             const allocator_type& alloc );

    Equivalent to ``concurrent_hash_map(init.begin(), init.end(), alloc)``.

Copying constructors
--------------------

    .. code:: cpp

        concurrent_hash_map( const concurrent_hash_map& other );

        concurrent_hash_map( const concurrent_hash_map& other,
                             const allocator_type& alloc );

    Constructs a copy of ``other``.

    If the allocator argument is not provided, it is obtained by calling
    ``std::allocator_traits<allocator_type>::select_on_container_copy_construction(other.get_allocator())``.

    The behavior is undefined in case of concurrent operations with ``other``.

Moving constructors
-------------------

    .. code:: cpp

        concurrent_hash_map( concurrent_hash_map&& other );

        concurrent_hash_map( concurrent_hash_map&& other,
                             const allocator_type& alloc );

    Constructs a ``concurrent_hash_map`` with the content of ``other`` using move semantics.

    ``other`` is left in a valid, but unspecified state.

    If the allocator argument is not provided, it is obtained by calling ``std::move(other.get_allocator())``.

    The behavior is undefined in case of concurrent operations with ``other``.

Destructor
----------

    .. code:: cpp

        ~concurrent_hash_map();

    Destroys the ``concurrent_hash_map``. Calls destructors of the stored elements and
    deallocates the used storage.

    The behavior is undefined in case of concurrent operations with ``*this``.

Assignment operators
--------------------

    .. code:: cpp

        concurrent_hash_map& operator=( const concurrent_hash_map& other );

    Replaces all elements in ``*this`` by the copies of the elements in ``other``.

    Copy-assigns allocators if ``std::allocator_traits<allocator_type>::propagate_on_container_copy_assignment::value``
    is ``true``.

    The behavior is undefined in case of concurrent operations with ``*this`` and ``other``.

    **Returns**: a reference to ``*this``.

-------------------------------

    .. code:: cpp

        concurrent_hash_map& operator=( concurrent_hash_map&& other );

    Replaces all elements in ``*this`` by the elements in ``other`` using move semantics.

    ``other`` is left in a valid, but unspecified state.

    Move-assigns allocators if ``std::allocator_traits<allocator_type>::propagate_on_container_move_assignment::value``
    is ``true``.

    The behavior is undefined in case of concurrent operations with ``*this`` and ``other``.

    **Returns**: a reference to ``*this``.

-------------------------------

    .. code:: cpp

        concurrent_hash_map& operator=( std::initializer_list<value_type> init );

    Replaces all elements in ``*this`` by the elements in ``init``.

    If ``init`` contains multiple elements with equal keys, it is unspecified which element is inserted.

    The behavior is undefined in case of concurrent operations with ``*this``.

    **Returns**: a reference to ``*this``.

get_allocator
-------------

    .. code:: cpp

        allocator_type get_allocator() const;

    **Returns**: a copy of the allocator associated with ``*this``.
