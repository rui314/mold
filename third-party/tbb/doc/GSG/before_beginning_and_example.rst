.. _Before_You_Begin:

Before You Begin
****************

After installing |short_name|, you need to set the environment variables:
  
#. Go to the oneTBB installation directory (``<install_dir>``). By default, ``<install_dir>`` is the following:
     
   * On Linux* OS:
	 
     * For superusers (root): ``/opt/intel/oneapi``
     * For ordinary users (non-root): ``$HOME/intel/oneapi``
     
   * On Windows* OS:

     * ``<Program Files>\Intel\oneAPI``

#. Set the environment variables, using the script in <install_dir>, by running
     
   * On Linux* OS:
	 
     ``vars.{sh|csh} in <install_dir>/tbb/latest/env``
	   
   * On Windows* OS:
	 
     ``vars.bat in <install_dir>/tbb/latest/env``


Example
*******

Below you can find a typical example for a |short_name| algorithm. 
The sample calculates a sum of all integer numbers from 1 to 100. 

.. code:: cpp

   int sum = oneapi::tbb::parallel_reduce(oneapi::tbb::blocked_range<int>(1,101), 0,
      [](oneapi::tbb::blocked_range<int> const& r, int init) -> int {
         for (int v = r.begin(); v != r.end(); v++  ) {
            init += v;
         }
         return init;
      },
      [](int lhs, int rhs) -> int {
         return lhs + rhs;
      }
   );