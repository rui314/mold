.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=============================
Concurrently unsafe modifiers
=============================

All member functions in this section can only be performed serially.
The behavior is undefined in case of concurrent execution of these member functions
with other (either concurrently safe) methods.

clear
-----

    .. code:: cpp

        void clear();

    Removes all elements from the container.

swap
----

    .. code:: cpp

        void swap( concurrent_hash_map& other );

    Swaps contents of ``*this`` and ``other``.

    Swaps allocators if ``std::allocator_traits<allocator_type>::propagate_on_container_swap::value`` is ``true``.

    Otherwise, if ``get_allocator() != other.get_allocator()``, the behavior is undefined.
