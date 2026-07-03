.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==============
ContainerRange
==============
**[req.container_range]**

``ContainerRange`` is a range that represents a concurrent container
or a part of the container.

The ``ContainerRange`` object can be used to traverse the container in parallel algorithms
like ``parallel_for``.

The type ``CR`` satisfies the ``ContainerRange`` requirements if:

* The type ``CR`` meets the requirements of :doc:`Range requirements <../algorithms/range>`.
* The type ``CR`` provides the following member types and functions:

    .. cpp:type:: CR::value_type

        The type of the item in the range.

    .. cpp:type:: CR::reference

        Reference type to the item in the range.

    .. cpp:type:: CR::const_reference

        Constant reference type to the item in the range.

    .. cpp:type:: CR::iterator

        Iterator type for range traversal.

    .. cpp:type:: CR::size_type

        Unsigned integer type for obtaining grain size.

    .. cpp:type:: CR::difference_type

        The type of the difference between two iterators.

    .. cpp:function:: iterator CR::begin()

        Returns an iterator to the beginning of the range.

    .. cpp:function:: iterator CR::end()

        Returns an iterator to the position that follows the last element in the range.

    .. cpp:function:: size_type CR::grainsize() const

        Returns the range grain size.
