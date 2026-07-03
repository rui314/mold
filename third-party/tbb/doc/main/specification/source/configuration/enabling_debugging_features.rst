.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

===========================
Enabling Debugging Features
===========================
**[configuration.debug_features]**

The following macros control certain debugging features. In general, it is useful to compile with
these features on for development code, and off for production code, because the features
may decrease performance. The table below summarizes the macros and their default values. A
value of 1 enables the corresponding feature; a value of 0 disables the feature.

.. table:: Debugging Macros

   ================================ ========================================== ==============================
   **Macro**                        **Default Value**                          **Feature**
   ================================ ========================================== ==============================
   ``TBB_USE_DEBUG``                * Windows* OS: 1 if ``_DEBUG`` is defined,
                                      0, otherwise.                            Default value for all other macros in this table.
                                    * All other systems: 0.
   -------------------------------- ------------------------------------------ ------------------------------
   ``TBB_USE_ASSERT``               ``TBB_USE_DEBUG``                          Enable internal assertion checking. Can significantly slow down performance.
   -------------------------------- ------------------------------------------ ------------------------------
   ``TBB_USE_PROFILING_TOOLS``      ``TBB_USE_DEBUG``                          Enable full support for analysis tools.
   ================================ ========================================== ==============================


TBB_USE_ASSERT Macro
--------------------

The ``TBB_USE_ASSERT`` macro controls whether error checking is enabled in the
header files. Define ``TBB_USE_ASSERT`` as ``1`` to enable error checking.

If an error is detected, the library prints an error message on ``stderr`` and
calls the standard C routine ``abort``. To stop a program when internal error
checking detects a failure, place a breakpoint on ``oneapi::tbb::assertion_failure``.

TBB_USE_PROFILING_TOOLS Macro
-----------------------------

The ``TBB_USE_PROFILING_TOOLS`` macro controls support for Intel®
Inspector XE, Intel® VTune™ Amplifier XE and Intel® Advisor.

Define ``TBB_USE_PROFILING_TOOLS`` as ``1`` to enable full support
for these tools. Leave ``TBB_USE_PROFILING_TOOLS`` undefined or equal to zero to enable
top performance in release builds, at the expense of turning off some support for tools.
