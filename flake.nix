{
  description = "Linux tray app that ducks non-voice audio while remote voice is active";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        lib = pkgs.lib;
        source = lib.cleanSourceWith {
          src = ./.;
          filter = path: type:
            let name = baseNameOf path;
            in lib.cleanSourceFilter path type
            && !(type == "directory" && lib.elem name [ ".direnv" "target" ])
            && !(lib.elem name [ "result" "result-bin" ]);
        };
        pwDuck = self.packages.${system}.default;
        appScript = name: args:
          pkgs.writeShellScript "pw-duck-app-${name}" ''
            exec ${pwDuck}/bin/pw-duck ${args} "$@"
          '';
        appMeta = description: { inherit description; };
      in
      {
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "pw-duck";
          version = "0.2.3";
          src = source;

          cargoLock.lockFile = ./Cargo.lock;
          buildFeatures = [ "gui" ];

          nativeBuildInputs = with pkgs; [
            pkg-config
            wrapGAppsHook4
            desktop-file-utils
            llvmPackages.libclang
            rustPlatform.bindgenHook
          ];

          buildInputs = with pkgs; [
            gtk4
            pipewire
          ];

          LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";

          preFixup = ''
            gappsWrapperArgs+=(--prefix PATH : ${pkgs.lib.makeBinPath [ pkgs.coreutils pkgs.pipewire pkgs.pulseaudio ]})
          '';

          postInstall = ''
            install -Dm644 assets/applications/pw-duck.desktop \
              $out/share/applications/pw-duck.desktop
            desktop-file-validate $out/share/applications/pw-duck.desktop

            mkdir -p $out/share/icons
            cp -r assets/icons/hicolor $out/share/icons/hicolor

            install -Dm644 README.md $out/share/doc/pw-duck/README.md
            install -Dm644 LICENSE $out/share/doc/pw-duck/LICENSE
          '';

          meta = {
            description = "Linux tray app that ducks non-voice audio while remote voice is active";
            license = lib.licenses.mit;
            mainProgram = "pw-duck";
            platforms = lib.platforms.linux;
          };
        };

        apps = {
          default = {
            type = "app";
            program = "${pwDuck}/bin/pw-duck";
            meta = appMeta "Start the pw-duck tray by default";
          };

          tray = {
            type = "app";
            program = "${appScript "tray" "tray"}";
            meta = appMeta "Start the pw-duck StatusNotifier tray";
          };

          tune-gui = {
            type = "app";
            program = "${appScript "tune-gui" "tune-gui"}";
            meta = appMeta "Open the graphical pw-duck tuner";
          };

          tune = {
            type = "app";
            program = "${appScript "tune" "tune"}";
            meta = appMeta "Open the terminal pw-duck tuner";
          };
        };

        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            rustc
            cargo
            rustfmt
            clippy
            rust-analyzer
            pkg-config
            pipewire
            pulseaudio
            wireplumber
            gtk4
            desktop-file-utils
            llvmPackages.clang
            llvmPackages.libclang
          ];

          LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";

          shellHook = ''
            echo "pw-duck devshell ready."
          '';
        };
      });
}
