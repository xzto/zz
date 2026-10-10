---
type: Playbook
title: Cross-compiling headless zz for Termux
description: Build the aarch64 Termux CLI on a Linux x86_64 host using Termux NDK sysroot packages.
resource: crates/zz-cli/Cargo.toml
tags:
- android
- termux
- linux
- cross-compilation
- headless
timestamp: 2026-10-10T19:39:18Z
---

# Overview

Build the headless `zz_cli` binary for 64-bit Termux (`arm64-v8a`) from Linux x86_64 with the Rust target `aarch64-linux-android`, Linux-host Zig and Clang, and the Termux `ndk-sysroot` and `ndk-multilib` packages. The verified build used Rust 1.97.0, Zig 0.16.0, Clang/LLD 22, and Termux NDK package version 30. It produced an Android API 24 AArch64 binary.

This procedure builds `zz-cli`, not the CEF desktop bundle. It does not change an installed `zz` on a device. Termux's x86_64 Docker image supplies target headers and libraries; its Android-built Zig and Clang are not the host compilers for this cross-build.

# Prepare the host

Install Docker, Rust 1.97.0 through rustup, Linux `clang`, `clang++`, LLD, GNU `ar`, `readelf`, and mise. From the repository root, install the pinned Zig and Rust target:

```sh
mise install zig@0.16.0
rustup target add aarch64-linux-android
zig version
rustc --version
```

The commands should report Zig 0.16.0 and Rust 1.97.0.

# Export the Termux sysroot

Start a disposable x86_64 Termux container. Run `pkg` as Termux's `system` user; the package frontend refuses root.

```sh
docker run -d --name zz-termux-sysroot termux/termux-docker sleep infinity
docker exec --user system zz-termux-sysroot \
  pkg install -y ndk-sysroot ndk-multilib
```

Create a synthetic NDK layout on the Linux host. Zig's Android build uses the standard NDK directory shape, while Termux installs the same headers and target libraries under `$PREFIX`.

```sh
export NDK="$HOME/.cache/zz-android-ndk"
export SYSROOT="$NDK/toolchains/llvm/prebuilt/linux-x86_64/sysroot"
export TERMUX_PREFIX=/data/data/com.termux/files/usr
export TARGET_LIB="$SYSROOT/usr/lib/aarch64-linux-android"

mkdir -p "$TARGET_LIB/24"
docker cp zz-termux-sysroot:"$TERMUX_PREFIX/include" "$SYSROOT/usr/include"
docker cp zz-termux-sysroot:"$TERMUX_PREFIX/aarch64-linux-android/lib/." \
  "$TARGET_LIB/"
docker cp zz-termux-sysroot:"$TERMUX_PREFIX/opt/ndk-multilib/aarch64-linux-android/lib/." \
  "$TARGET_LIB/"

for file in "$TARGET_LIB"/*; do
  case "$(basename "$file")" in [0-9][0-9]) continue ;; esac
  cp -a "$file" "$TARGET_LIB/24/"
done
ln -sfn libc++_shared.so "$TARGET_LIB/libc++.so"
ln -sfn ../libc++_shared.so "$TARGET_LIB/24/libc++.so"
for api in {21..36}; do
  [ "$api" = 24 ] || ln -sfn 24 "$TARGET_LIB/$api"
done
```

Zig may select an Android API directory while building its native helper. The Termux packages provide one unversioned set of libraries, so the other API directories point to that set. The Rust and Clang linkers below explicitly target API 24.

The host Clang also needs the Android compiler builtins archive from `ndk-multilib`:

```sh
export CLANG_RESOURCE="$HOME/.cache/zz-android-clang-resource"
mkdir -p "$CLANG_RESOURCE/lib/aarch64-unknown-linux-android24"
ln -sfn "$(clang -print-resource-dir)/include" "$CLANG_RESOURCE/include"
docker cp zz-termux-sysroot:"$TERMUX_PREFIX/opt/ndk-multilib/cross-compiler-rt/libclang_rt.builtins-aarch64-android.a" \
  "$CLANG_RESOURCE/lib/aarch64-unknown-linux-android24/libclang_rt.builtins.a"
```

# Configure cross-compilers

