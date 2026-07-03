.. _resource_limited_node_body_named_requirement:

``ResourceLimitedNodeBody`` Named Requirement
=============================================

The type ``Body`` satisfies ``ResourceLimitedNodeBody`` if it
satisfies the following requirements:

**ResourceLimitedNodeBody Requirements: Pseudo-Signature, Semantics**

.. code:: cpp

    Body::Body(const Body& other);

Copies the body.

------------------------------------------------------

.. code:: cpp

    Body::~Body();

Destroys the body.

------------------------------------------------------

.. code:: cpp

    void Body::operator()(const Input& input, OutputPortsType& ports,
                          ResourceHandle1& resource_handle1, ..., ResourceHandleN& resource_handleN);

Below, ``rl_node`` denotes the ``resource_limited_node`` instance into which the ``Body`` object
is passed during construction, and ``rl_node_type`` is its type.

**Requirements**:

* The ``Input`` type must be the same as the ``Input`` template argument of ``rl_node_type``.
* The ``OutputPortsType`` must be the same as ``rl_node_type::output_ports_type`` member type.
* ``ResourceHandle1``, ..., ``ResourceHandleN`` must be the same as ``resource_limiter::resource_handle_type``
  member type of the corresponding ``resource_limiter`` passed to ``rl_node`` during construction.

Processes the input message.
May call ``try_put`` on any of the output ports, possibly multiple times per port.
