{
  description = "TransitGuard Rust development environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { nixpkgs, ... }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs {
        inherit system;
      };
    in
    {
      devShells.${system}.default = pkgs.mkShell {
        packages = with pkgs; [
          rustc
          cargo
          rustfmt
          clippy
          rust-analyzer

          pkg-config
          openssl.dev
          postgresql

          git
          gh
          just
        ];

        RUST_BACKTRACE = "1";

        shellHook = ''
          echo
          echo "TransitGuard Rust development environment"
          echo "Rust: $(rustc --version)"
          echo "Cargo: $(cargo --version)"
          echo
        '';
      };
    };
}
