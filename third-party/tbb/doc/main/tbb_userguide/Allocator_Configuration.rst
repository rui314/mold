.. _Allocator_Configuration:

Configuring the Memory Allocator
================================


The oneTBB memory allocator provides the following API functions and
environment variables to configure its behavior:


-  the ``scalable_allocation_command`` function instructs the allocator
   to perform a certain action, such as cleaning up its internal memory
   buffers.


-  the ``scalable_allocation_mode`` function allows an application to
   set certain parameters for the memory allocator, such as an option to
   map memory in huge pages or define a recommended heap size. These
   settings take effect until modified by another call to
   ``scalable_allocation_mode``.


Some of the memory allocator parameters can also be set via system
environment variables. It can be useful to adjust the behavior without
modifying application source code, to ensure that a setting takes effect
as early as possible, or to avoid explicit dependency on the oneTBB
allocator binaries. The following environment variables are recognized:


-  ``TBB_MALLOC_USE_HUGE_PAGES`` controls usage of huge pages for memory
   mapping.


-  ``TBB_MALLOC_SET_HUGE_OBJECT_THRESHOLD`` defines the lower bound for
   the size (bytes), that is interpreted as huge and not released during
   regular cleanup operations.


These variables only take effect at the time the memory manager is
initialized; later environment changes are ignored. A call to
``scalable_allocation_mode`` overrides the effect of the corresponding
environment variable.
