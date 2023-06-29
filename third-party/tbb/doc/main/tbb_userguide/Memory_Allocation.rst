.. _Memory_Allocation:

Memory Allocation
=================


|full_name| provides several memory
allocator templates that are similar to the STL template class
std::allocator. Two templates, ``scalable_allocator<T>`` and
``cache_aligned_allocator<T>``, address critical issues in parallel
programming as follows:


-  **Scalability**. Problems of scalability arise when using memory
   allocators originally designed for serial programs, on threads that
   might have to compete for a single shared pool in a way that allows
   only one thread to allocate at a time.


   Use the ``scalable_allocator<T>`` template to avoid scalability
   bottlenecks. This template can improve the performance of programs
   that rapidly allocate and free memory.


-  **False sharing**. Problems of sharing arise when two threads access
   different words that share the same cache line. The problem is that a
   cache line is the unit of information interchange between processor
   caches. If one processor modifies a cache line and another processor
   reads the same cache line, the line must be moved from one processor
   to the other, even if the two processors are dealing with different
   words within the line. False sharing can hurt performance because
   cache lines can take hundreds of clocks to move.


   Use the ``cache_aligned_allocator<T>`` template to always allocate on
   a separate cache line. Two objects allocated by
   ``cache_aligned_allocator`` are guaranteed to not have false sharing.
   However, if an object is allocated by ``cache_aligned_allocator`` and
   another object is allocated some other way, there is no guarantee.


You can use these allocator templates as the *allocator* argument to STL
template classes.The following code shows how to declare an STL vector
that uses ``cache_aligned_allocator``\ for allocation:


::


   std::vector<int,cache_aligned_allocator<int> >;


.. tip::
   The functionality of ``cache_aligned_allocator<T>`` comes at some
   cost in space, because it must allocate at least one cache line’s
   worth of memory, even for a small object. So use
   ``cache_aligned_allocator<T>`` only if false sharing is likely to be
   a real problem.


The scalable memory allocator also provides a set of functions
equivalent to the C standard library memory management routines but has
the ``scalable_`` prefix in their names, as well as the way to easily
redirect the standard routines to these functions.

.. toctree::
   :maxdepth: 4

   ../tbb_userguide/Which_Dynamic_Libraries_to_Use
   ../tbb_userguide/Allocator_Configuration
   ../tbb_userguide/automatically-replacing-malloc
