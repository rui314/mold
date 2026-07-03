.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

============
Introduction
============
**[intro]**

This document specifies requirements for implementations of
oneAPI Threading Building Blocks (oneTBB).

oneTBB is a programming model
for scalable parallel programming using standard ISO C++
code. A program uses oneTBB to specify logical parallelism in algorithms, while a oneTBB implementation maps that parallelism
onto execution threads.

oneTBB employs generic programming via C++ templates,
with most of its interfaces defined by requirements on types and not
specific types. Generic programming makes oneTBB flexible yet efficient
through customizing APIs to specific needs of an application.

Here is the list of specific requirements for oneTBB implementations:

* An implementation should use the C++11 version of the standard and should not require newer versions except where explicitly specified; 
  it also should not require any non-standard language extensions.
* An implementation can use platform-specific APIs if they are compatible
  with the C++ execution and memory models. For example, a platform-specific
  implementation of threads can be used if that implementation provides
  the same execution guarantees as C++ threads.
* An implementation should support execution on single-core and multi-core CPUs,
  including those that provide simultaneous multithreading capabilities.
* On CPU, an implementation should support nested parallelism to enable building larger parallel components from smaller ones. 
