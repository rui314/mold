.. _integrate:

Integrate oneTBB
================

If you want to improve the performance and scalability of your application, you can integrate oneTBB into your project. 
For example, you may want to integrate oneTBB if your application needs to process large amounts of data in parallel. 

To integrate oneTBB, you need to:

* Link oneTBB with the project's source code. 
* Provide the necessary compiler and linker flags.

However, you can use CMake* and the pkg-config tool to simplify the process of integrating oneTBB into your project and handling its dependencies.
See the instructions below to learn how to use the tools. 

CMake*
*******

CMake* is a cross-platform build tool that helps you manage dependencies and build systems. 
Integrating oneTBB into your project using CMake*:

* Simplifies the process of building and linking against the library.
* Ensures that your project can be built and run on multiple platforms.
* Lets you manage oneTBB dependencies.

To add oneTBB to another project using CMake*, add the following commands to your ``CMakeLists.txt`` file:

.. code-block::

       `find_package(TBB REQUIRED)`
       `target_link_libraries(my_executable TBB::tbb)`

After that, configure your project with CMake* as usual.


Compile a Program Using pkg-config
***********************************

The pkg-config tool is used to simplify the compilation line by retrieving information about packages
from special metadata files. It helps avoid large hard-coded paths and makes compilation more portable.

To compile a test program ``test.cpp`` with oneTBB on Linux* OS, 
provide the full path to search for included files and libraries, or provide a line as the following: 

.. code-block::
   
       g++ -o test test.cpp $(pkg-config --libs --cflags tbb)

Where:

``--cflags`` provides oneTBB library include path:

.. code-block::

       $ pkg-config --cflags tbb
       -I<path-to>/tbb/latest/lib/pkgconfig/../..//include

``--libs`` provides the Intel(R) oneTBB library name and the search path to find it:

.. code-block::
   
       $ pkg-config –libs tbb
       -L<path to>tbb/latest/lib/pkgconfig/../..//lib/intel64/gcc4.8 -ltbb

.. note::

   For Windows* OS, additionally, use the ``--msvc-syntax`` option flag that converts the compiling and linking flags in an appropriate mode.
