.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==============
Element access
==============

All member functions in this section can be performed concurrently with each other,
concurrent growth methods and while traversing the container.

In case of concurrent growth, the element returned by the access method
can refer to the element that is under construction of the other thread.

Access by index
---------------

    .. code:: cpp

        value_type& operator[]( size_type index );

        const value_type& operator[]( size_type index ) const;

    **Returns**: a reference to the element on the position ``index``.

    The behavior is undefined if ``index() >= size()``.

---------------------------------------------

    .. code:: cpp

        value_type& at( size_type index );

        const value_type& at( size_type index ) const;

    **Returns**: a reference to the element on the position ``index``.

    **Throws**:

        * ``std::out_of_range`` if ``index >= size()``.
        * ``std::range_error`` if the vector is broken and the element on the position ``index``
          unallocated.

Access the first and the last element
-------------------------------------

    .. code:: cpp

        value_type& front();

        const value_type& front() const;

    **Returns**: a reference to the first element in the vector.

---------------------------------------------

    .. code:: cpp

        value_type& back();

        const value_type& back() const;

    **Returns**: a reference to the last element in the vector.
