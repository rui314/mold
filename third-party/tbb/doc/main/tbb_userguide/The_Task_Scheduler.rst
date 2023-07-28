.. _The_Task_Scheduler:

The Task Scheduler
==================


This section introduces the |full_name|
*task scheduler*. The task scheduler is the engine that powers the loop
templates. When practical, use the loop templates instead of
the task scheduler, because the templates hide the complexity of the
scheduler. However, if you have an algorithm that does not naturally map
onto one of the high-level templates, use the task scheduler.

.. toctree::
   :maxdepth: 4

   ../tbb_userguide/Task-Based_Programming
   ../tbb_userguide/When_Task-Based_Programming_Is_Inappropriate
   ../tbb_userguide/How_Task_Scheduler_Works
   ../tbb_userguide/Task_Scheduler_Bypass
   ../tbb_userguide/Guiding_Task_Scheduler_Execution