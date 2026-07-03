.. _Linux_OS:

Linux\* 
=======


This section uses *<tbb_install_dir>* to indicate the top-level
installation directory. The following table describes the subdirectory
structure for Linux\*, relative to *<tbb_install_dir>*

.. container:: tablenoborder

  .. list-table:: 
    :header-rows: 1

    * - Item     
      - Location     
      - Environment Variable     
    * - Header files     
      - | ``<tbb_install_dir>/include/oneapi/tbb.h``
	| ``<tbb_install_dir>/include/oneapi/tbb/*.h``     
      - |``C_INCLUDE_PATH``
        |``CPLUS_INCLUDE_PATH``
    * - Shared libraries     
      - ``<tbb_install_dir>/lib/<arch>/gcc4.8/<lib><compat_version><variant>.so.<version>``
      - | ``LIBRARY_PATH``
	| ``LD_LIBRARY_PATH``
    * - Symbolic links
      - | ``<tbb_install_dir>/lib/<arch>/gcc4.8/<lib><compat_version><variant>.so`` -> ``<tbb_install_dir>lib/<lib><compat_version><variant>.so.<ver_major>``
        | ``<tbb_install_dir>/lib/<lib><compat_version><variant>.so.<ver_major>`` -> ``<tbb_install_dir>lib/<lib><compat_version><variant>.so.<ver_major>.<ver_minor>``
      - 
    * - CMake files
      - ``<tbb_install_dir>/lib/cmake/tbb/*.cmake``
      - 
    * - pkg-config files
      - ``<tbb_install_dir>/lib/pkgconfig/*.pc``
      - 
    * - vars script
      - ``<tbb_install_dir>/env/vars.sh``
      - 

Where:

* ``<arch>`` - ``ia32`` or ``intel64``
  
   .. note:: Starting with oneTBB 2022.0, 32-bit binaries are supported only by the open-source version of the library. 

* ``<lib>`` - ``libtbb``, ``libtbbmalloc``, ``libtbbmalloc_proxy`` or ``libtbbbind``
* ``<variant>`` - ``_debug`` or empty
* ``<version>`` - binary version in a form of ``<ver_major>.<ver_minor>``
* ``<compat_version>`` - compatibility version for ``libtbbbind``
