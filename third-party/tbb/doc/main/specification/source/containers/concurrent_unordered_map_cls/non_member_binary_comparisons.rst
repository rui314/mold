.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=============================
Non-member binary comparisons
=============================

Two objects of ``concurrent_unordered_map`` are equal if the following conditions are ``true``:

* They contains an equal number of elements.
* Each element from the one container is also available in the other.

.. code:: cpp

    template <typename Key, typename T, typename Hash,
              typename KeyEqual, typename Allocator>
    bool operator==( const concurrent_unordered_map<Key, T, Hash, KeyEqual, Allocator>& lhs,
                     const concurrent_unordered_map<Key, T, Hash, KeyEqual, Allocator>& rhs );

**Returns**: ``true`` if ``lhs`` is equal to ``rhs``; ``false``, otherwise.

---------------------------------------------------------------------------------------------

.. code:: cpp

    template <typename Key, typename T, typename Hash,
              typename KeyEqual, typename Allocator>
    bool operator!=( const concurrent_unordered_map<Key, T, Hash, KeyEqual, Allocator>& lhs,
                     const concurrent_unordered_map<Key, T, Hash, KeyEqual, Allocator>& rhs );

Equivalent to ``!(lhs == rhs)``.

**Returns**: ``true`` if ``lhs`` is not equal to ``rhs``; ``false``, otherwise.