`cc-rs` passes an unversioned `--target=aarch64-linux-android` after the compiler command. The final versioned target option in each wrapper overrides it; otherwise Termux's headers reject the triple.

```sh
export TOOLS="$HOME/.cache/zz-android-toolchain"
mkdir -p "$TOOLS"

cat >"$TOOLS/clang-aarch64" <<'EOF'
#!/bin/sh
exec clang --sysroot="$SYSROOT" -resource-dir="$CLANG_RESOURCE" \
  -fuse-ld=lld \
  -L"$SYSROOT/usr/lib/aarch64-linux-android/24" \
  -L"$SYSROOT/usr/lib/aarch64-linux-android" \
  "$@" --target=aarch64-linux-android24
EOF

cat >"$TOOLS/clangxx-aarch64" <<'EOF'
#!/bin/sh
exec clang++ --sysroot="$SYSROOT" -resource-dir="$CLANG_RESOURCE" \
  -fuse-ld=lld \
  -L"$SYSROOT/usr/lib/aarch64-linux-android/24" \
  -L"$SYSROOT/usr/lib/aarch64-linux-android" \
  "$@" --target=aarch64-linux-android24
EOF
chmod +x "$TOOLS/clang-aarch64" "$TOOLS/clangxx-aarch64"
```

Export the cross settings in the same shell that runs Cargo:

```sh
export PATH="$(mise where zig@0.16.0):$PATH"
export ANDROID_NDK_HOME="$NDK"
export CC_aarch64_linux_android="$TOOLS/clang-aarch64"
export CXX_aarch64_linux_android="$TOOLS/clangxx-aarch64"
export AR_aarch64_linux_android="$(command -v ar)"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$TOOLS/clang-aarch64"
```

# Build and verify

From the repository root, build only the headless package:

```sh
cargo build --locked --release --target aarch64-linux-android -p zz-cli
```

The binary is `target/aarch64-linux-android/release/zz_cli`. Check its ABI and interpreter:

```sh
file target/aarch64-linux-android/release/zz_cli
readelf -h target/aarch64-linux-android/release/zz_cli | grep -E 'Class:|Machine:'
readelf -l target/aarch64-linux-android/release/zz_cli | grep interpreter
readelf -d target/aarch64-linux-android/release/zz_cli | grep NEEDED
```

Expect AArch64, `/system/bin/linker64`, and Android system dependencies such as `libc.so`, `libm.so`, and `libdl.so`. To smoke-test without replacing an installed command, copy the binary to a temporary path on the device, make it executable, and run `--version` there.

# Troubleshooting

- `pkg` reports that it cannot run as root: use `docker exec --user system`.
- Zig reports `AndroidNDKNotFound`: check that `ANDROID_NDK_HOME` names the synthetic NDK root and that `toolchains/llvm/prebuilt/linux-x86_64/sysroot/usr/include` exists.
- Clang reports `Unversioned target triples are not supported`: retain the final `--target=aarch64-linux-android24` in both wrappers.
- Clang warns that `-fuse-ld` and `-L` are unused during compile-only calls: `cc-rs` uses the same wrapper for compilation and linking; those flags apply during linking.
- Clang cannot find `libclang_rt.builtins.a`: check the custom resource path and the copied `libclang_rt.builtins-aarch64-android.a` archive.
- Running Zig from inside the x86_64 Termux container failed in its Android build runner (`getprop` and Android libc/header mismatches). Run Linux-host Zig for the cross-build.
- `just build linux` and `cargo xtask bundle-cef` build the desktop CEF app. Use the `zz-cli` Cargo package above for a headless Termux binary.

The exported sysroot is a build shim assembled from Termux packages, not a complete Google NDK. The recipe has been verified for the arm64 Termux target at API 24; check the output on the target device before replacing an installed binary.

# Key files

| File | Role |
| --- | --- |
| `crates/zz-cli/Cargo.toml` | Headless binary package |
| `rust-toolchain.toml` | Rust 1.97.0 pin |
| `mise.toml` | Zig 0.16.0 pin |
| `third_party/ghostty/pkg/android-ndk/build.zig` | Android sysroot discovery for the vendored Ghostty build |
