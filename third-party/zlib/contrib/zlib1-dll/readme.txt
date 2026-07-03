Use this directory to build the legacy zlib1.dll for Windows, which contains
both zlib and minizip. Use cmake either at the command prompt, or with Visual
Studio as outlined below.


To create a Visual Studio project
---------------------------------

1. Start cmake-gui.
2. Point source-dir to the source.
3. Point build-dir to the dir where you want to build.
4. Hit configure -- there you can select details.
5. Select the options you want, which are shown with descriptions after the
   configure run is complete.
6. Hit configure again to assure that everything that is needed is found.
7. For those not found, deactivate the option or install the dependency, e.g.
   bzip2 for minizip, and go back to step 6 until there is no red.
8. Hit generate.
9. Hit open project.

Now you can Build > Build solution.
