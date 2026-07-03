.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

======================================
Non-member lexicographical comparisons
======================================

.. code:: cpp

    template <typename T, typename Allocator>
    bool operator<( const concurrent_vector<T, Allocator>& lhs,
                    const concurrent_vector<T, Allocator>& rhs );

**Returns**: ``true`` if ``lhs`` is lexicographically `less` than ``rhs``; ``false``, otherwise.

.. code:: cpp

    template <typename T, typename Allocator>
    bool operator<=( const concurrent_vector<T, Allocator>& lhs,
                     const concurrent_vector<T, Allocator>& rhs );

**Returns**: ``true`` if ``lhs`` is lexicographically `less or equal` than ``rhs``; ``false``, otherwise.

.. code:: cpp

    template <typename T, typename Allocator>
    bool operator>( const concurrent_vector<T, Allocator>& lhs,
                    const concurrent_vector<T, Allocator>& rhs );

**Returns**: ``true`` if ``lhs`` is lexicographically `greater` than ``rhs``; ``false``, otherwise.

.. code:: cpp

    template <typename T, typename Allocator>
    bool operator>=( const concurrent_vector<T, Allocator>& lhs,
                     const concurrent_vector<T, Allocator>& rhs );

**Returns**: ``true`` if ``lhs`` is lexicographically `greater or equal` than ``rhs``; ``false``, otherwise.
