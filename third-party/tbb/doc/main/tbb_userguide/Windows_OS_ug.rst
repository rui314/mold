.. _Windows_OS_ug:

Windows\*
=========

This section uses <*tbb_install_dir*> to indicate the top-level
installation directory. The following table describes the subdirectory
structure for Windows\*, relative to <*tbb_install_dir*>.

.. container:: tablenoborder


   .. list-table:: 
      :header-rows: 1

      * - Item     
        - Location     
        - Environment Variable     
      * - Header files     
        - | ``include\oneapi\tbb.h``
	  | ``include\oneapi\tbb\*.h``     
        - ``INCLUDE``     
      * - .lib files     
        - ``lib\<arch>\vc<vcversion>\<lib><variant><version>.lib``\    
        - ``LIB``     
      * - .dll files     
        - ``redist\<arch>\vc<vcversion>\<lib><variant><version>.dll``
        - ``PATH``
      * - .pdb files
        - Same as corresponding ``.dll`` file.
        - \

where

* ``<arch>`` - ``ia32`` or ``intel64``

* ``<lib>`` - ``tbb``, ``tbbmalloc``, ``tbbmalloc_proxy`` or ``tbbbind``

* ``<vcversion>`` 

  - ``14`` - use for dynamic linkage  with the CRT

  - ``14_uwp`` - use for Windows 10 Universal Windows applications

  - ``14_uwd`` - use for Universal Windows Drivers

  - ``_mt`` - use for static linkage with the CRT

* ``<variant>`` - ``_debug`` or empty

* ``<version>`` - binary version
 
The last column shows which environment variables are used by the
Microsoft\* Visual C++\* or Intel® C++ Compiler Classic or Intel® oneAPI DPC++/C++ Compiler to find these
subdirectories.

.. CAUTION:: 
   Ensure that the relevant product directories are mentioned by the
   environment variables; otherwise the compiler might not find the
   required files.


.. note::
   Microsoft\* C/C++ run-time libraries come in static and dynamic
   forms. Either can be used with oneTBB. Linking to the oneTBB library
   is always dynamic.
