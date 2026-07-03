.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

===========
GatewayType
===========
**[req.gateway_type]**

A type `T` satisfies `GatewayType` if it meets the following requirements:

------------------------------------------------------------------------------------------

**GatewayType Requirements: Pseudo-Signature, Semantics**

.. cpp:function:: bool T::try_put( const Output &v )

    **Requirements:** The type ``Output`` must be the same as the template type argument ``Output`` of the
    corresponding ``async_node`` instance.

    Broadcasts ``v`` to all successors of the corresponding ``async_node`` instance.

.. cpp:function:: void T::reserve_wait()

    Notifies a flow graph that work has been submitted to an external activity.

.. cpp:function:: void T::release_wait()

    Notifies a flow graph that work submitted to an external activity has completed.
