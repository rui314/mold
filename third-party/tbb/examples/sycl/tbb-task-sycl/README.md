# `TBB-Task-Sycl` Sample
This sample illustrates how two Intel® oneAPI Threading Building Blocks (oneTBB)
 tasks can execute similar computational kernels, with one task executing the SYCL* code and the other task executing the TBB code. This `tbb-task-sycl` sample code is implemented using C++ and SYCL* for CPU and GPU.

| Optimized for                     | Description
|:---                               |:---
| OS                                | Linux* Ubuntu* 18.04 <br> Windows* 10, 11
| Hardware                          | Skylake with GEN9 or newer
| Software                          | Intel® oneAPI DPC++/C++ Compiler
| What you will learn               | How to offload the computation to GPU using Intel® oneAPI DPC++/C++ Compiler
| Time to complete                  | 15 minutes

## Purpose
The purpose of this sample is to show how similar computational kernels can be executed by two TBB tasks, with one executing on SYCL-compliant code and another on TBB code.

## Key Implementation Details
The implementation based on TBB tasks and SYCL explained.

## Building the TBB-Task-Sycl Program

### Setting Environment Variables
When working with the Command Line Interface (CLI), you should configure the Intel&reg; oneAPI Toolkit using environment variables. Set up your CLI environment by sourcing the `setvars` script every time you open a new terminal window. This practice ensures your compiler, libraries, and tools are ready for development.

> **Note**: If you have not already done so, set up your CLI environment by sourcing the `setvars` script located in the root of your oneAPI installation.
>
> Linux*:
> - For system wide installations: `. /opt/intel/oneapi/setvars.sh`
> - For private installations: `. ~/intel/oneapi/setvars.sh`
> - For non-POSIX shells, like csh, use the following command: `$ bash -c 'source <install-dir>/setvars.sh ; exec csh'`
>
> Windows*:
> - `C:\"Program Files (x86)"\Intel\oneAPI\setvars.bat`
> - For Windows PowerShell*, use the following command: `cmd.exe "/K" '"C:\Program Files (x86)\Intel\oneAPI\setvars.bat" && powershell'`
>
> Microsoft Visual Studio*:
> - Open a command prompt window and execute `setx SETVARS_CONFIG " "`. This only needs to be set once and will automatically execute the `setvars` script every time Visual Studio is launched.
>
>For more information on environment variables, see "Use the setvars Script" for [Linux or macOS](https://www.intel.com/content/www/us/en/develop/documentation/oneapi-programming-guide/top/oneapi-development-environment-setup/use-the-setvars-script-with-linux-or-macos.html), or [Windows](https://www.intel.com/content/www/us/en/develop/documentation/oneapi-programming-guide/top/oneapi-development-environment-setup/use-the-setvars-script-with-windows.html).

You can use [Modulefiles scripts](https://www.intel.com/content/www/us/en/develop/documentation/oneapi-programming-guide/top/oneapi-development-environment-setup/use-modulefiles-with-linux.html) to set up your development environment. The modulefiles scripts work with all Linux shells.

If you wish to fine tune the list of components and the version of those components, use
a [setvars config file](https://www.intel.com/content/www/us/en/develop/documentation/oneapi-programming-guide/top/oneapi-development-environment-setup/use-the-setvars-script-with-linux-or-macos/use-a-config-file-for-setvars-sh-on-linux-or-macos.html) to set up your development environment.

### Using Visual Studio Code*  (Optional)

You can use Visual Studio Code (VS Code*) extensions to set your environment, create launch configurations, and browse and download samples.

The basic steps to build and run a sample using VS Code include:
 - Download a sample using the extension **Code Sample Browser for Intel&reg; Software Development Tools**.
 - Configure the oneAPI environment with the extension **Environment Configurator for Intel&reg; Software Development Tools**.
 - Open a Terminal in VS Code (**Terminal>New Terminal**).
 - Run the sample in the VS Code terminal using the instructions below.

To learn more about the extensions and how to configure the oneAPI environment, see
[Using Visual Studio Code* to Develop Intel® oneAPI Applications](https://software.intel.com/content/www/us/en/develop/documentation/using-vs-code-with-intel-oneapi/top.html).

### On a Linux System
    * Build tbb-task-sycl program
      cd tbb-task-sycl &&
      mkdir build &&
      cd build &&
      cmake .. &&
      make VERBOSE=1

    * Run the program
      make run

    * Clean the program
      make clean

## Running the Sample

### Example of Output
    executing on CPU
    executing on GPU
    Heterogeneous triad correct.
    TBB triad correct.
    input array a_array: 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15
    input array b_array: 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15
    output array c_array on GPU: 0 1.5 3 4.5 6 7.5 9 10.5 12 13.5 15 16.5 18 19.5 21 22.5
    output array c_array_tbb on CPU: 0 1.5 3 4.5 6 7.5 9 10.5 12 13.5 15 16.5 18 19.5 21 22.5
    Built target run

### Troubleshooting
If an error occurs, troubleshoot the problem using the Diagnostics Utility for oneAPI.
[Learn more](https://www.intel.com/content/www/us/en/develop/documentation/diagnostic-utility-user-guide/top.html).
