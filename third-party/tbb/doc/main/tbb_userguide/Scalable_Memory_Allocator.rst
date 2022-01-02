.. _Scalable_Memory_Allocator:

Scalable Memory Allocator
=========================


Both the debug and release versions of |full_name| 
consists of two dynamic shared libraries, one with
general support and the other with a scalable memory allocator. The
latter is distinguished by ``malloc`` in its name. For example, the
release versions for Windows\* OS are ``tbb<version>.dll`` and ``tbbmalloc.dll``
respectively. Applications may choose to use only the general library,
or only the scalable memory allocator, or both. See the links below for
more information on memory allocation.

