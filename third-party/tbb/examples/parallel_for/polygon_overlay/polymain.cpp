/*
    Copyright (c) 2005-2022 Intel Corporation

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

// Polygon overlay
//
// Don't want warnings about deprecated sscanf, getenv
#ifndef _CRT_SECURE_NO_DEPRECATE
#define _CRT_SECURE_NO_DEPRECATE
#endif
#define _MAIN_C_ 1

#include <cstring>

#include <iostream>
#include <iomanip>
#include <algorithm>

#include "oneapi/tbb/tick_count.h"

#include "pover_global.hpp"
#include "polyover.hpp"
#include "pover_video.hpp"
#include "polymain.hpp"

#if _DEBUG
const char *faceNames[] = { "North", "East", "South", "West" };
#endif

/**
**/
int main(int argc, char *argv[]) {
    pover_video poly;
    poly.threaded = true;
    gVideo = &poly;

    if (!initializeVideo(argc, argv)) {
        return -1;
    }

    gIsGraphicalVersion = poly.graphic_display();
    if (argc > 1) {
        if (!ParseCmdLine(argc, argv)) {
            if (gIsGraphicalVersion)
                rt_sleep(10000);
            // if graphical, we haven't opened the console window so all the error messages we
            // so carefully wrote out disappeared into the ether.  :(
            return -1;
        }
    }

    if (gCsvFilename != nullptr) {
#define BUFLEN 1000
        std::string fname_buf = gCsvFilename;
        fname_buf += ".csv";
        gCsvFile.open(fname_buf.c_str());
    }

    // we have gMapXSize and gMapYSize determining the number of "squares"
    // we have g_xwinsize and g_ywinsize the total size of the window
    // we also have BORDER_SIZE the size of the border between maps
    // we need to determine
    //      g_polyBoxSize -- the number of pixels on each size of each square

    if (gIsGraphicalVersion) {
        int xpixelsPerMap =
            (g_xwinsize - 4 * BORDER_SIZE) / 3; // three maps, with borders between and outside
        gMapXSize = xpixelsPerMap; // make the boxes one per pixel
        gPolyXBoxSize = xpixelsPerMap / gMapXSize;
        int ypixelsPerMap = (g_ywinsize - 2 * BORDER_SIZE); // one map vertically
        gMapYSize = ypixelsPerMap; // one pixel per box, rather.

        gPolyYBoxSize = ypixelsPerMap / gMapYSize;
        if ((gPolyXBoxSize == 0) || (gPolyYBoxSize == 0)) {
            std::cout << "The display window is not large enough to show the maps"
                      << "\n";
            int minxSize = 4 * BORDER_SIZE + 3 * gMapXSize;
            int minySize = 2 * BORDER_SIZE + gMapYSize;
            std::cout << "  Should be at least " << minxSize << " x " << minySize << "."
                      << "\n";
            return -1;
        }
        map2XLoc = 2 * BORDER_SIZE + gMapXSize * gPolyXBoxSize;
        maprXLoc = 3 * BORDER_SIZE + 2 * gMapXSize * gPolyXBoxSize;
    }
    else { // not gIsGraphicalVersion
        // gMapXSize, gMapYSize, gNPolygons defined in pover_global.h
    }

    // create two polygon maps
    SetRandomSeed(gMyRandomSeed); // for repeatability

    gVideo->main_loop();

    return 0;
}

void Usage(int argc, char *argv[]) {
    char *cmdTail = strrchr(*argv, '\\');
    if (cmdTail == nullptr) {
        cmdTail = *argv;
    }
    else {
        cmdTail++;
    }
    std::cout
        << cmdTail
        << " [threads[:threads2]] [--polys npolys] [--size nnnxnnn] [--seed nnn] [--csv filename] [--grainsize n] [--use_malloc]"
        << "\n";
    std::cout << "Create polygon maps and overlay them."
              << "\n"
              << "\n";
    std::cout << "Parameters:"
              << "\n";
    std::cout << "   threads[:threads2] - number of threads to run"
              << "\n";
    std::cout << "   --polys npolys - number of polygons in each map"
              << "\n";
    std::cout << "   --size nnnxnnn - size of each map (X x Y)"
              << "\n";
    std::cout << "   --seed nnn - initial value of random number generator"
              << "\n";
    std::cout << "   --csv filename - write timing data to CSV-format file"
              << "\n";
    std::cout << "   --grainsize n - set grainsize to n"
              << "\n";
    std::cout << "   --use_malloc - allocate polygons with malloc instead of scalable allocator"
              << "\n";
    std::cout << "\n";
    std::cout << "npolys must be smaller than the size of the map"
              << "\n";
    std::cout << "\n";
    std::exit(-1);
}

