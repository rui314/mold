.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==================
Parallel iteration
==================

Member types ``concurrent_map::range_type`` and ``concurrent_map::const_range_type``
meet the :doc:`ContainerRange requirements <../../named_requirements/containers/container_range>`.

These types differ only in that the bounds for a ``concurrent_map::const_range_type``
are of type ``concurrent_map::const_iterator``, whereas the bounds for a ``concurrent_map::range_type``
are of type ``concurrent_map::iterator``.

range member function
---------------------

    .. code:: cpp

        range_type range();

        const_range_type range() const;

    **Returns**: a range object representing all elements in the container.
