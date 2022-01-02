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
      - | ``include/oneapi/tbb.h``
	| ``include/oneapi/tbb/*.h``     
      - ``CPATH``     
    * - Shared libraries     
      - ``lib/<arch>/<lib><variant>.so.<version>``
      - | ``LIBRARY_PATH``
	| ``LD_LIBRARY_PATH``

where

* ``<arch>`` - ``ia32`` or ``intel64``

* ``<lib>`` - ``libtbb``, ``libtbbmalloc``, ``libtbbmalloc_proxy`` or ``libtbbbind``

* ``<variant>`` - ``_debug`` or empty

* ``<version>`` - binary version in a form of ``<major>.<minor>``