.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

===============================
pre_scan_tag and final_scan_tag
===============================
**[algorithms.parallel_scan.scan_tags]**

Types that distinguish the phases of ``parallel_scan``.

Types ``pre_scan_tag`` and ``final_scan_tag`` are dummy types used in conjunction with ``parallel_scan``.
See the example in the :doc:`parallel_scan <parallel_scan_func>` section for demonstration of how they are used in the signature of ``operator()``.

.. code:: cpp

    // Defined in header <oneapi/tbb/parallel_scan.h>

    namespace oneapi {
        namespace tbb {

           struct pre_scan_tag {
               static bool is_final_scan();
               operator bool();
           };

           struct final_scan_tag {
               static bool is_final_scan();
               operator bool();
           };

        } // namespace tbb
    } // namespace oneapi

Member functions
----------------

.. cpp:function:: bool is_final_scan()

    ``true`` for a ``final_scan_tag``, ``false``, otherwise.

.. cpp:function:: operator bool()

    ``true`` for a ``final_scan_tag``, ``false``, otherwise.

