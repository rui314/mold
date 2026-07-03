.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=============================
Concurrently unsafe modifiers
=============================

All member functions in this section can only be performed serially.
The behavior is undefined in case of concurrent execution of these member functions
with other (either concurrently safe) methods.

Clearing
--------

    .. code:: cpp

       void clear();

    Removes all elements from the container.

Erasing elements
----------------

    .. code:: cpp

        iterator unsafe_erase( const_iterator pos );

        iterator unsafe_erase( iterator pos );

    Removes the element pointed to by ``pos`` from the container.

    Invalidates all iterators and references to the removed element.

    **Returns**: ``iterator`` that follows the removed element.

    **Requirements**: the iterator ``pos`` should be valid, dereferenceable
    and point to the element in ``*this``.

---------------------------------------------------------------------------------------------

    .. code:: cpp

        size_type unsafe_erase( const key_type& key );

    Removes the element equivalent to ``key`` if it exists in the container.

    Invalidates all iterators and references to the removed element.

    **Returns**: the number of removed elements.

---------------------------------------------------------------------------------------------

    .. code:: cpp

        template <typename K>
        size_type unsafe_erase( const K& key );

    Removes the element that is equivalent to ``key``
    if it exists in the container.

    Invalidates all iterators and references to the removed element.

    This overload only participates in overload resolution if all of the following conditions are met:

    - The qualified-id ``hasher::transparent_key_equal`` is valid and denotes a type.
    - ``std::is_convertible<K, iterator>::value`` is ``false``.
    - ``std::is_convertible<K, const_iterator>::value`` is ``false``.

    **Returns**: the number of removed elements.

Erasing sequences
-----------------

    .. code:: cpp

        iterator unsafe_erase( const_iterator first, const_iterator last );

    Removes all elements from the half-open interval ``[first, last)`` from the container.

    **Returns**: ``iterator`` that follows the last removed element.

    **Requirements**: the range ``[first, last)`` must be a valid subrange in ``*this``.

Extracting nodes
----------------

    .. code:: cpp

        node_type unsafe_extract( iterator pos );

        node_type unsafe_extract( const_iterator pos );

    Transfers ownership of the element pointed to by ``pos`` from the container to the node handle.

    No copy or move constructors of ``value_type`` are performed.

    Invalidates all iterators to the extracted element. Pointers and references to the extracted element remain valid.

    **Returns**: the node handle that owns the extracted element.

    **Requirements**: the iterator ``pos`` should be valid, dereferenceable
    and point to the element in ``*this``.

---------------------------------------------------------------------------------------------

    .. code:: cpp

        node_type unsafe_extract( const key_type& key );

    If an element equivalent to ``key`` exists, transfers ownership of this element
    from the container to the node handle.

    No copy or move constructors of ``value_type`` are performed.

    If there are multiple elements equivalent to ``key``,
    it is unspecified which element should be transferred.

    Invalidates all iterators to the extracted element.
    Pointers and references to the extracted element remain valid.

    **Returns**: the node handle that owns the extracted element or an empty node handle
    if an element equivalent to ``key`` was not found.

---------------------------------------------------------------------------------------------

    .. code:: cpp

        template <typename K>
        node_type unsafe_extract( const K& key );

    If an element equivalent to ``key`` exists,
    transfers ownership of this element from the container to the node handle.

    No copy or move constructors of ``value_type`` are performed.

    If there are multiple elements which are equivalent to ``key``,
    it is unspecified which element should be transferred.

    Invalidates all iterators to the extracted element. Pointers and references to the
    extracted element remain valid.

    This overload only participates in overload resolution if all of the following conditions are met:

    - The qualified-id ``hasher::transparent_key_equal`` is valid and denotes a type.
    - ``std::is_convertible<K, iterator>::value`` is ``false``.
    - ``std::is_convertible<K, const_iterator>::value`` is ``false``.

    **Returns**: the node handle that owns the extracted element or an empty node handle if
    an element equivalent to ``key`` was not found.

swap
----

    .. code:: cpp

        void swap( concurrent_unordered_multiset& other ) noexcept(/*See below*/);

    Swaps contents of ``*this`` and ``other``.

    Swaps allocators if ``std::allocator_traits<allocator_type>::propagate_on_container_swap::value`` is ``true``.

    Otherwise, if ``get_allocator() != other.get_allocator()``, the behavior is undefined.

    **Exceptions**: ``noexcept`` specification:

        .. code:: cpp

            noexcept(std::allocator_traits<allocator_type>::is_always_equal::value &&
                     std::is_nothrow_swappable<hasher>::value &&
                     std::is_nothrow_swappable<key_equal>::value
