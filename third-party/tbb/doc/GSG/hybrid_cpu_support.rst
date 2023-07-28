.. _hybrid_cpu_support:

Hybrid CPU and NUMA Support
***************************

If you need NUMA/Hybrid CPU support in oneTBB, you need to make sure that HWLOC* is installed on your system.

HWLOC* (Hardware Locality) is a library that provides a portable abstraction of the hierarchical topology of modern architectures (NUMA, hybrid CPU systems, etc). 
oneTBB relies on HWLOC* to identify the underlying topology of the system to optimize thread scheduling and memory allocation.

Without HWLOC*, oneTBB may not take advantage of NUMA/Hybrid CPU support. Therefore, it's important to make sure that HWLOC* is installed before using oneTBB on such systems.

Check HWLOC* on the System 
^^^^^^^^^^^^^^^^^^^^^^^^^^

To check if HWLOC* is already installed on your system, run `hwloc-ls`:

   * For Linux* OS, in the command line. 
   * For Windows* OS,  in the command prompt. 

If HWLOC* is installed, the command displays information about the hardware topology of your system. 
If it is not installed, you receive an error message saying that the command ``hwloc-ls`` could not be found.

.. note:: For Hybrid CPU support, make sure that HWLOC* is version 2.5 or higher.
          For NUMA support, install HWLOC* version 1.11 or higher. 

Install HWLOC*
^^^^^^^^^^^^^^

To install HWLOC*, visit the official Portable Hardware Locality website (https://www-lb.open-mpi.org/projects/hwloc/).

* For Windows* OS, binaries are available for download. 
* For Linux* OS, only the source code is provided and binaries should be built. 

On Linux* OS, HWLOC* can be also installed with package managers, such as APT*, YUM*, etc. 
To do so, run: ``sudo apt install hwloc``. 


.. note:: For Hybrid CPU support, make sure that HWLOC* is version 2.5 or higher.
          For NUMA support, install HWLOC* version 1.11 or higher.
