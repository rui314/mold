.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=====================
Scalable Memory Pools
=====================

Memory pools allocate and free memory from a
specified region or underlying allocator providing thread-safe, scalable
operations. The following table summarizes the memory pool concept. Here P
represents an instance of the memory pool class.

= ========================================================================================
\ Memory Pool Concept: Pseudo-Signature, Semantics
==========================================================================================
\ ``~P() throw();``
  \
  Destructor. Frees all the memory of allocated
  objects.
------------------------------------------------------------------------------------------
\ ``void P::recycle();``
  \
  Frees all the memory of allocated objects.
------------------------------------------------------------------------------------------
\ ``void* P::malloc(size_t n);``
  \
  Returns pointer to 
  **n** bytes
  allocated from memory pool.
------------------------------------------------------------------------------------------
\ ``void P::free(void* ptr);``
  \
  Frees memory object specified via 
  **ptr**
  pointer.
------------------------------------------------------------------------------------------
\ ``void* P::realloc(void* ptr, size_t n);``
  \
  Reallocates memory object pointed by 
  **ptr** to 
  **n** bytes.
------------------------------------------------------------------------------------------
= ========================================================================================


Model Types
-----------

Template class memory_pool and class fixed_pool model
the Memory Pool concept.

.. toctree::

   scalable_memory_pools/memory_pool_cls.rst
   scalable_memory_pools/fixed_pool_cls.rst
   scalable_memory_pools/memory_pool_allocator_cls.rst
