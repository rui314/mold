|short_name| Get Started Guide provides the information you need to begin working with oneTBB. 
It is helpful for new users of parallel programming and experienced developers that want to improve code performance. 

It is recommended for you to have a basic knowledge of C++ programming and some experience with parallel programming concepts. 

|full_name| is a runtime-based parallel programming model for C++ code that uses tasks.
The template-based runtime library can help you harness the latent performance of multi-core processors.

oneTBB enables you to simplify parallel programming by breaking computation into parallel running tasks. Within a single process, 
parallelism is carried out by mapping tasks to threads. Threads are an operating system mechanism that allows the same or different sets of instructions 
to be executed simultaneously. Using threads can make your program work faster and more efficiently.

Here you can see one of the possible executions of tasks by threads.

.. figure:: /GSG/Images/how-oneTBB-works.png
   :scale: 70%
   :align: center

Use oneTBB to write scalable applications that:

* Specify logical parallel structure instead of threads.
* Emphasize data-parallel programming.
* Take advantage of concurrent collections and parallel algorithms.

oneTBB supports nested parallelism and load balancing. It means that you can use the library without worrying about oversubscribing a system, which happens when more tasks are assigned to a system than it can handle efficiently. 

oneTBB is used in different areas, such as scientific simulations, gaming, data analysis, etc. 

It is available as a stand-alone product and as part of the |base_tk|.
