.. _Floating_Point_Settings:

Floating-point Settings
=======================

To propagate CPU-specific settings for floating-point computations to tasks executed by the task scheduler, you can use one of the following two methods:

* When a ``task_arena`` or a task scheduler for a given application thread is initialized, they capture the current floating-point settings of the thread. 
* The ``task_group_context`` class has a method to capture the current floating-point settings. 

By default, worker threads use floating-point settings obtained during the initialization of a ``task_arena`` or the implicit arena of the application thread. The settings are applied to all computations within that ``task_arena`` or started by that application thread.


For better control over floating point behavior, a thread may capture the current settings in a task group context. Do it at context creation with a special flag passed to the constructor:

::
    
    task_group_context ctx( task_group_context::isolated,
                        task_group_context::default_traits | task_group_context::fp_settings );


Or call the ``capture_fp_settings`` method:

::
    
     task_group_context ctx;
    ctx.capture_fp_settings();


You can then pass the task group context to most parallel algorithms, including ``flow::graph``, to ensure that all tasks related to this algorithm use the specified floating-point settings. 
It is possible to execute the parallel algorithms with different floating-point settings captured to separate contexts, even at the same time.

Floating-point settings captured to a task group context prevail over the settings captured during task scheduler initialization. It means, if a context is passed to a parallel algorithm, the floating-point settings captured to the context are used. 
Otherwise, if floating-point settings are not captured to the context, or a context is not explicitly specified, the settings captured during the task arena initialization are used.

In a nested call to a parallel algorithm that does not use the context of a task group with explicitly captured floating-point settings, the outer-level settings are used. 
If none of the outer-level contexts capture floating-point settings, the settings captured during task arena initialization are used.

It guarantees that: 

* Floating-point settings are applied to all tasks executed within a task arena, if they are captured: 

  * To a task group context. 
  * During the arena initialization. 

* A call to a oneTBB parallel algorithm does not change the floating-point settings of the calling thread, even if the algorithm uses different settings.

.. note:: 
    The guarantees above apply only to the following conditions:
    
    * A user code inside a task should: 
      
      * Not change the floating-point settings.
      * Revert any modifications. 
      * Restore previous settings before the end of the task.

    * oneTBB task scheduler observers are not used to set or modify floating point settings.

    Otherwise, the stated guarantees are not valid and the behavior related to floating-point settings is undefined.

