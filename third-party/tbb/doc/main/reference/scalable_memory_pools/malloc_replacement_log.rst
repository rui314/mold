.. _malloc_replacement_log:

TBB_malloc_replacement_log Function
===================================

.. note:: This function is for Windows* OS only. 

Summary
*******

Provides information about the status of dynamic memory allocation replacement.

Syntax
*******

:: 

   extern "C" int TBB_malloc_replacement_log(char *** log_ptr);


Header
******

::

   #include "oneapi/tbb/tbbmalloc_proxy.h"


Description
***********

Dynamic replacement of memory allocation functions on Windows* OS uses in-memory binary instrumentation techniques. 
To make sure that such instrumentation is safe, oneTBB first searches for a subset of replaced functions in the Visual C++* runtime DLLs
and checks if each one has a known bytecode pattern. If any required function is not found or its bytecode pattern is unknown, the replacement is skipped, 
and the program continues to use the standard memory allocation functions.

The ``TBB_malloc_replacement_log`` function allows the program to check if the dynamic memory replacement happens and to get a log of the performed checks.

**Returns:**

* 0, if all necessary functions are successfully found and the replacement takes place.
* 1, otherwise. 

The ``log_ptr`` parameter must be an address of a char** variable or be ``NULL``. If it is not ``NULL``, the function writes there the address of an array of 
NULL-terminated strings containing detailed information about the searched functions in the following format:

::

   search_status: function_name (dll_name), byte pattern: <bytecodes>

 
For more information about the replacement of dynamic memory allocation functions, see :ref:`Windows_C_Dynamic_Memory_Interface_Replacement`. 


Example 
*******

::

   #include "oneapi/tbb/tbbmalloc_proxy.h"
   #include <stdio.h>

   int main(){
       char **func_replacement_log;
       int func_replacement_status = TBB_malloc_replacement_log(&func_replacement_log);

       if (func_replacement_status != 0) {
           printf("tbbmalloc_proxy cannot replace memory allocation routines\n");
           for (char** log_string = func_replacement_log; *log_string != 0; log_string++) {
               printf("%s\n",*log_string);
            }
       }

       return 0;
   }


Example output:

:: 

   tbbmalloc_proxy cannot replace memory allocation routines
   Success: free (ucrtbase.dll), byte pattern: <C7442410000000008B4424>
   Fail: _msize (ucrtbase.dll), byte pattern: <E90B000000CCCCCCCCCCCC>
