.. _Windows_C_Dynamic_Memory_Interface_Replacement:

Windows\* OS C/C++ Dynamic Memory Interface Replacement
=======================================================


Release version of the proxy library is ``tbbmalloc_proxy.dll``, debug
version is ``tbbmalloc_proxy_debug.dll``.


The following dynamic memory functions are replaced:


-  Standard C library functions: ``malloc``, ``calloc``, ``realloc``,
   ``free``


-  Replaceable global C++ operators ``new`` and ``delete``


-  Microsoft\* C run-time library functions: ``_msize``,
   ``_aligned_malloc``, ``_aligned_realloc``, ``_aligned_free``,
   ``_aligned_msize``


.. note:: 
   Replacement of memory allocation functions is not supported for
   Universal Windows Platform applications.


To do the replacement use one of the following methods:


-  Add the following header to a source code of any binary which is
   loaded during application startup.


   ::


      #include "oneapi/tbb/tbbmalloc_proxy.h"


-  Alternatively, add the following parameters to the linker options for
   the .exe or .dll file that is loaded during application startup.

   For 32-bit code (note the triple underscore):


   ::


      tbbmalloc_proxy.lib /INCLUDE:"___TBB_malloc_proxy"
   
   For 64-bit code (note the double underscore):


   ::


      tbbmalloc_proxy.lib /INCLUDE:"__TBB_malloc_proxy"


The OS program loader must be able to find the proxy library and the
scalable memory allocator library at program load time. For that you may
include the directory containing the libraries in the ``PATH``
environment variable.


The replacement uses in-memory binary instrumentation of Visual C++\*
runtime libraries. To ensure correctness, it must first recognize a
subset of dynamic memory functions in these libraries. If a problem
occurs, the replacement is skipped, and the program continues to use the
standard memory allocation functions. You can use the ``TBB_malloc_replacement_log``
function to check if the replacement has succeeded and to get additional information.


Set the ``TBB_MALLOC_DISABLE_REPLACEMENT`` environment variable to 1 to
disable replacement for a specific program invocation. In this case, the
program will use standard dynamic memory allocation functions. Note that
the oneTBB memory allocation libraries are still required for the
program to start even if their usage is disabled.

