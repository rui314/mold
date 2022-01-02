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

/*
    The original source for this example is
    Copyright (c) 1994-2008 John E. Stone
    All rights reserved.

    Redistribution and use in source and binary forms, with or without
    modification, are permitted provided that the following conditions
    are met:
    1. Redistributions of source code must retain the above copyright
       notice, this list of conditions and the following disclaimer.
    2. Redistributions in binary form must reproduce the above copyright
       notice, this list of conditions and the following disclaimer in the
       documentation and/or other materials provided with the distribution.
    3. The name of the author may not be used to endorse or promote products
       derived from this software without specific prior written permission.

    THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS
    OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
    WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
    ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY
    DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
    DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
    OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
    HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
    LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
    OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
    SUCH DAMAGE.
*/

#include <omp.h>
#include <string.h>

#include "machine.hpp"
#include "types.hpp"
#include "macros.hpp"
#include "vector.hpp"
#include "tgafile.hpp"
#include "trace.hpp"
#include "light.hpp"
#include "shade.hpp"
#include "camera.hpp"
#include "util.hpp"
#include "intersect.hpp"
#include "global.hpp"
#include "ui.hpp"
#include "tachyon_video.hpp"

// shared but read-only so could be private too
static thr_parms *all_parms;
static scenedef scene;
static int startx;
static int stopx;
static int starty;
static int stopy;
static flt jitterscale;
static int totaly, totalx;

static int grain_size = 50;
const int DIVFACTOR = 2;

#define MIN(a, b) ((a) < (b) ? (a) : (b))

static color_t render_one_pixel(int x,
                                int y,
                                unsigned int *local_mbox,
                                unsigned int &serial,
                                int startx,
                                int stopx,
                                int starty,
                                int stopy) {
    /* private vars moved inside loop */
    ray primary, sample;
    color col, avcol;
    int R, G, B;
    intersectstruct local_intersections;
    int alias;
    /* end private */

    primary = camray(&scene, x, y);
    primary.intstruct = &local_intersections;
    primary.flags = RT_RAY_REGULAR;

    serial++;
    primary.serial = serial;
    primary.mbox = local_mbox;
    primary.maxdist = FHUGE;
    primary.scene = &scene;
    col = trace(&primary);

    serial = primary.serial;

    /* perform antialiasing if enabled.. */
    if (scene.antialiasing > 0) {
        for (alias = 0; alias < scene.antialiasing; alias++) {
            serial++; /* increment serial number */
            sample = primary; /* copy the regular primary ray to start with */
            sample.serial = serial;

#pragma omp critical
            {
                sample.d.x += ((rand() % 100) - 50) / jitterscale;
                sample.d.y += ((rand() % 100) - 50) / jitterscale;
                sample.d.z += ((rand() % 100) - 50) / jitterscale;
            }

            avcol = trace(&sample);

            serial = sample.serial; /* update our overall serial # */

            col.r += avcol.r;
            col.g += avcol.g;
            col.b += avcol.b;
        }

        col.r /= (scene.antialiasing + 1.0);
        col.g /= (scene.antialiasing + 1.0);
        col.b /= (scene.antialiasing + 1.0);
    }

    /* Handle overexposure and underexposure here... */
    R = (int)(col.r * 255);
    if (R > 255)
        R = 255;
    else if (R < 0)
        R = 0;

    G = (int)(col.g * 255);
    if (G > 255)
        G = 255;
    else if (G < 0)
        G = 0;

    B = (int)(col.b * 255);
    if (B > 255)
        B = 255;
    else if (B < 0)
        B = 0;

    return video->get_color(R, G, B);
}

static void parallel_thread(patch *pchin, int depth) {
    unsigned char col[3];
    col[0] = col[1] = col[2] = (32 * depth) % 256;
    depth++;
#pragma intel omp taskq firstprivate(depth)
    {
        int startx, stopx, starty, stopy;
        int xs, ys;

        startx = pchin->startx;
        stopx = pchin->stopx;
        starty = pchin->starty;
        stopy = pchin->stopy;

        if (((stopx - startx) >= grain_size) || ((stopy - starty) >= grain_size)) {
            int xpatchsize = (stopx - startx) / DIVFACTOR + 1;
            int ypatchsize = (stopy - starty) / DIVFACTOR + 1;
            for (ys = starty; ys <= stopy; ys += ypatchsize)
                for (xs = startx; xs <= stopx; xs += xpatchsize) {
                    patch pch;
                    pch.startx = xs;
                    pch.starty = ys;
                    pch.stopx = MIN(xs + xpatchsize, stopx);
                    pch.stopy = MIN(ys + ypatchsize, stopy);

#pragma intel omp task
                    parallel_thread(&pch, depth);
                }
        }
        else {
            /* just trace this patch */
            unsigned int mboxsize = sizeof(unsigned int) * (max_objectid() + 20);
            unsigned int *local_mbox = (unsigned int *)alloca(mboxsize);
            memset(local_mbox, 0, mboxsize);

            drawing_area drawing(startx, totaly - stopy, stopx - startx, stopy - starty);
            for (int i = 1, y = starty; y < stopy; ++y, i++) {
                if (!video->running)
                    continue;
                drawing.set_pos(0, drawing.size_y - i);
                unsigned int serial = 5 * ((stopx - startx) + (stopy - starty) * totalx);
                for (int x = startx; x < stopx; x++) {
                    color_t c =
                        render_one_pixel(x, y, local_mbox, serial, startx, stopx, starty, stopy);
                    drawing.put_pixel(c);
                }
            }
            video->next_frame();
        }
    }
}

void *thread_trace(thr_parms *parms) {
    // shared but read-only so could be private too
    all_parms = parms;
    scene = parms->scene;
    startx = parms->startx;
    stopx = parms->stopx;
    starty = parms->starty;
    stopy = parms->stopy;
    jitterscale = 40.0 * (scene.hres + scene.vres);
    totalx = parms->stopx - parms->startx + 1;
    totaly = parms->scene.vres;

    patch pch;
    pch.startx = startx;
    pch.stopx = stopx;
    pch.starty = starty;
    pch.stopy = stopy;
    int g;
    char *grain_str = getenv("TASKQ_GRAINSIZE");
    if (grain_str && (sscanf(grain_str, "%d", &g) > 0) && (g > 0))
        grain_size = g;
#pragma omp parallel
    parallel_thread(&pch, 0);

    return (nullptr);
}
