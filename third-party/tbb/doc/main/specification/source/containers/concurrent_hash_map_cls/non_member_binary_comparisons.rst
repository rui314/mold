.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=============================
Non-member binary comparisons
=============================

Two objects of ``concurrent_hash_map`` are equal if the following conditions are ``true``:

* They contain equal number of elements.
* Each element from one container is also available in the other.

.. code:: cpp

    template <typename Key, typename T, typename HashCompare, typename Allocator>
    bool operator==( const concurrent_hash_map<Key, T, HashCompare, Allocator>& lhs,
                     const concurrent_hash_map<Key, T, HashCompare, Allocator>& rhs );

**Returns**: ``true`` if ``lhs`` is  equivalent to  ``rhs``; ``false``, otherwise.

-----------------------------------------------------------------------------

.. code:: cpp

    template <typename Key, typename T, typename HashCompare, typename Allocator>
    bool operator!=( const concurrent_hash_map<Key, T, HashCompare, Allocator>& lhs,
                     const concurrent_hash_map<Key, T, HashCompare, Allocator>& rhs );

Equivalent to ``!(lhs == rhs)``.

**Returns**: ``true`` if ``lhs`` is not equal to ``rhs``; ``false``, otherwise.
