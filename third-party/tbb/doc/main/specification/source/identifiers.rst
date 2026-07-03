.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

===========
Identifiers
===========
**[identifiers]**

This section describes the identifier conventions used by oneTBB.

Case
----

The identifier convention in the library follows the style of the ISO C++ standard library. Identifiers are written in underscore_style, and concepts - in PascalCase.

Reserved Identifier Prefixes
----------------------------

The library reserves the ``__TBB`` prefix for internal identifiers and macros that should never be directly referenced by your code.
