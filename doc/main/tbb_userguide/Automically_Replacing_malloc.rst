.. _Automically_Replacing_malloc:

Automatically Replacing ``malloc`` and Other C/C++ Functions for Dynamic Memory Allocation
==========================================================================================


On Windows*, Linux\* operating systems, it is possible to automatically
replace all calls to standard functions for dynamic memory allocation
(such as ``malloc``) with the |full_name| scalable equivalents.
Doing so can sometimes improve application performance.


Replacements are provided by the proxy library (the library names can be
found in platform-specific sections below). A proxy library and a
scalable memory allocator library should be taken from the same release
of oneTBB, otherwise the libraries may be mutually incompatible.

.. toctree::
   :maxdepth: 4

   ../tbb_userguide/Windows_C_Dynamic_Memory_Interface_Replacement
   ../tbb_userguide/Linux_C_Dynamic_Memory_Interface_Replacement