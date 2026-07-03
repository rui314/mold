.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

===============
Non-member swap
===============

.. code:: cpp

    template <typename Key, typename T, typename Hash,
              typename KeyEqual, typename Allocator>
    void swap( concurrent_unordered_map<Key, T, Hash, KeyEqual, Allocator>& lhs,
               concurrent_unordered_map<Key, T, Hash, KeyEqual, Allocator>& rhs ) noexcept(noexcept(lhs.swap(rhs)));

Equivalent to ``lhs.swap(rhs)``.
