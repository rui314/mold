.. _Get_Started_Guide

Get Started with |full_name|
============================


|full_name| enables you to simplify parallel programming by breaking 
computation into parallel running tasks. oneTBB is available as a stand-alone
product and as part of the |base_tk|.

|short_name| is a runtime-based parallel programming model for C++ code that uses threads.
It consists of a template-based runtime library to help you harness the latent performance
of multi-core processors. Use |short_name| to write scalable applications that:

- Specify logical parallel structure instead of threads
- Emphasize data parallel programming
- Take advantage of concurrent collections and parallel algorithms

System Requirements
*******************

Refer to the `oneTBB System Requirements <https://software.intel.com/content/www/us/en/develop/articles/intel-oneapi-threading-building-blocks-system-requirements.html>`_.


Before You Begin
****************

Download |short_name| as a `stand-alone product <https://software.intel.com/content/www/us/en/develop/articles/oneapi-standalone-components.html#onetbb>`_ 
or as a part of the `Intel(R) oneAPI Base Toolkit <https://software.intel.com/content/www/us/en/develop/tools/oneapi/base-toolkit/download.html>`_.

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

Find more
*********

.. list-table:: 
   :widths: 40 60
   :header-rows: 0


   * - 
	   - `oneTBB Community Forum <https://community.intel.com/>`_
	   - `Product FAQs <https://software.intel.com/content/www/us/en/develop/support/faq-product.html>`_
	   - `Support requests <https://software.intel.com/content/www/us/en/develop/articles/how-to-create-a-support-request-at-online-service-center.html>`_
     - Use these resources if you need support with oneTBB.
   
   * - `Release Notes <https://software.intel.com/content/www/us/en/develop/articles/intel-oneapi-threading-building-blocks-release-notes.html>`_
     - Find up-to-date information about the product, including detailed notes, known issues, and changes.
   
   * - `Documentation <https://software.intel.com/content/www/us/en/develop/documentation/onetbb-documentation/top.html>`_: `Developer Guide <https://software.intel.com/content/www/us/en/develop/documentation/onetbb-documentation/top/onetbb-developer-guide.html>`_ and `API Reference <https://software.intel.com/content/www/us/en/develop/documentation/onetbb-documentation/top/onetbb-api-reference.html>`_
     - Learn to use oneTBB.   
   * - `GitHub* <https://github.com/oneapi-src/oneTBB>`_
     - Find oneTBB implementation in open source.
   

Notices and Disclaimers
***********************

Intel technologies may require enabled hardware, software or service activation.

No product or component can be absolutely secure.

Your costs and results may vary.

© Intel Corporation. Intel, the Intel logo, and other Intel marks are trademarks
of Intel Corporation or its subsidiaries. Other names and brands may be claimed
as the property of others.

No license (express or implied, by estoppel or otherwise) to any intellectual
property rights is granted by this document.

The products described may contain design defects or errors known as errata which
ay cause the product to deviate from published specifications. Current
characterized errata are available on request.

Intel disclaims all express and implied warranties, including without limitation,
the implied warranties of merchantability, fitness for a particular purpose,
and non-infringement, as well as any warranty arising from course of performance,
course of dealing, or usage in trade.
