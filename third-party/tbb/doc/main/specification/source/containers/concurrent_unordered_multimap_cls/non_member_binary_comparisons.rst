.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=============================
Non-member binary comparisons
=============================

Two objects of ``concurrent_unordered_multimap`` are equal if the following conditions are ``true``:

* They contain an equal number of elements.
* Each group of elements with the same key in one container has the corresponding group of equivalent
  elements in the other container (not necessary in the same order).

.. code:: cpp

    template <typename Key, typename T, typename Hash,
              typename KeyEqual, typename Allocator>
    bool operator==( const concurrent_unordered_multimap<Key, T, Hash, KeyEqual, Allocator>& lhs,
                     const concurrent_unordered_multimap<Key, T, Hash, KeyEqual, Allocator>& rhs );

**Returns**: ``true`` if ``lhs`` is equal to ``rhs``; ``false``, otherwise.

---------------------------------------------------------------------------------------------

.. code:: cpp

    template <typename Key, typename T, typename Hash,
              typename KeyEqual, typename Allocator>
    bool operator!=( const concurrent_unordered_multimap<Key, T, Hash, KeyEqual, Allocator>& lhs,
                     const concurrent_unordered_multimap<Key, T, Hash, KeyEqual, Allocator>& rhs );

Equivalent to ``!(lhs == rhs)``.

**Returns**: ``true`` if ``lhs`` is not equal to ``rhs``; ``false``, otherwise.
