.. _Parallelizing_Simple_Loops:

Parallelizing Simple Loops
==========================


The simplest form of scalable parallelism is a loop of iterations that
can each run simultaneously without interfering with each other. The
following sections demonstrate how to parallelize simple loops.


.. note:: 
   |full_name| components are
   defined in namespace ``tbb``. For brevity’s sake, the namespace is
   explicit in the first mention of a component, but implicit
   afterwards.


When compiling oneTBB programs, be sure to link in the oneTBB shared
library, otherwise undefined references will occur. The following table
shows compilation commands that use the debug version of the library.
Remove the "``_debug``" portion to link against the production version
of the library.


.. container:: tablenoborder


   .. list-table:: 
      :header-rows: 1

      * -  Operating System 
        -  Command line 
      * -     Windows\* OS     
        -      ``icl /MD example.cpp tbb_debug.lib``     
      * -     Linux\* OS     
        -      ``icc example.cpp -ltbb_debug``     


.. include:: Parallelizing_Simple_Loops_toctree.rst