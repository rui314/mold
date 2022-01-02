.. _Mixing_Two_Runtimes:

Mixing two runtimes
=======================================

Threading Building Blocks (TBB) and oneAPI Threading Building Blocks (oneTBB) can be safely used in
the same application. TBB and oneTBB runtimes are named differently and can be loaded safely within
the same process. In addition, the ABI versioning is completely different that prevents symbols
conflicts.

However, if both runtimes are loaded into the same process it can lead to
oversubscription because each runtime will use its own pool of threads. It might lead to a
performance penalty due to increased number of context switches. To check if both TBB and
oneTBB are loaded to the application, export ``TBB_VERSION=1`` before the application run. If
both runtimes are loaded there will be two blocks of output, for example:

oneTBB possible output:

.. code:: text

    oneTBB: SPECIFICATION VERSION	1.0
    oneTBB: VERSION		2021.2
    oneTBB: INTERFACE VERSION	12020
    oneTBB: TBB_USE_DEBUG	1
    oneTBB: TBB_USE_ASSERT	1
    oneTBB: TOOLS SUPPORT	disabled

TBB possible output:

.. code:: text

    TBB: VERSION		2018.0
    TBB: INTERFACE VERSION	10006
    TBB: BUILD_DATE		Mon 01 Mar 2021 01:28:40 PM UTC
    TBB: BUILD_HOST		localhost (x86_64)
    TBB: BUILD_OS		Fedora release 32 (Thirty Two)
    TBB: BUILD_KERNEL	Linux 5.8.9-200.fc32.x86_64 #1 SMP Mon Sep 14 18:28:45 UTC 2020
    TBB: BUILD_GCC		g++ (GCC) 10.2.1 20201125 (Red Hat 10.2.1-9)
    TBB: BUILD_LIBC	2.31
    TBB: BUILD_LD		GNU ld version 2.34-6.fc32
    TBB: BUILD_TARGET	intel64 on cc10_libc2.31_kernel5.8.9
    TBB: BUILD_COMMAND	g++ -DDO_ITT_NOTIFY -g -O2 -DUSE_PTHREAD -m64 -fPIC -D__TBB_BUILD=1 -Wall -Wno-parentheses -Wno-non-virtual-dtor -I../../src -I../../src/rml/include -I../../include -I.
    TBB: TBB_USE_DEBUG	0
    TBB: TBB_USE_ASSERT	0
    TBB: DO_ITT_NOTIFY	1
    TBB: RML	private
    TBB: Tools support	disabled

