.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=================
Size and capacity
=================

empty
-----

    .. code:: cpp

        bool empty() const;

    **Returns**: ``true`` if the container is empty; ``false``, otherwise.

    The result may differ with the actual container state in case of pending concurrent insertions or erasures.

size
----

    .. code:: cpp

        size_type size() const;

    **Returns**: the number of elements in the container.

    The result may differ with the actual container state in case of pending concurrent insertions or erasures.

max_size
--------

    .. code:: cpp

        size_type max_size() const;

    **Returns**: The maximum number of elements that container can hold.
