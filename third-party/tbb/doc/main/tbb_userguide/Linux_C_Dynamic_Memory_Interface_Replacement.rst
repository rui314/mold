.. _Linux_C_Dynamic_Memory_Interface_Replacement:

Linux\* OS C/C++ Dynamic Memory Interface Replacement
=====================================================


Release version of the proxy library is ``libtbbmalloc_proxy.so``,
debug version is ``libtbbmalloc_proxy_debug.so``.


The following dynamic memory functions are replaced:


-  Standard C library functions: ``malloc``, ``calloc``, ``realloc``,
   ``free``, (added in C11) ``aligned_alloc``


-  Standard POSIX\* function: ``posix_memalign``


-  Obsolete functions: ``valloc``, ``memalign``, ``pvalloc``,
   ``mallopt``


-  Replaceable global C++ operators ``new`` and ``delete``


-  GNU C library (glibc) specific functions: ``malloc_usable_size``,
   ``__libc_malloc``, ``__libc_calloc``, ``__libc_memalign``,
   ``__libc_free``, ``__libc_realloc``, ``__libc_pvalloc``,
   ``__libc_valloc``


You can do the replacement either by loading the proxy library at
program load time using the ``LD_PRELOAD`` environment variable (without
changing the executable file), or by linking the main executable file
with the proxy library.


The OS program loader must be able to find the proxy library and the
scalable memory allocator library at program load time. For that you may
include the directory containing the libraries in the
``LD_LIBRARY_PATH`` environment variable or add it to
``/etc/ld.so.conf``.


There are limitations for dynamic memory replacement:


-  glibc memory allocation hooks, such as ``__malloc_hook``, are not
   supported.


-  Mono is not supported.


.. container:: section


   .. rubric:: Examples
      :class: sectiontitle

   These examples show how to set ``LD_PRELOAD`` and how to link a
   program to use the memory allocation replacements.


   ::


      # Set LD_PRELOAD to load the release version of the proxy library
      LD_PRELOAD=libtbbmalloc_proxy.so 
      # Link with the release version of the proxy library
      g++ foo.o bar.o -ltbbmalloc_proxy -o a.out


   To use the debug version of the library, replace *tbbmalloc_proxy*
   with *tbbmalloc_proxy_debug* in the above examples.

