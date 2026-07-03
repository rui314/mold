.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

======
Lookup
======

All methods in this section can be executed concurrently with each other
and concurrently-safe modifiers.

find
----

    .. code:: cpp

        bool find( const_accessor& result, const key_type& key ) const;

        bool find( accessor& result, const key_type& key );

    If the ``result`` accessor is not empty, releases the ``result``.

    If an element with the key that is equivalent to ``key`` exists, sets the ``result`` to provide access
    to this element.

    **Returns**: ``true`` if an element with the key equivalent to  ``key`` is found; ``false`` otherwise.

--------------------------

    .. code:: cpp

        template <typename K>
        bool find( const_accessor& result, const K& key ) const;

        template <typename K>
        bool find( accessor& result, const K& key );

    If the ``result`` accessor is not empty, releases the ``result``.

    If an element with the key that compares equivalent to the value ``key`` exists, sets the ``result`` to provide access
    to this element.

    **Returns**: ``true`` if an element with the key that compares equivalent to the value ``key`` is found; ``false`` otherwise.

    This overload only participates in the overload resolution if qualified-id
    ``hash_compare_type::is_transparent`` is valid and denotes a type.

count
-----

    .. code:: cpp

        size_type count( const key_type& key ) const;

    **Returns**: ``1`` if an element with the key equivalent to ``key`` exists; ``0`` otherwise.

--------------------------

    .. code:: cpp

        template <typename K>
        size_type count( const K& key ) const;

    **Returns**: ``1`` if an element with the key that compares equivalent to the value ``key`` exists;
    ``0`` otherwise.

    This overload only participates in the overload resolution if qualified-id
    ``hash_compare_type::is_transparent`` is valid and denotes a type.
