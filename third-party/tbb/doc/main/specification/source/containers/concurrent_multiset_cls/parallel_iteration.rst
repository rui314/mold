.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==================
Parallel iteration
==================

Member types ``concurrent_multiset::range_type`` and ``concurrent_multiset::const_range_type``
meet the :doc:`ContainerRange requirements <../../named_requirements/containers/container_range>`.

These types differ only in that the bounds for a ``concurrent_multiset::const_range_type``
are of type ``concurrent_multiset::const_iterator``, whereas the bounds for a ``concurrent_multiset::range_type``
are of type ``concurrent_multiset::iterator``.

range member function
---------------------

    .. code:: cpp

        range_type range();

        const_range_type range() const;

    **Returns**: a range object representing all elements in the container.
