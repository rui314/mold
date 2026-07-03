.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

===============
Non-member swap
===============

.. code:: cpp

    template <typename T, typename Compare, typename Allocator>
    void swap( concurrent_set<T, Compare, Allocator>& lhs,
               concurrent_set<T, Compare, Allocator>& rhs );

Equivalent to ``lhs.swap(rhs)``.
