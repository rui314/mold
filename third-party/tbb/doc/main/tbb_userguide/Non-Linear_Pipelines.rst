.. _Non-Linear_Pipelines:

Non-Linear Pipelines
====================


Template function ``parallel_pipeline`` supports only linear pipelines.
It does not directly handle more baroque plumbing, such as in the
diagram below.


.. container:: fignone
   :name: image011


   |image0|


However, you can still use a pipeline for this. Just topologically sort
the filters into a linear order, like this:


The light gray arrows are the original arrows that are now implied by
transitive closure of the other arrows. It might seem that lot of
parallelism is lost by forcing a linear order on the filters, but in
fact the only loss is in the *latency* of the pipeline, not the
throughput. The latency is the time it takes a token to flow from the
beginning to the end of the pipeline. Given a sufficient number of
processors, the latency of the original non-linear pipeline is three
filters. This is because filters A and B could process the token
concurrently, and likewise filters D and E could process the token
concurrently.


.. container:: fignone
   :name: image012


   |image1|


In the linear pipeline, the latency is five filters. The behavior of
filters A, B, D and E above may need to be modified in order to properly
handle objects that don’t need to be acted upon by the filter other than
to be passed along to the next filter in the pipeline.


The throughput remains the same, because regardless of the topology, the
throughput is still limited by the throughput of the slowest serial
filter. If ``parallel_pipeline`` supported non-linear pipelines, it
would add a lot of programming complexity, and not improve throughput.
The linear limitation of ``parallel_pipeline`` is a good tradeoff of
gain versus pain.


.. |image0| image:: Images/image011.jpg
   :width: 281px
   :height: 107px
.. |image1| image:: Images/image012.jpg
   :width: 281px
   :height: 107px

