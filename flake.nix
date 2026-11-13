{
  description = "downpour: certflow and rain diagrams for capability authority flows";

  inputs = {
    command-utils = {
      url = "git+https://tangled.org/expede.wtf/nix-command-utils";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    flake-utils.url = "github:numtide/flake-utils";
    nixos-unstable.url = "nixpkgs/nixos-unstable-small";
    nixpkgs.url = "nixpkgs/nixos-26.05";

    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    self,
    command-utils,
    flake-utils,
    nixos-unstable,
    nixpkgs,
    rust-overlay,
  } @ inputs:
    flake-utils.lib.eachDefaultSystem (
      system: let
        overlays = [(import rust-overlay)];

        pkgs = import nixpkgs {inherit system overlays;};
        unstable = import nixos-unstable {inherit system overlays;};

        # Single sources of truth, read from the manifests that own them
        workspaceManifest = builtins.fromTOML (builtins.readFile ./Cargo.toml);
        rustVersion = (builtins.fromTOML (builtins.readFile ./rust-toolchain.toml)).toolchain.channel;
        # rust-overlay keys toolchains by full version (`1.91.0`), while
        # `rust-version` may omit the patch (`1.91`)
        rust-version = workspaceManifest.workspace.package.rust-version;
        msrv =
          if builtins.length (builtins.splitVersion rust-version) == 2
          then "${rust-version}.0"
          else rust-version;

        rust-toolchain = pkgs.rust-bin.stable.${rustVersion}.default.override {
          extensions = [
            "cargo"
            "clippy"
            "llvm-tools-preview"
            "rust-src"
            "rust-std"
          ];

          targets = [
            "aarch64-apple-darwin"
            "x86_64-apple-darwin"

            "aarch64-unknown-linux-musl"
            "x86_64-unknown-linux-musl"

            "thumbv6m-none-eabi" # proves rain_diagram_core is really no_std
            "wasm32-unknown-unknown"
          ];
        };

        msrv-toolchain = pkgs.rust-bin.stable.${msrv}.minimal.override {
          targets = ["wasm32-unknown-unknown"];
        };

        # Nightly rustfmt for unstable options (imports_granularity, etc.).
        # rustfmt links librustc_driver, so wrap it with the library path.
        nightly-rustfmt-unwrapped = pkgs.rust-bin.nightly.latest.minimal.override {
          extensions = ["rustfmt"];
        };

        nightly-rustfmt = pkgs.writeShellScriptBin "rustfmt" ''
          export DYLD_LIBRARY_PATH="${nightly-rustfmt-unwrapped}/lib''${DYLD_LIBRARY_PATH:+:$DYLD_LIBRARY_PATH}"
          export LD_LIBRARY_PATH="${nightly-rustfmt-unwrapped}/lib''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
          exec "${nightly-rustfmt-unwrapped}/bin/rustfmt" "$@"
        '';

        # Must match the exact `wasm-bindgen` pin in Cargo.toml
        wasm-bindgen-cli = unstable.wasm-bindgen-cli;

        format-pkgs = with pkgs; [
          alejandra
          taplo
        ];

        cargo-installs = with pkgs; [
          cargo-deny
          cargo-expand
          cargo-mutants
          cargo-nextest
          cargo-outdated
          cargo-sort
          cargo-udeps
          cargo-watch
          twiggy
          typos
          wasm-bindgen-cli
          wasm-tools
        ];

        rust = command-utils.rust.${system};
        wasm = command-utils.wasm.${system};
        cmd = command-utils.cmd.${system};

        # Hot-reloading preview: rebuilds the Wasm when Rust sources change and
        # reloads the page (see javascript/preview/serve.mjs). Run from the
        # repo root, or anywhere inside `nix develop` (via WORKSPACE_ROOT).
        preview = pkgs.writeShellApplication {
          name = "rain-preview";
          runtimeInputs = [
            rust-toolchain
            wasm-bindgen-cli
            pkgs.binaryen
            pkgs.nodejs
            pkgs.wasm-pack
          ];
          text = ''
            root="''${WORKSPACE_ROOT:-$PWD}"
            if [ ! -f "$root/javascript/preview/serve.mjs" ]; then
              echo "run this from the downpour repo root" >&2
              exit 1
            fi

            exec node "$root/javascript/preview/serve.mjs"
          '';
        };

        # The preview as a static site (for GitHub Pages): a release Wasm build,
        # the editor page, and the component, with `/` sending you to the
        # editor. Writes `_site/` (or the directory given).
        site = pkgs.writeShellApplication {
          name = "rain-site";
          runtimeInputs = [
            rust-toolchain
            wasm-bindgen-cli
            pkgs.binaryen
            pkgs.coreutils
            pkgs.stdenv.cc
            pkgs.wasm-pack
          ];
          text = ''
            root="''${WORKSPACE_ROOT:-$PWD}"
            out="$(realpath -m "''${1:-_site}")"
            case "$out" in
              / | "$root" | "$HOME") echo "refusing to replace $out" >&2; exit 1 ;;
            esac
            set -x
            rm -rf "$out"
            mkdir -p "$out/preview" "$out/rain-diagram"
            wasm-pack build --release --target web --no-pack \
              --out-dir "$out/rain-diagram/pkg" "$root/rust/rain_diagram_wasm"
            rm -f "$out/rain-diagram/pkg/.gitignore"
            cp "$root/javascript/rain-diagram/rain-diagram.js" "$out/rain-diagram/"
            for file in index.html embed.html main.js highlight.js history.js split.js; do
              cp "$root/javascript/preview/$file" "$out/preview/"
            done
            cat > "$out/index.html" <<'HTML'
            <!doctype html>
            <meta charset="utf-8">
            <title>Downpour</title>
            <meta http-equiv="refresh" content="0; url=preview/">
            <a href="preview/">Open the Downpour editor</a>
            HTML
            touch "$out/.nojekyll"
          '';
        };

        # wasm-bodge (github.com/alexjg/wasm-bodge) packages the Wasm bindings
        # as one npm package that loads in Node, bundlers, browsers, workers,
        # and script tags. Not in nixpkgs, so pinned here.
        wasm-bodge = pkgs.rustPlatform.buildRustPackage rec {
          pname = "wasm-bodge";
          version = "0.5.0";
          src = pkgs.fetchCrate {
            inherit pname version;
            hash = "sha256-rynFZi1ZMVzi/pBVpSjRMPIVkeJxCM7tfZblGjFLVDE=";
          };
          cargoHash = "sha256-lNgGxyLcO7yEATlMfjjVXBgmjtXI4l+lFk/yagP7lh0=";
          doCheck = false;
        };

        # `nix run .#bodge [template-package.json] [out-dir]`: build the
        # bindings package with wasm-bodge, on our stable toolchain
        # (`--panic abort`; unwind needs nightly), into javascript/rain-diagram-wasm
        bodge = pkgs.writeShellApplication {
          name = "rain-bodge";
          runtimeInputs = [
            rust-toolchain
            wasm-bindgen-cli
            wasm-bodge
            pkgs.binaryen
            pkgs.esbuild
            pkgs.nodejs
          ];
          text = ''
            set -x
            wasm-bodge build \
              --crate-path rust/rain_diagram_wasm \
              --package-json "''${1:-javascript/rain-diagram-wasm/package.json}" \
              --out-dir "''${2:-javascript/rain-diagram-wasm/dist}" \
              --panic abort
          '';
        };

        command_menu = command-utils.commands.${system} [
          (rust.build {cargo = pkgs.cargo;})
          (rust.test {
            cargo = pkgs.cargo;
            cargo-watch = pkgs.cargo-watch;
          })
          (rust.lint {cargo = pkgs.cargo;})
          (rust.fmt {cargo = pkgs.cargo;})
          (rust.doc {cargo = pkgs.cargo;})
          (rust.watch {cargo-watch = pkgs.cargo-watch;})

          (wasm.build {wasm-pack = pkgs.wasm-pack;})
          (wasm.release {
            wasm-pack = pkgs.wasm-pack;
            gzip = pkgs.gzip;
          })

          {
            commands.preview = cmd "Hot-reloading preview at http://127.0.0.1:8080 (PORT to change)" ''
              ${pkgs.lib.getExe preview}
            '';
            packages = [];
          }
        ];

        # CI checks: each is a standalone `nix run .#ci-<name>` app so hosted
        # CI and local runs execute byte-identical commands.
        ci-env = [
          nightly-rustfmt
          pkgs.cargo-deny
          pkgs.nodejs
          pkgs.stdenv.cc # the linker, so checks also pass outside the dev shell
          pkgs.typos
          rust-toolchain
        ];

        mkCheck = name: text: mkCheckWith [] name text;
        mkCheckWith = extra: name: text:
          pkgs.writeShellApplication {
            name = "rain-${name}";
            runtimeInputs = ci-env ++ extra;
            text = ''
              export RUSTFMT="${nightly-rustfmt}/bin/rustfmt"
              set -x
              ${text}
            '';
          };

        ci-checks = {
          ci-fmt = mkCheck "ci-fmt" ''
            cargo fmt --all --check
          '';

          ci-clippy = mkCheck "ci-clippy" ''
            cargo check --workspace --all-targets
            cargo clippy --workspace --all-targets --all-features -- -D warnings
            # What a plain dependency builds: default features, library only
            cargo clippy -p rain_diagram_core -- -D warnings
            cargo clippy -p rain_diagram_core --no-default-features -- -D warnings
            cargo clippy -p rain_diagram_wasm --target wasm32-unknown-unknown --all-targets -- -D warnings
          '';

          # rain_diagram_core's self dev-dependency enables `bolero` and
          # `serde`, so property tests always compile into this suite.
          ci-test = mkCheck "ci-test" ''
            cargo test --workspace --exclude rain_diagram_wasm
          '';

          ci-doc = mkCheck "ci-doc" ''
            cargo test --doc --workspace --all-features
            RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
          '';

          # Build the npm package with wasm-bodge, then load it by name the way
          # consumers do (import, require, ./slim, ./iife)
          # The package in a real Vite app, from a committed lockfile: build it
          # with no Wasm configuration, then load the production build in
          # headless Chromium and check the Wasm ran
          ci-vite = mkCheckWith [pkgs.chromium] "ci-vite" ''
            ${pkgs.lib.getExe bodge}
            cd javascript/rain-diagram-wasm/examples/vite
            npm ci --no-audit --no-fund
            node node_modules/vite/bin/vite.js build
            CHROMIUM=chromium node check.mjs
          '';

          ci-package = mkCheck "ci-package" ''
            ${pkgs.lib.getExe bodge}
            node --test javascript/rain-diagram-wasm/package-test/*.test.*
          '';

          # The docs as docs.rs builds them: nightly, every feature, and
          # `--cfg docsrs`, so feature-gated items carry their badge. Its own
          # target dir keeps nightly artifacts away from the stable builds
          ci-docsrs = pkgs.writeShellApplication {
            name = "rain-ci-docsrs";
            runtimeInputs = [nightly-rustfmt-unwrapped pkgs.stdenv.cc pkgs.gnugrep];
            text = ''
              set -x
              export CARGO_TARGET_DIR=target/docsrs
              RUSTDOCFLAGS="--cfg docsrs -D warnings" cargo doc -p rain_diagram_core --all-features --no-deps
              grep -q "crate feature <code>serde</code>" "$CARGO_TARGET_DIR/doc/rain_diagram_core/site_json/index.html"
            '';
          };

          # A target without `std` is the only real proof of no_std
          ci-no-std = mkCheck "ci-no-std" ''
            cargo check -p rain_diagram_core --no-default-features --features serde --target thumbv6m-none-eabi
          '';

          ci-bolero = mkCheck "ci-bolero" ''
            export BOLERO_RANDOM_ITERATIONS="''${BOLERO_RANDOM_ITERATIONS:-10000}"
            cargo test --profile bolero -p rain_diagram_core --features bolero,serde --tests
          '';

          ci-deny = mkCheck "ci-deny" ''
            cargo deny check
          '';

          # Syntax-check every script (there's no JS build step), build the
          # Wasm the bindings tests load, then run the JS tests
          ci-js = mkCheckWith [wasm-bindgen-cli pkgs.binaryen pkgs.wasm-pack] "ci-js" ''
            for file in javascript/*/*.js javascript/*/*.mjs javascript/*/*test/*.mjs; do
              node --check "$file"
            done
            wasm-pack build --dev --target web --out-dir ../../javascript/rain-diagram/pkg rust/rain_diagram_wasm
            node --test javascript/*/*.test.mjs javascript/*/test/*.test.mjs
          '';

          # The preview and its component in headless Chromium, driven over the
          # DevTools protocol (no npm dependencies)
          ci-browser = mkCheckWith [wasm-bindgen-cli pkgs.binaryen pkgs.chromium pkgs.wasm-pack] "ci-browser" ''
            wasm-pack build --dev --target web --out-dir ../../javascript/rain-diagram/pkg rust/rain_diagram_wasm
            CHROMIUM=chromium node --test javascript/preview/browser-test/*.test.mjs
          '';

          ci-typos = mkCheck "ci-typos" ''
            typos
          '';

          # Re-resolve as a library consumer would, preferring MSRV-compatible
          # versions, then restore the committed lockfile.
          ci-msrv = pkgs.writeShellApplication {
            name = "rain-ci-msrv";
            runtimeInputs = [msrv-toolchain pkgs.coreutils pkgs.stdenv.cc];
            text = ''
              export CARGO_RESOLVER_INCOMPATIBLE_RUST_VERSIONS=fallback
              committed_lock="$(mktemp)"
              cp Cargo.lock "$committed_lock"
              trap 'mv "$committed_lock" Cargo.lock' EXIT
              set -x
              cargo generate-lockfile
              cargo check --workspace --all-targets --all-features
            '';
          };
        };
      in {
        apps =
          builtins.mapAttrs (_: drv: {
            type = "app";
            program = pkgs.lib.getExe drv;
          })
          (ci-checks // {inherit bodge preview site;});

        packages = {inherit wasm-bodge;};

        devShells.default = pkgs.mkShell {
          name = "rain_shell";

          nativeBuildInputs =
            command_menu
            ++ [
              rust-toolchain
              nightly-rustfmt

              pkgs.binaryen
              pkgs.nodejs
              pkgs.pnpm
              pkgs.rust-analyzer
              pkgs.wasm-pack
            ]
            ++ format-pkgs
            ++ cargo-installs;

          shellHook = ''
            unset SOURCE_DATE_EPOCH
            export WORKSPACE_ROOT="$(pwd)"
            export RUSTFMT="${nightly-rustfmt}/bin/rustfmt"
            menu
          '';
        };

        formatter = pkgs.alejandra;
      }
    );
}
