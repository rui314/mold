.. _Debug_Versus_Release_Libraries:

Debug Versus Release Libraries
==============================


The following table details the |full_name| 
dynamic shared libraries that come in debug and release
versions.


.. container:: tablenoborder


   .. list-table::
      :header-rows: 1

      * -     Library 
        -     Description    
        -     When to Use    
      * -    | ``tbb_debug``
	     | ``tbbmalloc_debug``
	     | ``tbbmalloc_proxy_debug``
	     | ``tbbbind_debug``    
        -     These versions have extensive internal checking for correct use of the library.   
        -     Use with code that is compiled with the macro ``TBB_USE_DEBUG`` set to 1.    
      * -    | ``tbb``
	     | ``tbbmalloc``
	     | ``tbbmalloc_proxy``
	     | ``tbbbind``    
        -     These versions deliver top performance. They eliminate  most checking for correct use of the library.    
        -     Use with code compiled with ``TBB_USE_DEBUG`` undefined or set to zero.    

.. tip:: 
   Test your programs with the debug versions of the libraries first, to
   assure that you are using the library correctly.  With the release
   versions, incorrect usage may result in unpredictable program
   behavior.


oneTBB supports Intel® Inspector, Intel® VTune™ Profiler and Intel® Advisor.
Full support of these tools requires compiling with macro ``TBB_USE_PROFILING_TOOLS=1``.
That symbol defaults to 1 in the following conditions:

-  When ``TBB_USE_DEBUG=1``.
-  On the Microsoft Windows\* operating system, when ``_DEBUG=1``.

The :ref:`reference` section explains the default values in more detail.


.. CAUTION:: 
   The instrumentation support for Intel® Inspector becomes live after
   the first initialization of the task library. If the library
   components are used before this initialization occurs, Intel® Inspector
   may falsely report race conditions that are not really races.