bool ParseCmdLine(int argc, char *argv[]) {
    bool error_found = false;
    bool nPolysSpecified = false;
    bool nMapSizeSpecified = false;
    bool nSeedSpecified = false;
    bool csvSpecified = false;
    bool grainsizeSpecified = false;
    bool mallocSpecified = false;
    int origArgc = argc;
    char **origArgv = argv;
    unsigned int newnPolygons = gNPolygons;
    unsigned int newSeed = gMyRandomSeed;
    unsigned int newX = gMapXSize;
    unsigned int newY = gMapYSize;
    unsigned int newGrainSize = gGrainSize;
    argc--;
    argv++;
    if (argc > 0 && isdigit((*argv)[0])) {
        // first argument is one or two numbers, specifying how mny threads to run
        char *end;
        gThreadsHigh = gThreadsLow = (int)strtol(argv[0], &end, 0);
        switch (*end) {
            case ':': gThreadsHigh = (int)strtol(end + 1, nullptr, 0); break;
            case '\0': break;
            default:
                std::cout << "Unexpected character in thread specifier: " << *end << "\n";
                break;
        }
        if (gThreadsLow > gThreadsHigh) {
            int t = gThreadsLow;
            gThreadsLow = gThreadsHigh;
            gThreadsHigh = t;
        }
        argv++;
        argc--;
    }
    while (argc > 0) {
        // format 1: --size nnnxnnn, where nnn in {0 .. 9}+ -- size of map in "squares"
        if (!strncmp("--size", *argv, (std::size_t)6)) {
            if (nMapSizeSpecified) {
                std::cout << " Error: map size multiply specified"
                          << "\n";
                error_found = true;
            }
            else {
                argv++;
                argc--;
                if (argc == 0) {
                    error_found = true;
                    std::cout << " Error: --size must have a value"
                              << "\n";
                }
                if (strchr(*argv, 'x') != strrchr(*argv, 'x')) {
                    // more than one 'x'
                    std::cout << "Error: map size should be nnnxnnn (" << *argv << ")"
                              << "\n";
                    error_found = true;
                }
                else {
                    int rval;
                    rval = sscanf(*argv, "%ux%u", &newX, &newY);
                    if (rval != 2) {
                        std::cout << "Error parsing map size (format should be nnnxnnn (" << *argv
                                  << ")"
                                  << "\n";
                        error_found = true;
                    }
                    if (newX == 0 || newY == 0) {
                        std::cout << "Error: size of map should be greater than 0 (" << *argv << ")"
                                  << "\n";
                        error_found = true;
                    }
                }
            }
            argc--;
            argv++;
        }
        // format 2: --seed nnn -- initial random number seed
        else if (!strncmp("--seed", *argv, (std::size_t)6)) {
            argv++;
            argc--;
            if (nSeedSpecified) {
                std::cout << "Error: new seed multiply specified"
                          << "\n";
                error_found = true;
            }
            else {
                nSeedSpecified = true;
                int rtval = sscanf(*argv, "%u", &newSeed);
                if (rtval == 0) {
                    std::cout << "Error: --seed should be an unsigned number (instead of " << *argv
                              << ")"
                              << "\n";
                    error_found = true;
                }
            }
            argv++;
            argc--;
        }
        // format 3: --polys n[n] -- number of polygons in each map
        else if (!strncmp("--polys", *argv, (std::size_t)7)) {
            //unsigned int newnPolygons;
            argv++;
            argc--;
            if (nPolysSpecified) {
                std::cout << "Error: number of polygons multiply-specified"
                          << "\n";
                error_found = true;
            }
            else {
                int rtval = sscanf(*argv, "%u", &newnPolygons);
                if (newnPolygons == 0) {
                    std::cout << "Error: number of polygons must be greater than 0 (" << *argv
                              << ")"
                              << "\n";
                }
            }
            argv++;
            argc--;
        }
        // format 4: --csv <fileroot> -- name of CSV output file ("xxx" for "xxx.csv")
        else if (!strncmp("--csv", *argv, (std::size_t)5)) {
            argv++;
            argc--;
            if (csvSpecified) {
                std::cout << "Error: Multiple specification of CSV file"
                          << "\n";
                error_found = true;
            }
            else {
                gCsvFilename = *argv;
                argv++;
                argc--;
                csvSpecified = true;
            }
        }
        else if (!strncmp("--grainsize", *argv, (std::size_t)11)) {
            argv++;
            argc--;
            if (grainsizeSpecified) {
                std::cout << "Error: Multiple specification of grainsize"
                          << "\n";
                error_found = true;
            }
            else {
                int grval = sscanf(*argv, "%u", &newGrainSize);
                grainsizeSpecified = true;
                if (newGrainSize == 0) {
                    std::cout << "Error: grainsize must be greater than 0"
                              << "\n";
                    error_found = true;
                }
            }
            argv++;
            argc--;
        }
        else if (!strncmp("--use_malloc", *argv, (std::size_t)12)) {
            argv++;
            argc--;
            if (mallocSpecified) {
                std::cout << "Error: --use_malloc multiply-specified"
                          << "\n";
                error_found = true;
            }
            else {
                mallocSpecified = true;
                gMBehavior = UseMalloc;
            }
        }
        else {
            std::cout << "Error: unrecognized argument: " << *argv << "\n";
            error_found = true;
            argv++;
            argc--;
        }
    }
    if (!error_found) {
        if (newX * newY < newnPolygons) {
            error_found = true;
            std::cout
                << "Error: map size should not be smaller than the number of polygons (gNPolygons = "
                << newnPolygons << ", map size " << newX << "x" << newY << ")"
                << "\n";
        }
    }
    if (!error_found) {
        gMapXSize = newX;
        gMapYSize = newY;
        gNPolygons = newnPolygons;
        gMyRandomSeed = newSeed;
        gGrainSize = (int)newGrainSize;
    }
    else {
        Usage(origArgc, origArgv);
    }
    return !error_found;
}

