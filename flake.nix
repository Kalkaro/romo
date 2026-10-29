{
  description = "Romo — a native, offline Rust whiteboard";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  inputs.crane.url = "github:ipetkov/crane";

  outputs = { self, nixpkgs, crane }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
      pkgsFor = system: import nixpkgs { inherit system; };
      libraries = pkgs: with pkgs; [ openssl ] ++ lib.optionals stdenv.hostPlatform.isLinux [
        libGL libxkbcommon wayland libX11 libXcursor libXi libXrandr libxcb
        dbus fontconfig freetype vulkan-loader zlib
      ];
    in {
      packages = forAllSystems (system:
        let
          pkgs = pkgsFor system;
          craneLib = crane.mkLib pkgs;
          commonArgs = {
            pname = "romo";
            version = "0.1.0";
            src = pkgs.lib.fileset.toSource {
              root = ./.;
              fileset = pkgs.lib.fileset.unions [ ./Cargo.toml ./Cargo.lock ./src ./assets ];
            };
            strictDeps = true;
            nativeBuildInputs = with pkgs; [ pkg-config makeWrapper rustPlatform.bindgenHook ];
            buildInputs = libraries pkgs;
          };
          mkPackage = { runTests ? true }:
            let args = commonArgs // { CARGO_PROFILE = "release"; };
            in craneLib.buildPackage (args // {
              # Cache compiled crates separately from Romo's source and assets.
              cargoArtifacts = craneLib.buildDepsOnly (args // {
                doCheck = false;
                # This cache serves builds; avoid a separate metadata-only check pass.
                buildPhaseCargoCommand = "cargoWithProfile build --locked";
              });
              doCheck = runTests;
              postInstall = ''
                install -Dm644 assets/romo.svg $out/share/icons/hicolor/scalable/apps/romo.svg
                install -Dm644 assets/romo.desktop $out/share/applications/romo.desktop
              '' + pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux ''
                wrapProgram $out/bin/romo \
                  --prefix LD_LIBRARY_PATH : ${pkgs.lib.makeLibraryPath (libraries pkgs)} \
                  --prefix PATH : ${pkgs.lib.makeBinPath [ pkgs.zenity ]}
              '';
              meta = with pkgs.lib; {
                description = "Native offline whiteboard with notes, shapes, drawing, and local board files";
                license = licenses.mit;
                mainProgram = "romo";
                platforms = platforms.linux ++ platforms.darwin;
              };
            });
        in {
          default = mkPackage { };
          fast = mkPackage { runTests = false; };
        });

      apps = forAllSystems (system: {
        default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/romo";
          meta.description = "Open the Romo whiteboard";
        };
      });

      checks = forAllSystems (system: {
        default = self.packages.${system}.default;
      });

      devShells = forAllSystems (system:
        let pkgs = pkgsFor system;
        in {
          default = pkgs.mkShell {
            packages = with pkgs; [ cargo rustc rustfmt clippy pkg-config ]
              ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [ zenity ];
            nativeBuildInputs = [ pkgs.rustPlatform.bindgenHook ];
            buildInputs = libraries pkgs;
            LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath (libraries pkgs);
          };
        });
    };
}
