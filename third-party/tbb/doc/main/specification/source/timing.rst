.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

======
Timing
======
**[timing]**

Parallel programming is about speeding up *wall clock* time, which is the real time that it takes a program or function to run. The library provides API to simplify timing within an application.

Syntax
------

.. code:: cpp

   // Declared in tick_count.h

   class tick_count;

   class tick_count::interval_t;


Classes
-----------

.. toctree::

   timing/tick_count_cls.rst