// create a polygon map with at least gNPolygons polygons.
// Usually more than gNPolygons polygons will be generated, because the
// process of growing the polygons results in holes.
bool GenerateMap(Polygon_map_t **newMap,
                 int xSize,
                 int ySize,
                 int gNPolygons,
                 colorcomp_t maxR,
                 colorcomp_t maxG,
                 colorcomp_t maxB) {
    bool error_found = false;
    int *validPolys;
    int *validSide;
    int maxSides;
    RPolygon *newPoly;

    if (xSize <= 0) {
        std::cout << "xSize (" << xSize << ") should be > 0."
                  << "\n";
        error_found = true;
    }
    if (ySize <= 0) {
        std::cout << "ySize (" << ySize << ") should be > 0."
                  << "\n";
        error_found = true;
    }
    if (gNPolygons > (xSize * ySize)) {
        std::cout << "gNPolygons (" << gNPolygons << ") should be less than " << (xSize * ySize)
                  << "\n";
        error_found = true;
    }
    if (error_found)
        return false;
    // the whole map is [xSize x ySize] squares
    // the way we create the map is to
    //    1) pick nPolygon discrete squares on an [xSize x ySize] grid
    //    2) while there are unused squares on the grid
    //        3) pick a polygon with a side that has unused squares on a side
    //        4) expand the polygon by 1 to occupy the unused squares
    //
    // Continue until every square on the grid is occupied by a polygon
    int *tempMap;
    tempMap = (int *)malloc(xSize * ySize * sizeof(int));
    for (int i = 0; i < xSize; i++) {
        for (int j = 0; j < ySize; j++) {
            tempMap[i * ySize + j] = 0;
        }
    }

    // *newMap = new vector<RPolygon>;
    *newMap = new Polygon_map_t;
    (*newMap)->reserve(gNPolygons + 1); // how much bigger does this need to be on average?
    (*newMap)->push_back(RPolygon(0, 0, xSize - 1, ySize - 1));
    for (int i = 0; i < gNPolygons; i++) {
        int nX;
        int nY;
        do { // look for an empty square.
            nX = NextRan(xSize);
            nY = NextRan(ySize);
        } while (tempMap[nX * ySize + nY] != 0);
        int nR = (maxR * NextRan(1000)) / 999;
        int nG = (maxG * NextRan(1000)) / 999;
        int nB = (maxB * NextRan(1000)) / 999;
        (*newMap)->push_back(RPolygon(nX, nY, nX, nY, nR, nG, nB));
        tempMap[nX * ySize + nY] = i + 1; // index of this polygon + 1
    }
    // now have to grow polygons to fill the space.
    validPolys = (int *)malloc(4 * gNPolygons * sizeof(int));
    validSide = (int *)malloc(4 * gNPolygons * sizeof(int));
    for (int i = 0; i < gNPolygons; i++) {
        validPolys[4 * i] = validPolys[4 * i + 1] = validPolys[4 * i + 2] = validPolys[4 * i + 3] =
            i + 1;
        validSide[4 * i] = NORTH_SIDE;
        validSide[4 * i + 1] = EAST_SIDE;
        validSide[4 * i + 2] = SOUTH_SIDE;
        validSide[4 * i + 3] = WEST_SIDE;
    }
    maxSides = 4 * gNPolygons;
    while (maxSides > 0) {
        int indx = NextRan(maxSides);
        int polyIndx = validPolys[indx];
        int checkSide = validSide[indx];
        int xlow, xhigh, ylow, yhigh;
        int xlnew, xhnew, ylnew, yhnew;
        (**newMap)[polyIndx].get(&xlow, &ylow, &xhigh, &yhigh);
        xlnew = xlow;
        xhnew = xhigh;
        ylnew = ylow;
        yhnew = yhigh;
        // can this polygon be expanded along the chosen side?
        switch (checkSide) {
            case NORTH_SIDE:
                // y-1 from xlow to xhigh
                ylow = yhigh = (ylow - 1);
                ylnew--;
                break;
            case EAST_SIDE:
                // x+1 from ylow to yhigh
                xlow = xhigh = (xhigh + 1);
                xhnew++;
                break;
            case SOUTH_SIDE:
                // y+1 from xlow to xhigh
                ylow = yhigh = (yhigh + 1);
                yhnew++;
                break;
            case WEST_SIDE:
                // x-1 from ylow to yhigh
                xlow = xhigh = (xlow - 1);
                xlnew--;
                break;
        }
        bool okay_to_extend = !(((xlow < 0) || (xlow >= xSize)) || ((ylow < 0) || (ylow >= ySize)));
        for (int i = xlow; (i <= xhigh) && okay_to_extend; i++) {
            for (int j = ylow; (j <= yhigh) && okay_to_extend; j++) {
                okay_to_extend = tempMap[i * ySize + j] == 0;
            }
        }
        if (okay_to_extend) {
            (**newMap)[polyIndx].set(xlnew, ylnew, xhnew, yhnew);
            for (int i = xlow; i <= xhigh; i++) {
                for (int j = ylow; j <= yhigh && okay_to_extend; j++) {
                    tempMap[i * ySize + j] = polyIndx;
                }
            }
        }
        else {
            // once we cannot expand along a side, we will never be able to; remove from the list.
            for (int i = indx + 1; i < maxSides; i++) {
                validPolys[i - 1] = validPolys[i];
                validSide[i - 1] = validSide[i];
            }
            maxSides--;
        }
    }

    // Once no polygons can be grown, look for unused squares, and fill them with polygons.
    for (int j = 0; j < ySize; j++) {
        for (int i = 0; i < xSize; i++) {
            if (tempMap[i * ySize + j] == 0) {
                // try to grow in the x direction, then the y direction
                int ilen = i;
                int jlen = j;
                while (ilen < (xSize - 1) && tempMap[(ilen + 1) * ySize + jlen] == 0) {
                    ilen++;
                }
                bool yok = true;
                while (yok && jlen < (ySize - 1)) {
                    for (int k = i; k <= ilen && yok; k++) {
                        yok = (tempMap[k * ySize + jlen + 1] == 0);
                    }
                    if (yok) {
                        jlen++;
                    }
                }

                // create new polygon and push it on our list.
                int nR = (maxR * NextRan(1000)) / 999;
                int nG = (maxG * NextRan(1000)) / 999;
                int nB = (maxB * NextRan(1000)) / 999;
                (*newMap)->push_back(RPolygon(i, j, ilen, jlen, nR, nG, nB));
                gNPolygons++;
                for (int k = i; k <= ilen; k++) {
                    for (int l = j; l <= jlen; l++) {
                        tempMap[k * ySize + l] = gNPolygons;
                    }
                }
            }
        }
    }

#if _DEBUG
    if (!gIsGraphicalVersion) {
        std::cout << "\n"
                  << "Final Map:"
                  << "\n";
        for (int j = 0; j < ySize; j++) {
            std::cout << "Row " << std::setw(2) << j << ":";
            for (int i = 0; i < xSize; i++) {
                int it = tempMap[i * ySize + j];
                if (it < 10) {
                    std::cout << std::setw(2) << it;
                }
                else {
                    char ct = (int)'a' + it - 10;
                    std::cout << " " << ct;
                }
            }
            std::cout << "\n";
        }
    }
#endif // _DEBUG
    free(tempMap);
    free(validPolys);
    free(validSide);
    return true;
}

