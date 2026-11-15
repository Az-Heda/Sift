.PHONY: linux windows dev install clean

install:
	pnpm install

dev: install
	pnpm tauri dev

# Native Linux bundles -> src-tauri/target/release/bundle
# AppImage needs FUSE (unavailable on WSL): make linux LINUX_BUNDLES=appimage
LINUX_BUNDLES ?= deb,rpm
linux: install
	pnpm tauri build --bundles $(LINUX_BUNDLES)

# Windows standalone executable (no installer), cross-compiled from Linux/WSL.
# Requires: rustup target x86_64-pc-windows-msvc, cargo-xwin, clang, lld, llvm-rc
# -> src-tauri/target/x86_64-pc-windows-msvc/release/sift.exe
windows: install
	rustup target add x86_64-pc-windows-msvc
	pnpm tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc --no-bundle

clean:
	rm -rf dist src-tauri/target
