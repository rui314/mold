.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

===========================
Concurrently safe modifiers
===========================

All member functions in this section can be performed concurrently with each other,
lookup methods and while traversing the container.

Inserting values
----------------

    .. code:: cpp

        std::pair<iterator, bool> insert( const value_type& value );

    Attempts to insert the value ``value`` into the container.

    **Returns**: ``std::pair<iterator, bool``, where ``iterator`` points to the inserted element
    or to an existing element with equal key. Boolean value is ``true`` if insertion took place;
    ``false``, otherwise.

    **Requirements**: the type ``value_type`` must meet the ``CopyInsertable`` requirements from the
    [container.requirements] ISO C++ Standard section.

-----------------------------------------------------

    .. code:: cpp

        iterator insert( const_iterator hint, const value_type& other );

    Attempts to insert the value ``value`` into the container.

    Optionally uses the parameter ``hint`` as a suggestion to where the element should be placed.

    **Returns**: an ``iterator`` to the inserted element or to an existing element with equal key.

    **Requirements**: the type ``value_type`` must meet the ``CopyInsertable`` requirements from the
    [container.requirements] ISO C++ Standard section.

-----------------------------------------------------

    .. code:: cpp

        std::pair<iterator, bool> insert( value_type&& value );

    Attempts to insert the value ``value`` into the container using move semantics.

    ``value`` is left in a valid, but unspecified state.

    **Returns**: ``std::pair<iterator, bool``, where ``iterator`` points to the inserted
    element or to an existing element with equal key. Boolean value is ``true``
    if insertion took place; ``false``, otherwise.

    **Requirements**: the type ``value_type`` must meet the ``MoveInsertable`` requirements from the
    [container.requirements] ISO C++ Standard section.

-----------------------------------------------------

    .. code:: cpp

        iterator insert( const_iterator hint, value_type&& other );

    Attempts to insert the value ``value`` into the container using move semantics.

    Optionally uses the parameter ``hint`` as a suggestion to where the element should be placed.

    ``value`` is left in a valid, but unspecified state.

    **Returns**: an ``iterator`` to the inserted element or to an existing element with equal key.

    **Requirements**: the type ``value_type`` must meet the ``MoveInsertable`` requirements from the
    [container.requirements] ISO C++ Standard section.

Inserting sequences of elements
-------------------------------

    .. code:: cpp

        template <typename InputIterator>
        void insert( InputIterator first, InputIterator last );

    Attempts to insert all items from the half-open interval ``[first, last)``
    into the container.

    If the interval ``[first, last)`` contains multiple equal elements,
    it is unspecified which element should be inserted.

    **Requirements**: the type ``InputIterator`` must meet the requirements of `InputIterator`
    from the ``[input.iterators]`` ISO C++ Standard section.

-----------------------------------------------------

    .. code:: cpp

        void insert( std::initializer_list<value_type> init );

    Equivalent to ``insert(init.begin(), init.end())``.

Inserting nodes
---------------

    .. code:: cpp

        std::pair<iterator, bool> insert( node_type&& nh );

    If the node handle ``nh`` is empty, does nothing.

    Otherwise, attempts to insert the node owned by ``nh`` into the container.

    If the insertion fails, node handle ``nh`` keeps ownership of the node.

    Otherwise, ``nh`` is left in an empty state.

    No copy or move constructors of ``value_type`` are performed.

    The behavior is undefined if ``nh`` is not empty and ``get_allocator() != nh.get_allocator()``.

    **Returns**: ``std::pair<iterator, bool``, where ``iterator`` points to the
    inserted element or to an existing element equal to
    ``nh.value()``. Boolean value is ``true`` if insertion took place; ``false``, otherwise.

-----------------------------------------------------

    .. code:: cpp

        iterator insert( const_iterator hint, node_type&& nh );

    If the node handle ``nh`` is empty, does nothing.

    Otherwise, attempts to insert the node owned by ``nh`` into the container.

    Optionally uses the parameter ``hint`` as a suggestion to where the node should be placed.

    If the insertion fails, node handle ``nh`` remains ownership of the node.

    Otherwise, ``nh`` is left in an empty state.

    No copy or move constructors of ``value_type`` are performed.

    The behavior is undefined if ``nh`` is not empty and ``get_allocator() != nh.get_allocator()``.

    **Returns**: an iterator pointing to the inserted element or to an existing element
    equal to ``nh.value()``.

Emplacing elements
------------------

    .. code:: cpp

        template <typename... Args>
        std::pair<iterator, bool> emplace( Args&&... args );

    Attempts to insert an element constructed in-place from ``args`` into the container.

    **Returns**: ``std::pair<iterator, bool``, where ``iterator`` points to the inserted element
    or to an existing element with equal key. Boolean value is ``true`` if insertion took place;
    ``false``, otherwise.

    **Requirements**: the type ``value_type`` must meet the ``EmplaceConstructible`` requirements
    from the [container.requirements] ISO C++ section.

-----------------------------------------------------

    .. code:: cpp

        template <typename... Args>
        iterator emplace_hint( const_iterator hint, Args&&... args );

    Attempts to insert an element constructed in-place from ``args`` into the container.

    Optionally uses the parameter ``hint`` as a suggestion to where the node should be placed.

    **Returns**: an ``iterator`` to the inserted element or to an existing element with equal key.

    **Requirements**: the type ``value_type`` must meet the ``EmplaceConstructible`` requirements
    from the [container.requirements] ISO C++ section.

Merging containers
------------------

    .. code:: cpp

        template <typename SrcCompare>
        void merge( concurrent_set<T, SrcCompare, Allocator>& source );

        template <typename SrcCompare>
        void merge( concurrent_set<T, SrcCompare, Allocator>&& source );

        template <typename SrcCompare>
        void merge( concurrent_multiset<T, SrcCompare, Allocator>& source );

        template <typename SrcCompare>
        void merge( concurrent_multiset<T, SrcCompare, Allocator>&& source );

    Transfers those elements from ``source`` which keys do not exist in the container.

    In case of merging with the container with multiple equal elements,
    it is unspecified which element would be transferred.

    No copy or move constructors of ``value_type`` are performed.

    The behavior is undefined if ``get_allocator() != source.get_allocator()``.