void CheckPolygonMap(Polygon_map_t *checkMap) {
#define indx(i, j) (i * gMapYSize + j)
#define rangeCheck(str, n, limit)                                               \
    if (((n) < 0) || ((n) >= limit)) {                                          \
        std::cout << "checkMap error: " << str << " out of range (" << n << ")" \
                  << "\n";                                                      \
        anError = true;                                                         \
    }
#define xRangeCheck(str, n) rangeCheck(str, n, gMapXSize)
#define yRangeCheck(str, n) rangeCheck(str, n, gMapYSize)
    // The first polygon is the whole map.
    bool anError = false;
    int *cArray;
    if (checkMap->size() <= 0) {
        std::cout << "checkMap error: no polygons in map"
                  << "\n";
        return;
    }
    // mapXhigh and mapYhigh are inclusive, that is, if the map is 5x5, those values would be 4.
    int mapXhigh, mapYhigh, mapLowX, mapLowY;
    int gMapXSize, gMapYSize;
    (*checkMap)[0].get(&mapLowX, &mapLowY, &mapXhigh, &mapYhigh);
    if ((mapLowX != 0) || (mapLowY != 0)) {
        std::cout << "checkMap error: map origin not (0,0) (X=" << mapLowX << ", Y=" << mapLowY
                  << ")"
                  << "\n";
        anError = true;
    }
    if ((mapXhigh < 0) || (mapYhigh < 0)) {
        std::cout << "checkMap error: no area in map (X=" << mapXhigh << ", Y=" << mapYhigh << ")"
                  << "\n";
        anError = true;
    }
    if (anError)
        return;
    // bounds for array.
    gMapXSize = mapXhigh + 1;
    gMapYSize = mapYhigh + 1;
    cArray = (int *)malloc(sizeof(int) * (gMapXSize * gMapYSize));

    for (int i = 0; i < gMapXSize; i++) {
        for (int j = 0; j < gMapYSize; j++) {
            cArray[indx(i, j)] = 0;
        }
    }

    int xlow, xhigh, ylow, yhigh;
    for (int k = 1; k < int(checkMap->size()) && !anError; k++) {
        (*checkMap)[k].get(&xlow, &ylow, &xhigh, &yhigh);
        xRangeCheck("xlow", xlow);
        yRangeCheck("ylow", ylow);
        xRangeCheck("xhigh", xhigh);
        yRangeCheck("yhigh", yhigh);
        if (xlow > xhigh) {
            std::cout << "checkMap error: xlow > xhigh (" << xlow << "," << xhigh << ")"
                      << "\n";
            anError = true;
        }
        if (ylow > yhigh) {
            std::cout << "checkMap error: ylow > yhigh (" << ylow << "," << yhigh << ")"
                      << "\n";
            anError = true;
        }
        for (int i = xlow; i <= xhigh; i++) {
            for (int j = ylow; j <= yhigh; j++) {
                if (cArray[indx(i, j)] != 0) {
                    std::cout << "checkMap error: polygons " << cArray[indx(i, j)] << " and " << k
                              << " intersect"
                              << "\n";
                    anError = true;
                }
                cArray[indx(i, j)] = k;
            }
        }
    }
    for (int i = 0; i < gMapXSize; i++) {
        for (int j = 0; j < gMapYSize; j++) {
            if (cArray[indx(i, j)] == 0) {
                std::cout << "checkMap error: block(" << i << ", " << j << ") not in any polygon"
                          << "\n";
                anError = true;
            }
        }
    }
    free(cArray);
}

