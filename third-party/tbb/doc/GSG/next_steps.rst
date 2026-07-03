.. _next_steps:

Next Steps
===========

After installing oneTBB, complete the following steps to start working with the library.

Set the Environment Variables
*****************************

After installing |short_name|, set the environment variables:
  
#. Go to the oneTBB installation directory. 

#. Set the environment variables using the script in ``<install_dir>`` by running:
     
   * On Linux* OS: ``vars.{sh|csh} in <install_dir>/tbb/latest/env``
   * On Windows* OS: ``vars.bat in <install_dir>/tbb/latest/env``

.. tip::

   oneTBB can coordinate with Intel(R) OpenMP on CPU resources usage to avoid
   excessive oversubscription when both runtimes are used within a process. To
   enable this feature set ``TCM_ENABLE`` environment variable to ``1``. For
   more details, see :ref:`avoid_cpu_overutilization`.


Build and Run a Sample 
**********************

.. tab-set::

    .. tab-item:: Windows* OS

        #. Create a new C++ project using your IDE. In this example, Microsoft* Visual Studio* Code is used. 
        #. Create an ``example.cpp`` file in the project. 
        #. Copy and paste the code below. It is a typical example of a |short_name| algorithm. The sample calculates a sum of all integer numbers from 1 to 100. 
   
           .. code:: 

              #include <oneapi/tbb.h>
            
              int main (){
                  int sum = oneapi::tbb::parallel_reduce(
                      oneapi::tbb::blocked_range<int>(1,101), 0,
                      [](oneapi::tbb::blocked_range<int> const& r, int init) -> int {
                          for (int v = r.begin(); v != r.end(); v++) {
                              init += v;
                          }
                          return init;
                      },
                      [](int lhs, int rhs) -> int {
                          return lhs + rhs;
                      }
                  );
      
                  printf("Sum: %d\n", sum);
                  return 0;
              }
      
        #. Open the ``tasks.json`` file in the ``.vscode`` directory and paste the following lines to the args array:

           * ``-Ipath/to/oneTBB/include`` to add oneTBB include directory. 
           * ``path/to/oneTBB/`` to add oneTBB. 

           For example:

           .. code-block::

                  { 
                     "tasks": [
                          {
                             "label": "build & run",
                             "type": "cppbuild",
                             "group": {
                             "args": [ 
                                 "/IC:\\Program Files (x86)\\Intel\\oneAPI\\tbb\\2022.0.0\\include",
                                 "C:\\Program Files (x86)\\Intel\\oneAPI\\tbb\\2022.0.0\\lib\\tbb12.lib"
                           

        #. Build the project. 
        #. Run the example. 
        #. If oneTBB is configured correctly, the output displays ``Sum: 5050``.  

    .. tab-item:: Linux* OS

        #. Create an ``example.cpp`` file in the project. 
        #. Copy and paste the code below. It is a typical example of a |short_name| algorithm. The sample calculates a sum of all integer numbers from 1 to 100. 
         
           .. code:: 

              #include <oneapi/tbb.h>

              int main(){
                  int sum = oneapi::tbb::parallel_reduce(
                      oneapi::tbb::blocked_range<int>(1,101), 0,
                      [](oneapi::tbb::blocked_range<int> const& r, int init) -> int {
                          for (int v = r.begin(); v != r.end(); v++) {
                              init += v;
                          }
                          return init;
                      },
                      [](int lhs, int rhs) -> int {
                          return lhs + rhs;
                      }
                  );
      
                  printf("Sum: %d\n", sum);
                  return 0;
              }

        #. Compile the code using oneTBB. For example, 

           .. code-block:: 

                  g++ -std=c++11 example.cpp -o example -ltbb

      
        #. Run the executable:

           .. code-block:: 

                  ./example
      
        #. If oneTBB is configured correctly, the output displays ``Sum: 5050``. 



Enable Hybrid CPU and NUMA Support
***********************************

To support Hybrid CPU and NUMA platforms, oneTBB relies on the HWLOC* library.
 
The HWLOC functionality is accessed through a set of proxy libraries whose names begin with the ``tbbbind`` prefix. oneTBB automatically loads them at runtime when needed. To find and load these libraries successfully, locate them in the same directory as the oneTBB library itself (e.g., alongside ``libtbb.so`` or ``tbb.dll``).
 
Starting with oneTBB 2022.2, the default ``tbbbind`` library is statically linked with HWLOC 2.x. This version is used if a system-provided HWLOC cannot be found.
 
If you prefer to use a system-provided or custom-built version of HWLOC, make sure it is available in the search paths used by the dynamic loader. Consult your platform’s dynamic loader documentation for details about these paths (e.g., ``LD_LIBRARY_PATH`` on Linux* OS or ``PATH`` on Windows* OS).
 
To use a specific HWLOC version, place one of the following tbbbind variants in the same directory as the oneTBB library:

* ``tbbbind_2_5`` — depends on HWLOC version 2.5 or higher. Use this version if hybrid CPU support is required.
* ``tbbbind_2_0`` — depends on HWLOC versions 2.1 to 2.4.

.. tip:: To confirm that tbbbind is loaded successfully at runtime, set the environment variable ``TBB_VERSION=1`` before launching your application.
