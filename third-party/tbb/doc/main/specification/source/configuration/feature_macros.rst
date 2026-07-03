.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==============
Feature Macros
==============
**[configuration.feature_macros]**

Macros in this section control optional features of the library.

TBB_USE_EXCEPTIONS macro
------------------------

The ``TBB_USE_EXCEPTIONS`` macro controls whether the library headers use exception-handling
constructs such as ``try``, ``catch``, and ``throw``. The headers do not use these constructs when
``TBB_USE_EXCEPTIONS=0``.

For the Microsoft Windows*, Linux*, and macOS* operating systems, the default value is 1 if
exception handling constructs are enabled in the compiler, and 0, otherwise.

.. caution::

    The runtime library may still throw an exception when ``TBB_USE_EXCEPTIONS=0``.

TBB_USE_GLIBCXX_VERSION macro
-----------------------------

The ``TBB_USE_GLIBCXX_VERSION`` macro can be used to specify
the proper version of GNU libstdc++ if the detection fails. Define the value 
of the macro equal to ``Major*10000 + Minor*100 + Patch``,
where Major.Minor.Patch is the actual GCC/libstdc++ version (if unknown,
it can be obtained with the ``'gcc -dumpversion'`` command).
For example, if you use libstdc++ from GCC 4.9.2, define
``TBB_USE_GLIBCXX_VERSION=40902``.

