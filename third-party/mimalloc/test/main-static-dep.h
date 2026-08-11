#pragma once

#if __cplusplus
extern "C" {
#endif

#ifdef WIN32
typedef void (__cdecl *TestFun)(void);
__declspec(dllexport) void __cdecl Test(void);
#else
typedef void (*TestFun)(void);
void Test(void);
#endif

#if __cplusplus
}
#endif