.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==================
Parallel iteration
==================

Member types ``concurrent_vector::range_type`` and ``concurrent_vector::const_range_type``
meet the :doc:`ContainerRange requirements <../../named_requirements/containers/container_range>`.

These types differ only in that the bounds for a ``concurrent_vector::const_range_type``
are of type ``concurrent_vector::const_iterator``, whereas the bounds for a ``concurrent_vector::range_type``
are of type ``concurrent_vector::iterator``.

range member function
---------------------

    .. code:: cpp

        range_type range( size_type grainsize = 1 );

        const_range_type range( size_type grainsize = 1 ) const;

    **Returns**: a range object representing all elements in the container.
