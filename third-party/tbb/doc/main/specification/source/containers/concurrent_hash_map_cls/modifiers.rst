.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

===========================
Concurrently safe modifiers
===========================

All methods in this section can be executed concurrently with each other
and lookup methods.

Inserting values
----------------

    .. code:: cpp

        bool insert( const_accessor& result, const key_type& key );

        bool insert( accessor& result, const key_type& key );

    If the ``result`` accessor is not empty, releases the ``result`` and
    attempts to insert the value constructed from ``key, mapped_type()`` into the container.

    Sets the ``result`` to provide access to the inserted element or to the element with equal key,
    which was already presented in the container.

    **Requirements**:

    * the ``value_type`` type must meet the ``EmplaceConstructible`` requirements described in the [container.requirements] section of the ISO C++ Standard.
    * the ``mapped_type`` type must meet the ``DefaultConstructible`` requirements described in the [defaultconstructible] section of the ISO C++ Standard.

    **Returns**: ``true`` if an element is inserted; ``false`` otherwise.

--------------------------

    .. code:: cpp

        template <typename K>
        bool insert( const_accessor& result, const K& key );

        template <typename K>
        bool insert( accessor& result, const K& key );

    If the ``result`` accessor is not empty, releases the ``result`` and
    attempts to insert the value constructed from ``key, mapped_type()`` into the container.

    Sets the ``result`` to provide access to the inserted element or to the element with the key,
    that compares equivalent to the value ``key``, which was already presented in the container.

    This overload only participates in the overload resolution if:

    * qualified-id ``hash_compare_type::is_transparent`` is valid and denotes a type
    * ``std::is_constructible<key_type, const K&>::value`` is ``true``

    **Requirements**: the ``mapped_type`` type must meet the ``DefaultConstructible`` requirements
    described in the [defaultconstructible] section of the ISO C++ Standard.

    **Returns**: ``true`` if an element is inserted; ``false`` otherwise.

--------------------------

    .. code:: cpp

        bool insert( const_accessor& result, const value_type& value );

        bool insert( accessor& result, const value_type& value );

    If the ``result`` accessor is not empty, releases the ``result`` and
    attempts to insert the value ``value`` into the container.

    Sets the ``result`` to provide access to the inserted element or to the element with equal key,
    which was already presented in the container.

    **Requirements**: the ``value_type`` type must meet the ``CopyInsertable`` requirements described in the
    [container.requirements] section of the ISO C++ Standard.

    **Returns**: ``true`` if an element is inserted; ``false`` otherwise.

--------------------------

    .. code:: cpp

        bool insert( const value_type& value );

    Attempts to insert the value ``value`` into the container.

    **Requirements**: the ``value_type`` type must meet the ``CopyInsertable`` requirements described in the
    [container.requirements] section of the ISO C++ Standard.

    **Returns**: ``true`` if an element is inserted; ``false`` otherwise.

--------------------------

    .. code:: cpp

        bool insert( const_accessor& result, value_type&& value );

        bool insert( accessor& result, value_type&& value );

    If the ``result`` accessor is not empty, releases the ``result`` and
    attempts to insert the value ``value`` into the container using move semantics.

    Sets the ``result`` to provide access to the inserted element or to the element with equal key,
    which was already presented in the container.

    ``value`` is left in a valid, but unspecified state.

    **Requirements**: the ``value_type`` type must meet the ``MoveInsertable`` requirements described in the
    [container.requirements] section of the ISO C++ Standard.

    **Returns**: ``true`` if an element is inserted; ``false`` otherwise.

--------------------------

    .. code:: cpp

        bool insert( value_type&& value );

    Attempts to insert the value ``value`` into the container using move semantics.

    **Requirements**: the ``value_type`` type must meet the ``MoveInsertable`` requirements described in the
    [container.requirements] section of the ISO C++ Standard.

    **Returns**: ``true`` if an element is inserted; ``false`` otherwise.

Inserting sequences of elements
-------------------------------

    .. code:: cpp

        template <typename InputIterator>
        void insert( InputIterator first, InputIterator last );

    Attempts to insert all items from the half-open interval ``[first, last)``
    into the container.

    If the interval ``[first, last)`` contains multiple elements with equal keys,
    it is unspecified which element should be inserted.

    **Requirements**: the ``InputIterator`` type must meet the requirements of `InputIterator`
    described in the ``[input.iterators]`` section of the ISO C++ Standard.

--------------------------

    .. code:: cpp

        void insert( std::initializer_list<value_type> init );

    Equivalent to ``insert(init.begin(), init.end())``.

Emplacing elements
------------------

    .. code:: cpp

        template <typename... Args>
        bool emplace( const_accessor& result, Args&&... args );

        template <typename... Args>
        bool emplace( accessor& result, Args&&... args );

    If the ``result`` accessor is not empty, releases the ``result`` and
    attempts to insert an element constructed in-place from ``args`` into the container.

    Sets the ``result`` to provide access to the inserted element or to the element with equal key,
    which was already presented in the container.

    **Requirements**: the type ``value_type`` must meet the ``EmplaceConstructible`` requirements described in the
    [container.requirements] section of the ISO C++ Standard.

    **Returns**: ``true`` if an element is inserted; ``false`` otherwise

--------------------------

    .. code:: cpp

        template <typename... Args>
        bool emplace( Args&&... args );

    Attempts to insert an element constructed in-place from ``args`` into the container.

    **Requirements**: the type ``value_type`` must meet the ``EmplaceConstructible`` requirements described in the
    [container.requirements] section of the ISO C++ Standard.

    **Returns**: ``true`` if an element is inserted; ``false`` otherwise

Erasing elements
----------------

    .. code:: cpp

        bool erase( const key_type& key );

    If an element with the key equivalent to  ``key`` exists, removes it from the container.

    **Returns**: ``true`` if an element is removed; ``false`` otherwise.

--------------------------

    .. code:: cpp

        template <typename K>
        bool erase( const K& key );

    If an element with the key that compares equivalent to the value ``key`` exists, removes it from the container.

    This overload only participates in the overload resolution if qualified-id
    ``hash_compare_type::is_transparent`` is valid and denotes a type.

    **Returns**: ``true`` if an element is removed; ``false`` otherwise.

--------------------------

    .. code:: cpp

        bool erase( const_accessor& item_accessor );
        bool erase( accessor& item_accessor );

    Removes an element owned by ``item_accessor`` from the container.

    **Requirements**: ``item_accessor`` should not be empty.

    **Returns**: ``true`` if an element is removed by the current thread; ``false``
    if it is removed by another thread.