bool CompOnePolygon(RPolygon &p1, RPolygon &p2) {
    int xl1, xh1, yl1, yh1;
    int xl2, xh2, yl2, yh2;
    p1.get(&xl1, &yl1, &xh1, &yh1);
    p2.get(&xl2, &yl2, &xh2, &yh2);
    if (yl1 > yl2)
        return true;
    if (yl1 < yl2)
        return false;
    return (xl1 > xl2);
}

bool PolygonsEqual(RPolygon *p1, RPolygon *p2) {
    int xl1, xh1, yl1, yh1;
    int xl2, xh2, yl2, yh2;
    p1->get(&xl1, &yl1, &xh1, &yh1);
    p2->get(&xl2, &yl2, &xh2, &yh2);
    return ((xl1 == xl2) && (yl1 == yl2) && (xh1 == xh2) && (yh1 == yh2));
}

bool ComparePolygonMaps(Polygon_map_t *map1, Polygon_map_t *map2) {
    // create two new polygon maps, copy the pointers from the original to these.
    // we have to skip the first polygon, which is the size of the whole map
    Polygon_map_t *t1, *t2;
    bool is_ok = true;
    t1 = new Polygon_map_t;
    t1->reserve(map1->size());
    for (unsigned int i = 1; i < map1->size(); i++) {
        t1->push_back(map1->at(i));
    }
    t2 = new Polygon_map_t;
    t2->reserve(map2->size());
    for (unsigned int i = 1; i < map2->size(); i++) {
        t2->push_back(map2->at(i));
    }
    // sort the two created maps by (xlow, ylow)
    sort(t1->begin(), t1->end());
    sort(t2->begin(), t2->end());
    // compare each element of both maps.
    if (t1->size() != t2->size()) {
        std::cout << "Error: maps not the same size ( " << int(t1->size()) << " vs "
                  << int(t2->size()) << ")."
                  << "\n";
    }
    int maxSize = (int)((t1->size() < t2->size()) ? t1->size() : t2->size());
    for (int i = 0; i < maxSize; i++) {
        if (!PolygonsEqual(&((*t1)[i]), &((*t2)[i]))) {
            std::cout << "Error: polygons unequal (" << (*t1)[i] << " vs " << (*t2)[i] << "\n";
            is_ok = false;
        }
    }
    delete t1;
    delete t2;
    return is_ok;
}

void SetRandomSeed(int newSeed) {
    srand((unsigned)newSeed);
}

int NextRan(int n) {
    // assert(n > 1);
    // if we are given 1, we will just return 0
    //assert(n < RAND_MAX);
    int rrand = rand() << 15 | rand();
    if (rrand < 0)
        rrand = -rrand;
    return rrand % n;
}

std::ostream &operator<<(std::ostream &s, const RPolygon &p) {
    int xl, yl, xh, yh;
    p.get(&xl, &yl, &xh, &yh);
    return s << "[(" << xl << "," << yl << ")-(" << xh << "," << yh << ")] ";
}
