.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==================================
Construction, destruction, copying
==================================

Empty container constructors
----------------------------

    .. code:: cpp

        concurrent_priority_queue();

        explicit concurrent_priority_queue( const allocator_type& alloc );

        explicit concurrent_priority_queue( const Compare& compare, const allocator_type& alloc );

    Constructs an empty ``concurrent_priority_queue``. The initial capacity is unspecified. If provided,
    uses the predicate ``compare`` for priority comparisons and the allocator ``alloc`` to allocate
    the memory.

    .. code:: cpp

        concurrent_priority_queue( size_type init_capacity,
                                   const allocator_type& alloc = allocator_type() );

        concurrent_priority_queue( size_type init_capacity,
                                   const Compare& compare,
                                   const allocator_type& alloc = allocator_type() );

    Constructs an empty ``concurrent_priority_queue`` with the initial capacity ``init_capacity``.
    If provided, uses the predicate ``compare`` for priority comparisons and the allocator ``alloc``
    to allocate the memory.

Constructors from the sequence of elements
------------------------------------------

    .. code:: cpp

        template <typename InputIterator>
        concurrent_priority_queue( InputIterator first, InputIterator last,
                                   const allocator_type& alloc = allocator_type() );

        template <typename InputIterator>
        concurrent_priority_queue( InputIterator first, InputIterator last,
                                   const Compare& compare,
                                   const allocator_type& alloc = allocator_type() );

    Constructs a ``concurrent_priority_queue`` containing all elements from the half-open interval
    ``[first, last)``.

    If provided, uses the predicate ``compare`` for priority comparisons and the allocator ``alloc``
    to allocate the memory.

    **Requirements**: the type ``InputIterator`` must meet the `InputIterator` requirements from the
    ``[input.iterators]`` ISO C++ Standard section.

    .. code:: cpp

        concurrent_priority_queue( std::initializer_list<value_type> init,
                                   const allocator_type& alloc = allocator_type() );

    Equivalent to ``concurrent_priority_queue(init.begin(), init.end(), alloc)``.

    .. code:: cpp

        concurrent_priority_queue( std::initializer_list<value_type> init,
                                   const Compare& compare,
                                   const allocator_type& alloc = allocator_type() );

    Equivalent to ``concurrent_priority_queue(init.begin(), init.end(), compare, alloc)``.

Copying constructors
--------------------

    .. code:: cpp

        concurrent_priority_queue( const concurrent_priority_queue& other );

        concurrent_priority_queue( const concurrent_priority_queue& other,
                                   const allocator_type& alloc );

    Constructs a copy of ``other``.

    If the allocator argument is not provided, it is obtained by 
    ``std::allocator_traits<allocator_type>::select_on_container_copy_construction(other.get_allocator())``.

    The behavior is undefined in case of concurrent operations with ``other``.

Moving constructors
-------------------

    .. code:: cpp

        concurrent_priority_queue( concurrent_priority_queue&& other );

        concurrent_priority_queue( concurrent_priority_queue&& other,
                                   const allocator_type& alloc );

    Constructs a copy of ``other`` using move semantics.

    ``other`` is left in a valid, but unspecified state.

    If the allocator argument is not provided, it is obtained by ``std::move(other.get_allocator())``.

    The behavior is undefined in case of concurrent operations with ``other``.

Destructor
----------

    .. code:: cpp

        ~concurrent_priority_queue();

    Destroys the ``concurrent_priority_queue``. Calls destructors of the stored elements and
    deallocates the used storage.

    The behavior is undefined in case of concurrent operations with ``*this``.

Assignment operators
--------------------

    .. code:: cpp

        concurrent_priority_queue& operator=( const concurrent_priority_queue& other );

    Replaces all elements in ``*this`` by the copies of the elements in ``other``.

    Copy-assigns allocators if ``std::allocator_traits<allocator_type>::propagate_on_container_copy_assignment::value``
    is ``true``.

    The behavior is undefined in case of concurrent operations with ``*this`` and ``other``.

    **Returns**: a reference to ``*this``.

    .. code:: cpp

        concurrent_priority_queue& operator=( concurrent_priority_queue&& other );

    Replaces all elements in ``*this`` by the elements in  ``other`` using move semantics.

    ``other`` is left in a valid, but unspecified state.

    Move-assigns allocators if ``std::allocator_traits<allocator_type>::propagate_on_container_move_assignment::value``
    is ``true``.

    The behavior is undefined in case of concurrent operations with ``*this`` and ``other``.

    **Returns**: a reference to ``*this``.

    .. code:: cpp

        concurrent_priority_queue& operator=( std::initializer_list<value_type> init );

    Replaces all elements in ``*this`` by the elements in ``init``.

    The behavior is undefined in case of concurrent operations with ``*this``.

    **Returns**: a reference to ``*this``.

assign
------

    .. code:: cpp

        template <typename InputIterator>
        void assign( InputIterator first, InputIterator last );

    Replaces all elements in ``*this`` be the elements in the half-open interval ``[first, last)``.

    The behavior is undefined in case of concurrent operations with ``*this``.

    **Requirements**: the type ``InputIterator`` must meet the `InputIterator` requirements from the
    ``[input.iterators]`` ISO C++ Standard section.

    .. code:: cpp

        void assign( std::initializer_list<value_type> init );

    Equivalent to ``assign(init.begin(), init.end())``.
