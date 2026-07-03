.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

===================
ParallelForEachBody
===================
**[req.parallel_for_each_body]**

A type `Body` satisfies `ParallelForEachBody` if it meets the `Function Objects`
requirements described in the [function.objects] section of the ISO C++ standard,
as well as meets exactly one of the following two alternative requirements for ``operator()``:

----------------------------------------------------------------

**ParallelForEachBody Requirements: Pseudo-Signature, Semantics**

Alternative 1:

.. cpp:function:: void Body::operator()( ReferenceType item ) const

    Process the received item.

----------------------------------------------------------------

Alternative 2:

.. cpp:function:: void Body::operator()( ReferenceType item, oneapi::tbb::feeder<ItemType>& feeder ) const
                  void Body::operator()( ItemType&& item, oneapi::tbb::feeder<ItemType>& feeder ) const

    Process the received item. May invoke the ``feeder.add`` function to spawn additional items.
    The ``Body::operator()`` must accept both ``ReferenceType`` and ``ItemType&&`` values as the first argument.

-----------------------------------------------------------------

where

* ``ItemType`` is ``std::iterator_traits<Iterator>::value_type`` for the type of the iterator
  the ``parallel_for_each`` algorithm operates with, and
* ``ReferenceType`` is ``std::iterator_traits<Iterator>::reference`` if the iterator type is
  a `forward iterator` as described in the [forward.iterators] section of the ISO C++ Standard,
* otherwise, ``ReferenceType`` is ``ItemType&&``.

.. note::

    The usual rules for :ref:`pseudo-signatures <pseudo_signatures>` apply.
    Therefore, ``Body::operator()`` may optionally take items by value or by ``const`` reference.

See also:

* :doc:`parallel_for_each algorithm <../../algorithms/functions/parallel_for_each_func>`
* :doc:`feeder class<../../algorithms/functions/feeder>`
