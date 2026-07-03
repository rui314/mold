.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==========
Algorithms
==========
**[algorithms]**

oneAPI Threading Building Blocks provides a set of generic parallel algorithms.

Parallel Functions
------------------

.. toctree::
   :titlesonly:

   algorithms/functions/collaborative_call_once_func.rst

   algorithms/functions/parallel_for_func.rst
   algorithms/functions/parallel_reduce_func.rst
   algorithms/functions/parallel_deterministic_reduce_func.rst
   algorithms/functions/parallel_scan_func.rst

   algorithms/functions/parallel_for_each_func.rst

   algorithms/functions/parallel_invoke_func.rst
   algorithms/functions/parallel_pipeline_func.rst
   algorithms/functions/parallel_sort_func.rst

Blocked Ranges
--------------

Types that meet the :doc:`Range requirements <named_requirements/algorithms/range>`.

.. toctree::
   :titlesonly:

   algorithms/blocked_ranges/blocked_range_cls.rst
   algorithms/blocked_ranges/blocked_range2d_cls.rst
   algorithms/blocked_ranges/blocked_range3d_cls.rst
   algorithms/blocked_ranges/blocked_nd_range_cls.rst

.. _Partitioners:

Partitioners
------------

A partitioner specifies how a loop template should partition its work among threads.

.. toctree::
   :titlesonly:

   algorithms/partitioners/auto_partitioner.rst
   algorithms/partitioners/affinity_partitioner.rst
   algorithms/partitioners/static_partitioner.rst
   algorithms/partitioners/simple_partitioner.rst

Split Tags
----------

.. toctree::
   :titlesonly:

   algorithms/split_tags/proportional_split_cls.rst
   algorithms/split_tags/split_cls.rst
