{
  description = "pw-duck: PipeWire auto-ducking for app playback streams (Rust)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };

        pw-duck = pkgs.rustPlatform.buildRustPackage {
          pname = "pw-duck";
          version = "0.1.0";

          src = self;

          cargoLock = {
            lockFile = ./Cargo.lock;
          };

          nativeBuildInputs = with pkgs; [
            pkg-config
            llvmPackages.clang
          ];

          buildInputs = with pkgs; [
            pipewire
          ];

          LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
          PKG_CONFIG_PATH = "${pkgs.pipewire.dev}/lib/pkgconfig";
        };
      in
      {
        packages = {
          default = pw-duck;
          pw-duck = pw-duck;
        };

        apps = {
          default = flake-utils.lib.mkApp { drv = pw-duck; exePath = "/bin/pw-duck"; };
          pw-duck = flake-utils.lib.mkApp { drv = pw-duck; exePath = "/bin/pw-duck"; };
        };

        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            rustc cargo
            rustfmt clippy
            pkg-config
            pipewire
            wireplumber
            llvmPackages.clang
            llvmPackages.libclang
          ];
          PKG_CONFIG_PATH = "${pkgs.pipewire.dev}/lib/pkgconfig";
          LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
          shellHook = ''
            echo "pw-duck devshell ready."
          '';
        };
      });
}
