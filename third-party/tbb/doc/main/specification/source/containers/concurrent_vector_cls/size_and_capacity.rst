.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=================
Size and capacity
=================

size
----

    .. code:: cpp

        size_type size() const noexcept;

    **Returns**: the number of elements in the vector.

empty
-----

    .. code:: cpp

        bool empty() const noexcept;

    **Returns**: ``true`` if the vector is empty; ``false``, otherwise.

max_size
--------

    .. code:: cpp

        size_type max_size() const noexcept;

    **Returns**: the maximum number of elements that the vector can hold.

capacity
--------

    .. code:: cpp

        size_type capacity() const noexcept;

    **Returns**: the maximum number of elements that the vector
    can hold without allocating more memory.
