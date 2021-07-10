let
  # Pin some fairly new nixpkgs
  sources = builtins.fetchTarball {
    name = "nixpkgs-unstable-2021-07-06";
    url = "https://github.com/nixos/nixpkgs/archive/291b3ff5af268eb7a656bb11c73f2fe535ea2170.tar.gz";
    sha256 = "1z2l7q4cmiaqb99cd8yfisdr1n6xbwcczr9020ss47y2z1cn1x7x";
  };

  # For bootstrapping, and also as comparison baseline
  pkgs = import sources {
    overlays = [
      (pkgs: super: {
        # TODO replace with proper packaging once https://github.com/NixOS/nixpkgs/pull/128889 is merged
        mold = pkgs.stdenv.mkDerivation {
          name = "mold";
          src = ./.;
          buildInputs = with pkgs; [ zlib openssl ];
          nativeBuildInputs = with pkgs; [ autoPatchelfHook cmake xxHash ];
          dontUseCmakeConfigure = true;
          buildPhase = "make -j $NIX_BUILD_CORES";
          installPhase = "make PREFIX=$out install";
        };

        binutils_mold = pkgs.wrapBintoolsWith {
          bintools = pkgs.binutils-unwrapped.overrideAttrs (old: {
            postInstall = "ln -sf ${pkgs.mold}/bin/mold $out/bin/ld";
          });
        };
        
        stdenv_mold = super.overrideCC super.stdenv (super.wrapCCWith rec {
          cc = super.gcc-unwrapped;
          bintools = pkgs.binutils_mold;
        });
      })
    ];
  };

  # Actual nixpkgs with patched linker in all packages. Note:
  # - This will bootstrap the entire toolchain and build all transitive dependencies every time mold changes.
  # - Packages that require a custom linker (like lld or some special GCC) won't be built with mold regardless.
  pkgsMold = import sources {
    overlays = [
      (self: super: {
        stdenv = pkgs.stdenv_mold;
        mold = pkgs.mold;
      })
    ];
  };
  
  # Like pkgsMold, but we disable mold individually for known failing packages until their issues are resolved
  pkgsMoldCI = pkgsMold.appendOverlays [
    (self: super: {
      inherit (pkgs)
        valgrind # https://github.com/rui314/mold/issues/81#issuecomment-876425070
      ;
    })
  ];
in {
  inherit pkgs pkgsMold pkgsMoldCI;

  # Packages we already know that work in order to catch regressions
  ciPackages = with pkgsMoldCI; linkFarmFromDrvs "packages-with-mold" [
    binutils
    stdenv
    # TODO
  ];
}
