.. _class_join_node_extension:

Type-specified message keys for join_node
=========================================

.. note::
    To enable this feature, define the ``TBB_PREVIEW_FLOW_GRAPH_FEATURES`` macro to 1.

.. contents::
    :local:
    :depth: 1

Description
***********

The extension allows a key matching ``join_node`` to obtain keys via functions associated with
its input types. The extension simplifies the existing approach by removing the need to
provide a function object for each input port of ``join_node``.

API
***

Header
------

.. code:: cpp

   #include "oneapi/tbb/flow_graph.h"

Syntax
------

The extension adds a special constructor to the ``join_node`` interface when the
``key_matching<typename K, class KHash=tbb_hash_compare>`` policy is
used. The constructor has the following signature:

.. code:: cpp

   join_node( graph &g )

When constructed this way, a ``join_node`` calls the
``key_from_message`` function for each incoming message to obtain the key associated
with it. The default implementation of ``key_from_message`` is the following

.. code:: cpp

   namespace oneapi {
       namespace tbb {
           namespace flow {
               template <typename K, typename T>
               K key_from_message( const T &t ) {
                   return t.key();
               }
           }
       }
   }

``T`` is one of the user-provided types in ``OutputTuple`` and is
used to construct the ``join_node``, and ``K`` is the key type
of the node.
By default, the ``key()`` method defined in the message class will be called.
Alternatively, the user can define its own ``key_from_message`` function in the
same namespace with the message type. This function will be found via C++ argument-dependent
lookup and used in place of the default implementation.

See Also
********

`join_node Specification <https://spec.oneapi.com/versions/latest/elements/oneTBB/source/flow_graph/join_node_cls.html>`_
