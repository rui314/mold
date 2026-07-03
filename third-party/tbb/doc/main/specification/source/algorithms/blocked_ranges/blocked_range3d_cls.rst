.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

===============
blocked_range3d
===============
**[algorithms.blocked_range3d]**

Class template that represents a recursively divisible three-dimensional half-open interval.

A ``blocked_range3d`` is the three-dimensional extension of ``blocked_range2d``.

.. code:: cpp

    namespace oneapi {
        namespace tbb {
            template<typename PageValue, typename RowValue=PageValue, typename ColValue=RowValue>
            class blocked_range3d {
            public:
                // Types
                using page_range_type = blocked_range<PageValue>;
                using row_range_type = blocked_range<RowValue>;
                using col_range_type = blocked_range<ColValue>;

                // Constructors
                blocked_range3d(
                    PageValue page_begin, PageValue page_end,
                    typename page_range_type::size_type page_grainsize,
                    RowValue row_begin, RowValue row_end,
                    typename row_range_type::size_type row_grainsize,
                    ColValue col_begin, ColValue col_end,
                    typename col_range_type::size_type col_grainsize );
                blocked_range3d( PageValue page_begin, PageValue page_end
                                RowValue row_begin, RowValue row_end,
                                ColValue col_begin, ColValue col_end );
                blocked_range3d( blocked_range3d& r, split );
                blocked_range3d( blocked_range3d& r, proportional_split& proportion );

                // Capacity
                bool empty() const;

                // Access
                bool is_divisible() const;
                const page_range_type& pages() const;
                const row_range_type& rows() const;
                const col_range_type& cols() const;
            };

        } // namespace tbb
    } // namespace oneapi        

Requirements:

* The *PageValue*, *RowValue* and *ColValue* must meet the :doc:`blocked_range requirements <../../named_requirements/algorithms/blocked_range_val>`

Member types
------------

.. code:: cpp

    using page_range_type = blocked_range<PageValue>;

The type of the page values.

.. code:: cpp

    using row_range_type = blocked_range<RowValue>;

The type of the row values.

.. code:: cpp

    using col_range_type = blocked_range<ColValue>;

The type of the column values.

Member functions
----------------

.. code:: cpp

    blocked_range3d(PageValue page_begin, PageValue page_end,
        typename page_range_type::size_type page_grainsize,
        RowValue row_begin, RowValue row_end,
        typename row_range_type::size_type row_grainsize,
        ColValue col_begin, ColValue col_end,
        typename col_range_type::size_type col_grainsize);

**Effects:**  Constructs a ``blocked_range3d`` representing a three-dimensional space of values.
The space is the half-open Cartesian product ``[page_begin, page_end) x [row_begin, row_end) x [col_begin, col_end)``, with the given grain sizes for the pages, rows and columns.

**Example:**  The statement ``blocked_range3d<int,char,int> r(0, 6, 2, 'a', 'z'+1, 3, 0, 10, 2 );`` constructs a three-dimensional
space that contains all value pairs of the form ``(i, j, k)``, where ``i`` ranges from 0 to 6 with a grain size of 2,
``j`` ranges from ``'a'`` to ``'z'`` with a grain size of 3, and ``k`` ranges from 0 to 9 with a grain size of 2.

.. code:: cpp

    blocked_range3d(PageValue page_begin, PageValue page_end,
            RowValue row_begin, RowValue row_end,
            ColValue col_begin, ColValue col_end);

Same as ``blocked_range3d(page_begin,page_end,1,row_begin,row_end,1,col_begin,col_end,1)``.

.. code:: cpp

    blocked_range3d( blocked_range3d& range, split );

Basic splitting constructor.

**Requirements**: ``is_divisible()`` is true.

**Effects**: Partitions ``range`` into two subranges. The newly constructed ``blocked_range3d`` is approximately
the second half of the original ``range``, and ``range`` is updated to be the remainder.
Each subrange has the same grain size as the original ``range``. Splitting is done either by pages, rows, or columns.
The choice of which axis to split is intended to cause, after repeated splitting, the
subranges to approach the aspect ratio of the respective page, row, and column grain sizes.

.. code:: cpp

    blocked_range3d( blocked_range3d& range, proportional_split proportion );

Proportional splitting constructor.

**Requirements**: ``is_divisible()`` is true.

**Effects**: Partitions ``range`` into two subranges in the given ``proportion``
across one of its axes. The choice of which axis to split is made in the same way as for the basic splitting
constructor; then, proportional splitting is done for the chosen axis. The second axis and the grain sizes for
each subrange remain the same as in the original range.

.. code:: cpp

    bool empty() const;

**Effects**: Determines if range is empty.

**Returns:** ``pages.empty()||rows().empty()||cols().empty()``

.. code:: cpp

    bool is_divisible() const;

**Effects**: Determines if the range can be split into subranges.

**Returns:** ``pages().is_divisible()||rows().is_divisible()||cols().is_divisible()``

.. code:: cpp

    const page_range_type& pages() const;

**Returns:**  Range containing the pages of the value space.

.. code:: cpp

    const row_range_type& rows() const;

**Returns:**  Range containing the rows of the value space.

.. code:: cpp

    const col_range_type& cols() const;

**Returns:**  Range containing the columns of the value space.

See also:

* :doc:`blocked_range <blocked_range_cls>`
* :doc:`blocked_range2d <blocked_range2d_cls>`

