.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

======
feeder
======
**[algorithms.parallel_for_each.feeder]**

Inlet into which additional work items for a ``parallel_for_each`` can be fed.

.. code:: cpp

    // Defined in header <oneapi/tbb/parallel_for_each.h>

    namespace oneapi {
        namespace tbb {

            template<typename Item>
            class feeder {
            public:
                void add( const Item& item );
                void add( Item&& item );
            };

        } // namespace tbb
    } //namespace oneapi

Member functions
----------------

.. cpp:function:: void add( const Item& item )

    Adds item to a collection of work items to be processed.

    **Requirements**: The ``Item`` type must meet the `CopyConstructible` requirements from the [copyconstructible] section of the ISO C++ Standard.

.. cpp:function:: void add( Item&& item )

    Same as the above but uses the move constructor of ``Item``, if available.
 
    **Requirements**: The ``Item`` type must meet the `MoveConstructible` requirements from the [moveconstructible] section of the ISO C++ Standard.

.. caution::

    Must be called from a ``Body::operator()`` created by the ``parallel_for_each`` function.
    Otherwise, the termination semantics of method ``operator()`` are undefined.
