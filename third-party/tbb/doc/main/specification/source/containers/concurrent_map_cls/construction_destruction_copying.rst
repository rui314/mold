.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==================================
Construction, destruction, copying
==================================

Empty container constructors
----------------------------

    .. code:: cpp

        concurrent_map();

        explicit concurrent_map( const key_compare& comp,
                                 const allocator_type& alloc = allocator_type() );

        explicit concurrent_map( const allocator_type& alloc );

    Constructs an empty ``concurrent_map``.

    If provided, uses the comparison function object ``comp`` for all ``key_type``
    comparisons and the allocator ``alloc`` to allocate the memory.

Constructors from the sequence of elements
------------------------------------------

    .. code:: cpp

        template <typename InputIterator>
        concurrent_map( InputIterator first, InputIterator last,
                        const key_compare& comp = key_compare(),
                        const allocator_type& alloc = allocator_type() );

        template <typename InputIterator>
        concurrent_map( InputIterator first, InputIterator last,
                        const allocator_type& alloc = allocator_type() );

    Constructs the ``concurrent_map``, which contains the elements from the half-open interval ``[first, last)``.

    If the range ``[first, last)`` contains multiple elements with equal keys, it is unspecified which element would be inserted.

    If provided, uses the comparison function object ``comp`` for all ``key_type`` comparisons and the allocator ``alloc`` to allocate the memory.

    **Requirements**: the type ``InputIterator`` must meet the requirements of `InputIterator` from the ``[input.iterators]`` ISO C++ Standard section.

------------------------------------------------------

    .. code:: cpp

        concurrent_map( std::initializer_list<value_type> init, const key_compare& comp = key_compare(),
                        const allocator_type& alloc = allocator_type() );

    Equivalent to ``concurrent_map(init.begin(), init.end(), comp, alloc)``.

------------------------------------------------------

    .. code:: cpp

        concurrent_map( std::initializer_list<value_type> init,
                        const allocator_type& alloc );

    Equivalent to ``concurrent_map(init.begin(), init.end(), alloc)``.

Copying constructors
--------------------

    .. code:: cpp

        concurrent_map( const concurrent_map& other );

        concurrent_map( const concurrent_map& other, const allocator_type& alloc );

    Constructs a copy of ``other``.

    If the allocator argument is not provided, it is obtained by calling
    ``std::allocator_traits<allocator_type>::select_on_container_copy_construction(other.get_allocator())``.

    The behavior is undefined in case of concurrent operations with ``other``.

Moving constructors
-------------------

    .. code:: cpp

        concurrent_map( concurrent_map&& other );

        concurrent_map( concurrent_map&& other, const allocator_type& alloc );

    Constructs a `concurrent_map` with the contents of ``other`` using move semantics.

    ``other`` is left in a valid, but unspecified state.

    If the allocator argument is not provided, it is obtained by calling ``std::move(other.get_allocator())``.

    The behavior is undefined in case of concurrent operations with ``other``.

Destructor
----------

    .. code:: cpp

        ~concurrent_map();

    Destroys the ``concurrent_map``. Calls destructors of the stored elements and
    deallocates the used storage.

    The behavior is undefined in case of concurrent operations with ``*this``.

Assignment operators
--------------------

    .. code:: cpp

        concurrent_map& operator=( const concurrent_map& other );

    Replaces all elements in ``*this`` by the copies of the elements in ``other``.

    Copy-assigns allocators if ``std::allocator_traits<allocator_type>::propagate_on_container_copy_assignment::value``
    is ``true``.

    The behavior is undefined in case of concurrent operations with ``*this`` and ``other``.

    **Returns**: a reference to ``*this``.

------------------------------------------------------

    .. code:: cpp

        concurrent_map& operator=( concurrent_map&& other );

    Replaces all elements in ``*this`` by the elements in ``other`` using move semantics.

    ``other`` is left in a valid, but unspecified state.

    Move-assigns allocators if ``std::allocator_traits<allocator_type>::propagate_on_container_move_assignment::value``
    is ``true``.

    The behavior is undefined in case of concurrent operations with ``*this`` and ``other``.

    **Returns**: a reference to ``*this``.

------------------------------------------------------

    .. code:: cpp

        concurrent_map& operator=( std::initializer_list<value_type> init );

    Replaces all elements in ``*this`` by the elements in ``init``.

    If ``init`` contains multiple elements with equal keys, it is unspecified which element would be inserted.

    The behavior is undefined in case of concurrent operations with ``*this``.

    **Returns**: a reference to ``*this``.
