.. _appendix_B:

Appendix B Mixing With Other Threading Packages
===============================================


Correct Interoperability
^^^^^^^^^^^^^^^^^^^^^^^^

You can use |short_name| with other threading packages. No additional
effort is required.


Here is an example that parallelizes an outer loop with OpenMP and an
inner loop with |short_name|.

.. literalinclude:: ./examples/tbb_mixing_other_runtimes_example.cpp
   :language: c++
   :start-after: /*begin outer loop openmp with nested tbb*/
   :end-before: /*end outer loop openmp with nested tbb*/


``#pragma omp parallel`` instructs OpenMP to create a team of
threads. Each thread executes the code block statement associated with
the directive.

``#pragma omp for`` indicates that the compiler should distribute
the iterations of the following loop among the threads in the existing
thread team, enabling parallel execution of the loop body.


See the similar example with the POSIX\* Threads:

.. literalinclude:: ./examples/tbb_mixing_other_runtimes_example.cpp
   :language: c++
   :start-after: /*begin pthreads with tbb*/
   :end-before: /*end pthreads with tbb*/


.. _avoid_cpu_overutilization:

Avoid CPU Overutilization
^^^^^^^^^^^^^^^^^^^^^^^^^

While you can safely use |short_name| with other threading packages
without affecting the execution correctness, running a large number of
threads from multiple thread pools concurrently can lead to
oversubscription. This may significantly overutilize system resources,
affecting the execution performance.


Consider the previous example with nested parallelism, but with an
OpenMP parallel region executed within the parallel loop:

.. literalinclude:: ./examples/tbb_mixing_other_runtimes_example.cpp
   :language: c++
   :start-after: /*begin outer loop tbb with nested omp*/
   :end-before: /*end outer loop tbb with nested omp*/


Due to the semantics of the OpenMP parallel region, this composition of
parallel runtimes may result in a quadratic number of simultaneously
running threads. Such oversubscription can degrade the performance.


|short_name| solves this issue with Thread Composability Manager (TCM).
It is an experimental CPU resource coordination layer that enables
better cooperation between different threading runtimes.


By default, TCM is disabled. To enable it, set ``TCM_ENABLE``
environment variable to ``1``. To make sure it works as intended set
``TCM_VERSION`` environment variable to ``1`` before running your
application and check the output for lines starting with ``TCM:``. The
``TCM: TCM_ENABLE 1`` line confirms that Thread Composability Manager is
active.


Example output:

::

    TCM: VERSION            1.3.0
    <...>
    TCM: TCM_ENABLE         1


When used with the OpenMP implementation of Intel(R) oneAPI DPC++/C++ Compiler,
TCM allows to avoid simultaneous scheduling of excessive threads in the
scenarios similar to the one above.


Submit feedback or ask questions about Thread Composability
Manager through |short_name| `GitHub Issues
<https://github.com/uxlfoundation/oneTBB/issues>`_ or `Discussions
<https://github.com/uxlfoundation/oneTBB/discussions>`_.


.. note::
   Coordination on the use of CPU resources requires support for Thread
   Composability Manager. For optimal coordination, make sure that each
   threading package in your application integrates with TCM.


.. rubric:: See also

* `End Parallel Runtime Scheduling Conflicts with Thread Composability
  Manager
  <https://www.intel.com/content/www/us/en/developer/videos/threading-composability-manager-with-onetbb.html>`_

