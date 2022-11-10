.. _Intro_gsg:


|full_name| is a runtime-based parallel programming model for C++ code that uses threads. 
It consists of a template-based runtime library to help you harness the latent performance of multi-core processors.

oneTBB enables you to simplify parallel programming by breaking computation into parallel running tasks. Within a single process, 
parallelism is carried out through threads, an operating system mechanism that allows the same or different sets of instructions 
to be executed simultaneously.

Here you can see one of the possible executions of tasks by threads.

.. figure:: /GSG/Images/how-oneTBB-works.png
   :scale: 70%
   :align: center

Use oneTBB to write scalable applications that:

* Specify logical parallel structure instead of threads
* Emphasize data-parallel programming
* Take advantage of concurrent collections and parallel algorithms

oneTBB supports nested parallelism and load balancing. It means that you can use the library without being worried about oversubscribing a system.
