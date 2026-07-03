.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

===============
Non-member swap
===============

.. code:: cpp

    template <typename Key, typename T, typename HashCompare, typename Allocator>
    void swap( concurrent_hash_map<Key, T, HashCompare, Allocator>& lhs,
               concurrent_hash_map<Key, T, HashCompare, Allocator>& rhs );

Equivalent to ``lhs.swap(rhs)``.
