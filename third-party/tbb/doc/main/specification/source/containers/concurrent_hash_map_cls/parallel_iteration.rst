.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==================
Parallel iteration
==================

Member types ``concurrent_hash_map::range_type`` and ``concurrent_hash_map::const_range_type``
meet the :doc:`ContainerRange requirements <../../named_requirements/containers/container_range>`.

These types differ only in that the bounds for a ``concurrent_hash_map::const_range_type``
are of type ``concurrent_hash_map::const_iterator``, whereas the bounds for a ``concurrent_hash_map::range_type``
are of type ``concurrent_hash_map::iterator``.

Traversing the ``concurrent_hash_map`` is not thread safe. The behavior is undefined in case of concurrent execution of any
member functions while traversing the ``range_type`` or ``const_range_type``.

range member function
---------------------

    .. code:: cpp

        range_type range( std::size_t grainsize = 1 );

        const_range_type range( std::size_t grainsize = 1 ) const;

    **Returns**: a range object representing all elements in the container.
