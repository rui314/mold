.. _Mac_OS:

macOS\*
=======

This section uses *<install_dir>* to indicate the top-level installation directory.
The following table describes the subdirectory structure for macOS\*, relative to *<install_dir>*.

.. container:: tablenoborder

   .. list-table:: 
      :header-rows: 1

      * - Item     
        - Location     
        - Environment Variable     
      * - Header files     
        - | ``include/oneapi/tbb.h``
 	  | ``include/oneapi/tbb/*.h``     
        - ``CPATH`` 
      * - Shared libraries
        - ``lib/<lib><variant>.<version>.dylib``
        - | ``LIBRARY_PATH``
	  | ``DYLD_LIBRARY_PATH``

where

* ``<lib>`` - ``libtbb``, ``libtbbmalloc`` or ``libtbbmalloc_proxy``

* ``<variant>`` - ``_debug`` or empty

* ``<version>`` - binary version in a form of ``<major>.<minor>``
