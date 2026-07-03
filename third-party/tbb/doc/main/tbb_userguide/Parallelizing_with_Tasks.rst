.. _Parallelizing_with_Tasks:

Parallelizing with Tasks
========================

When parallel loops or the flow graph are not sufficient, the |full_name|
library supports parallelization directly with tasks. Tasks
can be created using the function ``oneapi::tbb::parallel_invoke`` or
the class ``oneapi::tbb::task_group``. 


.. toctree::
   :maxdepth: 4

   ../tbb_userguide/creating_tasks_with_parallel_invoke
   ../tbb_userguide/creating_tasks_with_task_group
   ../tbb_userguide/task_group_thread_safety
