.. _Mac_OS:

macOS\*
=======

This section uses *<tbb_install_dir>* to indicate the top-level installation directory.
The following table describes the subdirectory structure for macOS\*, relative to *<tbb_install_dir>*.

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
        - ``<tbb_install_dir>/lib/<lib><variant>.<version>.dylib``
        - | ``LIBRARY_PATH``
	  | ``DYLD_LIBRARY_PATH``
      * - Symbolic links
        - | ``<tbb_install_dir>/lib/<lib><variant>.dylib`` -> ``<tbb_install_dir>lib/<lib><variant>.<ver_major>.dylib``
          | ``<tbb_install_dir>/lib/<lib><variant>.<ver_major>.dylib`` -> ``<tbb_install_dir>lib/<lib><variant>.<ver_major>.<ver_minor>.dylib``
        - \ 
      * - CMake files
        - ``<tbb_install_dir>/lib/cmake/tbb/*.cmake``
        - \
      * - pkg-config files
        - ``<tbb_install_dir>/lib/pkgconfig/tbb.pc``
        - \
      * - vars script
        - ``<tbb_install_dir>/env/vars.sh``
        - \

where

* ``<lib>`` - ``libtbb``, ``libtbbmalloc`` or ``libtbbmalloc_proxy``

* ``<variant>`` - ``_debug`` or empty

* ``<version>`` - binary version in a form of ``<ver_major>.<ver_minor>``

See :ref:`Integrate oneTBB <integrate>` to learn how to use CMake* and pkg-config tools.
To set the environment, see :ref:`Next Steps <next_steps>`.
