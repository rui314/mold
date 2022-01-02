.. _Partitioner_Summary:

Partitioner Summary
===================


The parallel loop templates ``parallel_for`` and ``parallel_reduce``
take an optional *partitioner* argument, which specifies a strategy for
executing the loop. The following table summarizes partitioners and
their effect when used in conjunction with ``blocked_range``.


.. container:: tablenoborder


   .. list-table::
      :header-rows: 1

      * -     Partitioner    
        -     Description    
        -     When Used with ``blocked_range(i,j,g)``
      * -     ``simple_partitioner``
        -     Chunksize bounded by grain size.    
        -      ``g/2 ≤ chunksize ≤ g``
      * -      ``auto_partitioner`` (default)    
        -     Automatic chunk size.    
        -     ``g/2 ≤ chunksize``     
      * -     ``affinity_partitioner``     
        -      Automatic chunk size, cache affinity and uniform distribution of iterations.    
        -     ``g/2 ≤ chunksize``     
      * -     ``static_partitioner``     
        -      Deterministic chunk size, cache affinity and uniform distribution of iterations without load balancing.    
        -     ``max(g/3, problem_size/num_of_resources) ≤ chunksize``      




An ``auto_partitioner`` is used when no partitioner is specified. In
general, the ``auto_partitioner`` or ``affinity_partitioner`` should be
used, because these tailor the number of chunks based on available
execution resources. ``affinity_partitioner`` and ``static_partitioner``
may take advantage of ``Range`` ability to split in a given ratio (see
"Advanced Topic: Other Kinds of Iteration Spaces") for distributing
iterations in nearly equal chunks between computing resources.


``simple_partitioner`` can be useful in the following situations:


-  The subrange size for ``operator()`` must not exceed a limit. That
   might be advantageous, for example, if your ``operator()`` needs a
   temporary array proportional to the size of the range. With a limited
   subrange size, you can use an automatic variable for the array
   instead of having to use dynamic memory allocation.


-  A large subrange might use cache inefficiently. For example, suppose
   the processing of a subrange involves repeated sweeps over the same
   memory locations. Keeping the subrange below a limit might enable the
   repeatedly referenced memory locations to fit in cache.


-  You want to tune to a specific machine.

