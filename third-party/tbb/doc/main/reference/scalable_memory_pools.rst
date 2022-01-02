.. _scalable_memory_pools_reference:

Scalable Memory Pools
=====================

.. note::
   To enable this feature, set the ``TBB_PREVIEW_MEMORY_POOL`` macro to 1.

Memory pools allocate and free memory from a specified region or an underlying allocator using
thread-safe, scalable operations. The  following table summarizes the Memory Pool named requirement.
Here, ``P`` represents an instance of the memory pool class.

.. container:: tablenoborder

   .. list-table:: 
      :header-rows: 1

      * -    Pseudo-Signature
        -    Semantics
      * -    \ ``~P() throw();``
        -    Destructor. Frees all the allocated memory.
      * -    \ ``void P::recycle();``
        -    Frees all the allocated memory.
      * -    \ ``void* P::malloc(size_t n);``
        -    Returns a pointer to ``n`` bytes allocated from the memory pool.
      * -    \ ``void P::free(void* ptr);``
        -    Frees the memory object specified via ``ptr`` pointer.
      * -    \ ``void* P::realloc(void* ptr, size_t n);``
        -    Reallocates the memory object pointed by ``ptr`` to ``n`` bytes.

.. container:: section

    .. rubric:: Model Types
        :class: sectiontitle

    The ``memory_pool`` template class and the ``fixed_pool`` class meet the Memory Pool named requirement.

.. toctree::
    :titlesonly:

    scalable_memory_pools/memory_pool_cls
    scalable_memory_pools/fixed_pool_cls
    scalable_memory_pools/memory_pool_allocator_cls
