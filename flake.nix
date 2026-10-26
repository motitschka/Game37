{
  description = "Game37: a console-style game library for Nintendo and Sony Systems on Linux";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  outputs =
    { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
      lib = pkgs.lib;
      cargoToml = lib.importTOML ./Cargo.toml;
      # The libretro Cores Game37 downloads are built for ordinary distributions and need these
      # libraries, which NixOS doesn't have in /usr/lib. Linking them into Game37 loads them when it
      # starts, so a Core finds them already loaded. glibc's own libraries need nothing extra.
      coreLibs = [ "stdc++" "gcc_s" "gomp" "z" "GL" ];
    in
    {
      packages.${system}.default = pkgs.rustPlatform.buildRustPackage {
        pname = "game37";
        inherit (cargoToml.package) version;
        src = lib.cleanSource self;
        cargoLock.lockFile = ./Cargo.lock;

        nativeBuildInputs = [ pkgs.pkg-config pkgs.makeWrapper pkgs.copyDesktopItems ];
        buildInputs = [ pkgs.sdl3 pkgs.sdl3-ttf pkgs.zlib pkgs.libGL ];
        RUSTFLAGS = lib.concatMapStringsSep " " (arg: "-C link-arg=${arg}") (
          [ "-Wl,--push-state,--no-as-needed" ] ++ map (l: "-l${l}") coreLibs ++ [ "-Wl,--pop-state" ]
        );

        # Game37 finds its font with fontconfig's fc-match.
        postInstall = ''
          wrapProgram $out/bin/game37 --prefix PATH : ${lib.makeBinPath [ pkgs.fontconfig ]}
          install -Dm644 ${./data/game37.svg} $out/share/icons/hicolor/scalable/apps/game37.svg
        '';

        desktopItems = [
          (pkgs.makeDesktopItem {
            name = "game37";
            desktopName = "Game37";
            comment = "Play your Nintendo and Sony games";
            exec = "game37";
            icon = "game37";
            startupWMClass = "game37";
            categories = [ "Game" "Emulator" ];
          })
        ];

        meta = {
          description = "Console-style game library for Nintendo and Sony Systems";
          homepage = "https://github.com/motitschka/Game37";
          license = lib.licenses.gpl3Plus;
          mainProgram = "game37";
          platforms = [ "x86_64-linux" ];
        };
      };
    };
}
