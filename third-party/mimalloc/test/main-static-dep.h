#pragma once

typedef void (__stdcall * TestFun)(void);

#if __cplusplus
extern "C" 
#endif
__declspec(dllexport) void __cdecl Test(void);



