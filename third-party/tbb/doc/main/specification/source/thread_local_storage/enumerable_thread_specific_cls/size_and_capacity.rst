.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=================
Size and capacity
=================

.. namespace:: oneapi::tbb::enumerable_thread_specific
	       
.. cpp:function:: size_type size() const

    Returns the number of elements in ``*this``.
    The value is equal to the number of distinct threads that have called ``local()`` after
    ``*this`` was constructed or most recently cleared.

.. cpp:function:: bool empty() const

    Returns ``true`` if the container is empty; ``false``, otherwise.

