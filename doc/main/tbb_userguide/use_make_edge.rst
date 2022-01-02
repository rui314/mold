.. _use_make_edge:

Use make_edge and remove_edge
=============================


These are the basic guidelines for creating and removing edges:


-  use make_edge and remove_edge


-  Avoid using register_successor and register_predecessor


-  Avoid using remove_successor and remove_predecessor


As a convention, to communicate the topology, use only functions
flow::make_edge and flow::remove_edge. The runtime library uses node
functions, such as sender<T>::register_successor, to create these edges,
but those functions should not be called directly. The runtime library
calls these node functions directly to implement optimizations on the
topology at runtime.

