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
      in
      {
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
