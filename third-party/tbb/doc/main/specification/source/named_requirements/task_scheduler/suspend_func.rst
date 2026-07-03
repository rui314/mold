.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

===========
SuspendFunc
===========
**[req.suspend_func]**

A type `Func` satisfies `SuspendFunc` if it meets the following requirements:

----------------------------------------------------------------------

**SuspendFunc Requirements: Pseudo-Signature, Semantics**

.. namespace:: SuspendFunc
	       
.. cpp:function:: Func::Func( const Func& )

    Copy constructor.

.. cpp:function:: void Func::operator()( oneapi::tbb::task::suspend_point )

    Body that accepts the current task execution point to resume later.

See also:

* :doc:`resumable tasks <../../task_scheduler/scheduling_controls/resumable_tasks>`

