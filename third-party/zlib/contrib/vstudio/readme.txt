The pre-made Visual Studio projects have been removed in lieu of the ability to
use cmake to build such projects on demand.

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


See contrib/zlib1-dll/readme.txt for how to build the legacy zlib1.dll.
