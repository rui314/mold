.. _task_group_extensions:

task_group extensions
=====================

This section documents ``task_group`` API extensions for advanced use cases.

.. note::
    To enable these extensions, define the ``TBB_PREVIEW_TASK_GROUP_EXTENSIONS`` macro with a value of ``1``.

.. toctree::
    :titlesonly:

    task_group_ext/task_bypass_support.rst
    task_group_ext/task_completion_handle_cls.rst
    task_group_ext/dynamic_dependencies.rst
    task_group_ext/wait_single_task.rst

.. rubric:: See also

* :onetbb-spec:`oneapi::tbb::task_group specification <task_scheduler/task_group/task_group_cls>`
* :onetbb-spec:`oneapi::tbb::task_group_context specification <task_scheduler/scheduling_controls/task_group_context_cls>`
* :onetbb-spec:`oneapi::tbb::task_group_status specification <task_scheduler/task_group/task_group_status_enum>`
* :onetbb-spec:`oneapi::tbb::task_handle class <task_scheduler/task_group/task_handle>`
