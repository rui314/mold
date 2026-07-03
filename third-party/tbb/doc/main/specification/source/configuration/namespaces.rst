.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==========
Namespaces
==========
**[configuration.namespaces]**

This section describes the oneTBB namespace conventions.

tbb Namespace
-------------

The ``tbb`` namespace contains public identifiers defined by the library
that you can reference in your program.

tbb::flow Namespace
-------------------

The ``tbb::flow`` namespace contains public identifiers defined by the
library that you can reference in your program related to the flow graph feature. See
:doc:`Flow Graph <../flow_graph>` for more information.

oneapi::tbb Namespace
---------------------

The ``tbb`` namespace is a part of the top level ``oneapi`` namespace. Therefore, all API from the ``tbb``
namespace (incl. the ``tbb::flow`` namespace) are available in the ``oneapi::tbb`` namespace. The
``oneapi::tbb`` namespace can be considered as an alias for the ``tbb`` namespace:

.. code:: cpp

    namespace oneapi { namespace tbb = ::tbb; }
