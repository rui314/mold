.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==========
FilterBody
==========
**[req.filter_body]**

A type `Body` should meet one of the following requirements depending on the filter type:

----------------------------------------------------------------

**MiddleFilterBody Requirements: Pseudo-Signature, Semantics**

.. namespace:: MiddleFilterBody

.. cpp:function:: OutputType Body::operator()( InputType item ) const

    Processes the received item and then returns it.

----------------------------------------------------------------

**FirstFilterBody Requirements: Pseudo-Signature, Semantics**

.. namespace:: FirstFilterBody

.. cpp:function:: OutputType Body::operator()( oneapi::tbb::flow_control& fc ) const

    Returns the next item from an input stream. Calls ``fc.stop()`` at the end of an input stream.

----------------------------------------------------------------

**LastFilterBody Requirements: Pseudo-Signature, Semantics**

.. namespace:: LastFilterBody

.. cpp:function:: void Body::operator()( InputType item ) const

    Processes the received item. 

----------------------------------------------------------------

**SingleFilterBody Requirements: Pseudo-Signature, Semantics**

.. namespace:: SingleFilterBody

.. cpp:function:: void Body::operator()( oneapi::tbb::flow_control& fc ) const

    Processes an element from an input stream. Calls ``fc.stop()`` at the end of an input stream.

See also:

* :doc:`filter class <../../algorithms/functions/parallel_pipeline_func/filter_cls>`
