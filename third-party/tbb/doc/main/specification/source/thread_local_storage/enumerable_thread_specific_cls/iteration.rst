.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=========
Iteration
=========

Class template ``enumerable_thread_specific`` supports random access iterators,
which enable iteration over the set of all elements in the container.

.. namespace:: oneapi::tbb::enumerable_thread_specific
	       
.. cpp:function:: iterator begin()

    Returns ``iterator`` pointing to the beginning of the set of elements.

.. cpp:function:: iterator end()

    Returns ``iterator`` pointing to the end of the set of elements.

.. cpp:function:: const_iterator begin() const

    Returns ``const_iterator`` pointing to the beginning of the set of elements.

.. cpp:function:: const_iterator end() const

    Returns ``const_iterator`` pointing to the end of the set of elements.

Class template ``enumerable_thread_specific`` supports ``const_range_type`` and ``range_type`` types,
which model the :doc:`ContainerRange requirement <../../named_requirements/containers/container_range>`.
The types differ only in that the bounds for a ``const_range_type`` are of type ``const_iterator``,
whereas the bounds for a ``range_type`` are of type ``iterator``.

.. cpp:function:: const_range_type range( size_t grainsize=1 ) const

    **Returns**: A ``const_range_type`` representing all elements in ``*this``.
    The parameter ``grainsize`` is in units of elements.

.. cpp:function:: range_type range( size_t grainsize=1 )

    **Returns**: A ``range_type`` representing all elements in ``*this``.
    The parameter ``grainsize`` is in units of elements.

