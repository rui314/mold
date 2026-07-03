.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
.. SPDX-FileCopyrightText: Contributors to the oneAPI Specification project.
..
.. SPDX-License-Identifier: CC-BY-4.0

===================
Version Information
===================
**[configuration.version_information]**

oneTBB has macros, an environment variable, and a function that reveal
version and runtime information.

.. code:: cpp

    // Defined in header <oneapi/tbb/version.h>

    #define ONETBB_SPEC_VERSION /*implementation-defined*/

    #define TBB_VERSION_MAJOR   /*implementation-defined*/
    #define TBB_VERSION_MINOR   /*implementation-defined*/
    #define TBB_VERSION_STRING  /*implementation-defined*/

    #define TBB_INTERFACE_VERSION_MAJOR /*implementation-defined*/
    #define TBB_INTERFACE_VERSION_MINOR /*implementation-defined*/
    #define TBB_INTERFACE_VERSION       /*implementation-defined*/

    const char* TBB_runtime_version();
    int TBB_runtime_interface_version();

Version Macros
--------------

oneTBB defines macros related to versioning, as described below.

* ``ONETBB_SPEC_VERSION`` macro defined to the decimal literal whose value equals to
  ``x * 100 + y``, where ``x`` is the major version and ``y`` is the minor version
  of the latest specification of oneTBB fully supported by the implementation.
* ``TBB_VERSION_MAJOR`` macro defined to integral value that represents major library version.
* ``TBB_VERSION_MINOR`` macro defined to integral value that represents minor library version.
* ``TBB_VERSION_STRING`` macro defined to the string representation of the full library version.
* ``TBB_INTERFACE_VERSION`` macro defined to a decimal literal for the current interface version.
  The value of the literal equals to ``x * 1000 + y * 10 + z`` where ``x`` is the major interface
  version number, ``y`` is the minor interface version number, and ``z`` is a decimal digit.
  This macro is increased in each release.
* ``TBB_INTERFACE_VERSION_MAJOR`` macro defined to ``TBB_INTERFACE_VERSION/1000``, which is the
  major interface version number.
* ``TBB_INTERFACE_VERSION_MINOR`` macro defined to ``TBB_INTERFACE_VERSION%1000/10``, which is the
  minor interface version number.

TBB_runtime_interface_version Function
--------------------------------------

Function that returns the interface version of the oneTBB library that was loaded at runtime.

The value returned by ``TBB_runtime_interface_version()`` may differ from the value of
``TBB_INTERFACE_VERSION`` obtained at compile time. This can be used to identify whether an
application was compiled against a compatible version of the oneTBB headers.

In general, the run-time value ``TBB_runtime_interface_version()`` must be greater than
or equal to the compile-time value of ``TBB_INTERFACE_VERSION``. Otherwise, the application may fail to
resolve all symbols at run time.

TBB_runtime_version Function
----------------------------

Function that returns the version string of the oneTBB library that was loaded at runtime.

The value returned by ``TBB_runtime_version()`` may differ from the value of
``TBB_VERSION_STRING`` obtained at compile time.

TBB_VERSION Environment Variable
--------------------------------

Set the environment variable ``TBB_VERSION`` to ``1`` to cause the library to print information on 
``stderr``. Each line is of the form ``“TBB: tag value”``, where *tag* and *value* provide additional
library information below.

.. caution::

    This output is implementation specific and may change at any time.
