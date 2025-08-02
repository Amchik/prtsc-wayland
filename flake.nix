{
  description = "Screenshot utility for Wayland";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
      rustPlatform = pkgs.makeRustPlatform {
        cargo = pkgs.cargo;
        rustc = pkgs.rustc;
      };

      prtsc-wayland = rustPlatform.buildRustPackage {
        pname = "prtsc-wayland";
        version = "0.3.0";
        src = ./.;

        cargoLock = {
          lockFile = ./Cargo.lock;
        };

        nativeBuildInputs = with pkgs; [
          pkg-config
        ];

        buildInputs = with pkgs; [
          wayland
          libxkbcommon
        ];
      };
    in
    {
      packages = {
        prtsc-wayland = prtsc-wayland;
        default = prtsc-wayland;
      };

      devShells.${system}.default = pkgs.mkShell {
        buildInputs = with pkgs; [
          rustc
          cargo
          pkg-config
          wayland
          libxkbcommon
        ];
      };
    };
}

