/*
    Copyright (c) 2005-2021 Intel Corporation

    Licensed under the Apache License, Version 2.0 (the "License");
    you may not use this file except in compliance with the License.
    You may obtain a copy of the License at

        http://www.apache.org/licenses/LICENSE-2.0

    Unless required by applicable law or agreed to in writing, software
    distributed under the License is distributed on an "AS IS" BASIS,
    WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
    See the License for the specific language governing permissions and
    limitations under the License.
*/

#include "oneapi/tbb/global_control.h"

#include "seismic_video.hpp"
#include "universe.hpp"

const char *const SeismicVideo::titles[2] = { "Seismic Simulation: Serial",
                                              "Seismic Simulation: Parallel" };
void SeismicVideo::on_mouse(int x, int y, int key) {
    if (key == 1) {
        u_.TryPutNewPulseSource(x, y);
    }
}

void SeismicVideo::on_key(int key) {
    key &= 0xff;
    if (char(key) == ' ')
        initIsParallel = !initIsParallel;
    else if (char(key) == 'p')
        initIsParallel = true;
    else if (char(key) == 's')
        initIsParallel = false;
    else if (char(key) == 'e')
        updating = true;
    else if (char(key) == 'd')
        updating = false;
    else if (key == 27)
        running = false;
    title = titles[initIsParallel ? 1 : 0];
}

void SeismicVideo::on_process() {
    oneapi::tbb::global_control c(oneapi::tbb::global_control::max_allowed_parallelism,
                                  threadsHigh);
    for (int frames = 0; numberOfFrames_ == 0 || frames < numberOfFrames_; ++frames) {
        if (initIsParallel)
            u_.ParallelUpdateUniverse();
        else
            u_.SerialUpdateUniverse();
        if (!next_frame())
            break;
    }
}

#if defined(_WINDOWS) && !defined(_CONSOLE)
#include "resource.hpp"
LRESULT CALLBACK WndProc(HWND hWnd, UINT message, WPARAM wParam, LPARAM lParam);
SeismicVideo *gVideo = nullptr;
#endif

SeismicVideo::SeismicVideo(Universe &u,
                           int number_of_frames,
                           int threads_high,
                           bool init_is_parallel)
        : numberOfFrames_(number_of_frames),
          initIsParallel(init_is_parallel),
          u_(u),
          threadsHigh(threads_high) {
    title = titles[initIsParallel ? 1 : 0];
#if defined(_WINDOWS) && !defined(_CONSOLE)
    gVideo = this;
    LoadStringA(video::win_hInstance, IDC_SEISMICSIMULATION, szWindowClass, MAX_LOADSTRING);
    memset(&wcex, 0, sizeof(wcex));
    wcex.lpfnWndProc = (WNDPROC)WndProc;
    wcex.hIcon = LoadIcon(video::win_hInstance, MAKEINTRESOURCE(IDI_SEISMICSIMULATION));
    wcex.hCursor = LoadCursor(nullptr, IDC_ARROW);
    wcex.hbrBackground = (HBRUSH)(COLOR_WINDOW + 1);
    wcex.lpszMenuName = LPCTSTR(IDC_SEISMICSIMULATION);
    wcex.lpszClassName = szWindowClass;
    wcex.hIconSm = LoadIcon(video::win_hInstance, MAKEINTRESOURCE(IDI_SMALL));
    win_set_class(wcex); // ascii convention here
    win_load_accelerators(IDC_SEISMICSIMULATION);
#endif
}

#if defined(_WINDOWS) && !defined(_CONSOLE)

//  FUNCTION: WndProc(HWND, unsigned, WORD, LONG)

//  PURPOSE:  Processes messages for the main window.

//  WM_COMMAND  - process the application menu
//  WM_PAINT    - Paint the main window
//  WM_DESTROY  - post a quit message and return

LRESULT CALLBACK About(HWND hDlg, UINT message, WPARAM wParam, LPARAM lParam) {
    switch (message) {
        case WM_INITDIALOG: return TRUE;
        case WM_COMMAND:
            if (LOWORD(wParam) == IDOK || LOWORD(wParam) == IDCANCEL) {
                EndDialog(hDlg, LOWORD(wParam));
                return TRUE;
            }
            break;
    }
    return FALSE;
}

LRESULT CALLBACK WndProc(HWND hWnd, UINT message, WPARAM wParam, LPARAM lParam) {
    int wmId;
    switch (message) {
        case WM_COMMAND:
            wmId = LOWORD(wParam);
            // Parse the menu selections:
            switch (wmId) {
                case IDM_ABOUT:
                    DialogBox(
                        video::win_hInstance, MAKEINTRESOURCE(IDD_ABOUTBOX), hWnd, (DLGPROC)About);
                    break;
                case IDM_EXIT: PostQuitMessage(0); break;
                case ID_FILE_PARALLEL: gVideo->on_key('p'); break;
                case ID_FILE_SERIAL: gVideo->on_key('s'); break;
                case ID_FILE_ENABLEGUI: gVideo->on_key('e'); break;
                case ID_FILE_DISABLEGUI: gVideo->on_key('d'); break;
                default: return DefWindowProc(hWnd, message, wParam, lParam);
            }
            break;
        default: return DefWindowProc(hWnd, message, wParam, lParam);
    }
    return 0;
}

#endif
