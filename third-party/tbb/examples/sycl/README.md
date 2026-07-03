# Code Samples of Intel(R) oneAPI Threading Building Blocks (oneTBB)

| Code sample name                          | Supported Intel(R) Architecture(s) | Description
|:---                                       |:---                                |:---
| tbb-async-sycl             | GPU, CPU  | The calculations are split between TBB Flow Graph asynchronous node that calls SYCL* kernel on GPU while TBB functional node does CPU part of calculations.
| tbb-task-sycl              | GPU, CPU  | One TBB task executes SYCL* code on GPU while another TBB task performs calculations using TBB parallel_for.
| tbb-resumable-tasks-sycl   | GPU, CPU  | The calculations are split between TBB resumable task that calls SYCL kernel on GPU while TBB parallel_for does CPU part of calculations.
