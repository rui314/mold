.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

======
Lookup
======

All methods in this section can be executed concurrently with each other,
concurrently-safe modifiers and while traversing the container.

count
-----

    .. code:: cpp

        size_type count( const key_type& key );

    **Returns**: the number of elements with the key equivalent to ``key``.

-----------------------------------------------------

    .. code:: cpp

        template <typename K>
        size_type count( const K& key );

    **Returns**: the number of elements with the key equivalent to ``key``.

    This overload only participates in overload resolution if qualified-id
    ``key_compare::is_transparent`` is valid and denotes a type.

find
----

    .. code:: cpp

        iterator find( const key_type& key );

        const_iterator find( const key_type& key ) const;

    **Returns**: an iterator to the element with the key equivalent to ``key``, or ``end()``
    if no such element exists.

    If there are multiple elements with the key equivalent to ``key``, it is unspecified which element should be found.

-----------------------------------------------------

    .. code:: cpp

        template <typename K>
        iterator find( const K& key );

        template <typename K>
        const_iterator find( const K& key ) const;

    **Returns**: an iterator to the element with the key that is equivalent to ``key``, or ``end()`` if no such element exists.

    If there are multiple elements with the key that is equivalent to ``key``,
    it is unspecified which element should be found.

    These overloads only participates in overload resolution if qualified-id
    ``key_compare::is_transparent`` is valid and denotes a type.

contains
--------

    .. code:: cpp

        bool contains( const key_type& key ) const;

    **Returns**: ``true`` if an element with the key equivalent to ``key`` exists
    in the container; ``false``, otherwise.

-----------------------------------------------------

    .. code:: cpp

        template <typename K>
        bool contains( const K& key ) const;

    **Returns**: ``true`` if an element with the key equivalent to ``key`` exists in the container; ``false``, otherwise.

    This overload only participates in overload resolution if qualified-id
    ``key_compare::is_transparent`` is valid and denotes a type.

lower_bound
-----------

    .. code:: cpp

        iterator lower_bound( const key_type& key );

        const_iterator lower_bound( const key_type& key ) const;

    **Returns**: an iterator to the first element in the container with the key
    that is `not less` than ``key``.

-----------------------------------------------------

    .. code:: cpp

        template <typename K>
        iterator lower_bound( const K& key )

        template <typename K>
        const_iterator lower_bound( const K& key ) const

    **Returns**: an iterator to the first element in the container with the key that
    is `not less` than ``key``.

    These overloads only participates in overload resolution if qualified-id
    ``key_compare::is_transparent`` is valid and denotes a type.

upper_bound
-----------

    .. code:: cpp

      iterator upper_bound( const key_type& key );

      const_iterator upper_bound( const key_type& key ) const;

    **Returns**: an iterator to the first element in the container with the key
    that compares `greater` than ``key``.

-----------------------------------------------------

    .. code:: cpp

      template <typename K>
      iterator upper_bound( const K& key );

      template <typename K>
      const_iterator upper_bound( const K& key ) const;

    **Returns**: an iterator to the first element in the container with the key
    that compares ``greater`` than ``key``.

    These overloads only participate in overload resolution if qualified-id
    ``key_compare::is_transparent`` is valid and denotes a type.

equal_range
-----------

    .. code:: cpp

        std::pair<iterator, iterator> equal_range( const key_type& key );

        std::pair<const_iterator, const_iterator> equal_range( const key_type& key ) const;

    **Returns**: if at least one element with the key equivalent to ``key`` exists, a pair of iterators ``{f, l}``,
    where ``f`` is an iterator to the first element with the key equivalent to ``key``,
    ``l`` is an iterator to the element that follows the last element with the key equivalent to ``key``.
    Otherwise - ``{end(), end()}``.

-----------------------------------------------------

    .. code:: cpp

        template <typename K>
        std::pair<iterator, iterator> equal_range( const K& key )

        template <typename K>
        std::pair<const_iterator, const_iterator> equal_range( const K& key )

    **Returns**: if at least one element with the key equivalent to ``key`` exists, a pair of iterators ``{f, l}``,
    where ``f`` is an iterator to the first element with the key that is equivalent to ``key``,
    ``l`` is an iterator to the element that follows the last element with the key that is
    equivalent to ``key``. Otherwise, ``{end(), end()}``.

    These overloads only participates in overload resolution if qualified-id
    ``key_compare::is_transparent`` is valid and denotes a type.
