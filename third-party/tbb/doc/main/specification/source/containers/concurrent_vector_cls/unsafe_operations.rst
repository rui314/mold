.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==============================
Concurrently unsafe operations
==============================

All member functions in this section can only be performed serially.
The behavior is undefined in case of concurrent execution of these member functions
with other (either concurrently safe) methods.

Reserving
---------

    .. code:: cpp

        void reserve( size_type n );

    Reserves memory for at least ``n`` elements.

    **Throws**: ``std::length_error`` if ``n > max_size()``.

Resizing
--------

    .. code:: cpp

        void resize( size_type n );

    If ``n < size()``, the vector is reduced to its first ``n`` elements.

    Otherwise, appends ``n - size()`` new elements default-constructed in-place to
    the end of the vector.

---------------------------------------------

    .. code:: cpp

        void resize( size_type n, const value_type& value );

    If ``n < size()``, the vector is reduced to its first ``n`` elements.

    Otherwise, appends ``n - size()`` copies of ``value`` to
    the end of the vector.

shrink_to_fit
-------------

    .. code:: cpp

        void shrink_to_fit();

    Removes the unused capacity of the vector.

    Call for this method can also reorganize the internal vector representation in the memory.

clear
-----

    .. code:: cpp

        void clear();

    Removes all elements from the container.

swap
----

    .. code:: cpp

        void swap( concurrent_vector& other ) noexcept(/*See below*/);

    Swaps contents of ``*this`` and ``other``.

    Swaps allocators if ``std::allocator_traits<allocator_type>::propagate_on_container_swap::value`` is ``true``.

    Otherwise, if ``get_allocator() != other.get_allocator()``, the behavior is undefined.

    **Exceptions**: ``noexcept`` specification:

        .. code:: cpp

            noexcept(std::allocator_traits<allocator_type>::propagate_on_container_swap::value ||
                     std::allocator_traits<allocator_type>::is_always_equal::value
