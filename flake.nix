{
  description = "Token-efficient code intelligence and editing tools for AI agents";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in {
      packages = forAllSystems (system:
        let
          pkgs = import nixpkgs { inherit system; };
          manifest = builtins.fromTOML (builtins.readFile ./Cargo.toml);
          goldeneye = pkgs.rustPlatform.buildRustPackage {
            pname = "goldeneye-tool";
            version = manifest.workspace.package.version;
            src = self;
            cargoLock.lockFile = ./Cargo.lock;
            cargoBuildFlags = [ "--package" "goldeneye" ];
            cargoTestFlags = [ "--package" "goldeneye" ];
            postInstall = ''
              install -Dm644 LICENSE "$out/share/doc/goldeneye-tool/LICENSE"
              install -Dm644 NOTICE "$out/share/doc/goldeneye-tool/NOTICE"
            '';
            meta = {
              description = "Token-efficient code intelligence and editing tools for AI agents";
              homepage = "https://github.com/GoldeneyeZ/goldeneye-tool";
              license = pkgs.lib.licenses.mit;
              mainProgram = "goldeneye";
              platforms = pkgs.lib.platforms.unix;
            };
          };
        in {
          inherit goldeneye;
          default = goldeneye;
        });

      apps = forAllSystems (system: {
        default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/goldeneye";
        };
      });

      checks = forAllSystems (system: {
        package = self.packages.${system}.default;
      });
    };
}
