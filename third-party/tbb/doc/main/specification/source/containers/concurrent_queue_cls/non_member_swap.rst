.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

===============
Non-member swap
===============

.. code:: cpp

    template <typename T, typename Allocator>
    void swap( concurrent_queue<T, Allocator>& lhs,
               concurrent_queue<T, Allocator>& rhs );

Equivalent to ``lhs.swap(rhs)``.
