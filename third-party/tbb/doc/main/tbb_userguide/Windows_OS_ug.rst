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
        - | ``<tbb_install_dir>\include\oneapi\tbb.h``
	  | ``<tbb_install_dir>\include\oneapi\tbb\*.h``     
        - ``INCLUDE``     
      * - .lib files     
        - ``<tbb_install_dir>\lib\<arch>\vc<vcversion>\<lib><version><compat_version><variant>.lib``\    
        - ``LIB``     
      * - .dll files     
        - ``<tbb_install_dir>\redist\<arch>\vc<vcversion>\<lib><version><compat_version><variant>.dll``
        - ``PATH``
      * - .pdb and .def files
        - Same as corresponding ``.dll`` file.
        - \
      * - CMake files
        - ``<tbb_install_dir>\lib\cmake\tbb\*.cmake``
        - \
      * - pkg-config files
        - ``<tbb_install_dir>\lib\pkgconfig\*.pc``
        - \
      * - vars script
        - ``<tbb_install_dir>\env\vars.bat``
        - \

Where

* ``<arch>`` - ``ia32`` or ``intel64``

  .. note:: Starting with oneTBB 2022.0, 32-bit binaries are supported only by the open-source version of the library.

* ``<lib>`` - ``tbb``, ``tbbmalloc``, ``tbbmalloc_proxy`` or ``tbbbind``
* ``<vcversion>`` 

  - ``14`` - use for dynamic linkage with the Microsoft* C runtime (CRT)

  - ``14_uwp`` - use for Windows 10 Universal Windows applications

  - ``14_uwd`` - use for Universal Windows Drivers

  - ``_mt`` - use for static linkage with the CRT

* ``<variant>`` - ``_debug`` or empty
* ``<version>`` - binary version
* ``<compat_version>`` - compatibility version for ``tbbbind``
 
The last column shows, which environment variables are used by the
Microsoft\* Visual C++\* or Intel® oneAPI DPC++/C++ Compiler, to find these
subdirectories.

See :ref:`Integrate oneTBB <integrate>` to learn how to use CMake* and pkg-config tools.
To set the environment, see :ref:`Next Steps <next_steps>`.

.. CAUTION:: 
   Ensure that the relevant product directories are mentioned by the
   environment variables; otherwise the compiler might not find the
   required files.


.. note::
   Microsoft\* C/C++ run-time libraries come in static and dynamic
   forms. Either can be used with oneTBB. Linking to the oneTBB library
   is always dynamic.
