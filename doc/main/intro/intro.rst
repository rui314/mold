.. _intro:

Introduction
============


|full_name| is a library that supports scalable parallel programming using
standard ISO C++ code. It does not require special languages or
compilers. It is designed to promote scalable data parallel programming.
Additionally, it fully supports nested parallelism, so you can build
larger parallel components from smaller parallel components. To use the
library, you specify tasks, not threads, and let the library map tasks
onto threads in an efficient manner.


Many of the library interfaces employ generic programming, in which
interfaces are defined by requirements on types and not specific types.
The C++ Standard Template Library (STL) is an example of generic
programming. Generic programming enables oneTBB to be flexible yet
efficient. The generic interfaces enable you to customize components to
your specific needs.


.. note:: 
   |full_name| requires C++11 standard compiler support.


The net result is that oneTBB enables you to specify parallelism far
more conveniently than using raw threads, and at the same time can
improve performance.


.. admonition:: Product and Performance Information 

   Performance varies by use, configuration and other factors. Learn more at `www.intel.com/PerformanceIndex <https://www.intel.com/PerformanceIndex>`_.
   Notice revision #20201201 



