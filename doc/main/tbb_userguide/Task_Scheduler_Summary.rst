.. _Task_Scheduler_Summary:

Task Scheduler Summary
======================


The task scheduler works most efficiently for fork-join parallelism with
lots of forks, so that the task-stealing can cause sufficient
breadth-first behavior to occupy threads, which then conduct themselves
in a depth-first manner until they need to steal more work.

