#!/usr/bin/env bash
# Runs inside the Linux release container (see Containerfile).
#   /src   - the host repository, mounted read-only
#   /out   - receives deb/, rpm/ and appimage/
#   /cache - persistent volume for the Cargo registry and build target
#
# Builds rawtap once, stages one install tree (binary, launcher, icons), and
# packs that same tree as a .deb, an .rpm and an AppImage.
set -euo pipefail

cargo_registry_cache=/cache/cargo-registry
export CARGO_TARGET_DIR=/cache/target
mkdir -p "$cargo_registry_cache" "$CARGO_TARGET_DIR"
ln -sfn "$cargo_registry_cache" "${CARGO_HOME}/registry"

# Build from a fresh copy of the working tree: tracked plus untracked,
# non-ignored files only, so the host's target/ never leaks in.
git config --global --add safe.directory /src
rm -rf /build
mkdir -p /build
git -C /src ls-files -z --cached --others --exclude-standard \
    | rsync -a --from0 --files-from=- --ignore-missing-args /src/ /build/
cd /build

version="$(sed -n 's/^version = "\([^"]*\)"$/\1/p' Cargo.toml | head -n1)"
[[ -n "$version" ]] || { echo "Could not read the version from Cargo.toml." >&2; exit 1; }
app_id="info.brumley.rawtap"
summary="See and send raw serial and TCP data, in hex and text"

cargo build --release --locked
binary="${CARGO_TARGET_DIR}/release/rawtap"

# One install tree for every package format.
stage=/work/stage
rm -rf /work
mkdir -p "${stage}/usr/bin" "${stage}/usr/share/applications" "${stage}/usr/share/doc/rawtap"
install -m 0755 "$binary" "${stage}/usr/bin/rawtap"
install -m 0644 "packaging/${app_id}.desktop" "${stage}/usr/share/applications/${app_id}.desktop"
desktop-file-validate "${stage}/usr/share/applications/${app_id}.desktop"
for size in 32 48 64 128 256; do
    install -D -m 0644 "packaging/icons/rawtap-${size}.png" \
        "${stage}/usr/share/icons/hicolor/${size}x${size}/apps/rawtap.png"
done
install -m 0644 LICENSE "${stage}/usr/share/doc/rawtap/copyright"

# .deb ----------------------------------------------------------------------
deb_root=/work/deb
cp -a "$stage" "$deb_root"
mkdir -p "${deb_root}/DEBIAN"
cat > "${deb_root}/DEBIAN/control" <<CONTROL
Package: rawtap
Version: ${version}
Architecture: amd64
Maintainer: Jack Brumley <82296582+jackbrumley@users.noreply.github.com>
Depends: libc6 (>= 2.35), libudev1
Section: comm
Priority: optional
Homepage: https://brumley.info/rawtap
Description: ${summary}
 RawTap watches a serial port, TCP connection or TCP listener in hex and
 text, logs everything, and works as a switch/console terminal.
CONTROL
mkdir -p /work/out/deb
dpkg-deb --root-owner-group --build "$deb_root" "/work/out/deb/rawtap_${version}_amd64.deb"

# .rpm ----------------------------------------------------------------------
rpm_top=/work/rpmbuild
mkdir -p "${rpm_top}/SPECS"
cat > "${rpm_top}/SPECS/rawtap.spec" <<SPEC
Name:           rawtap
Version:        ${version}
Release:        1
Summary:        ${summary}
License:        AGPL-3.0-only
URL:            https://brumley.info/rawtap
# The binary is built and stripped by cargo; package it as it is.
%global debug_package %{nil}
%global __os_install_post %{nil}

%description
RawTap watches a serial port, TCP connection or TCP listener in hex and
text, logs everything, and works as a switch/console terminal.

%install
cp -a ${stage}/. %{buildroot}/

%files
/usr/bin/rawtap
/usr/share/applications/${app_id}.desktop
/usr/share/icons/hicolor/*/apps/rawtap.png
%doc /usr/share/doc/rawtap/copyright
SPEC
rpmbuild --define "_topdir ${rpm_top}" --target x86_64 -bb "${rpm_top}/SPECS/rawtap.spec"
mkdir -p /work/out/rpm
cp "${rpm_top}"/RPMS/x86_64/rawtap-"${version}"-1.x86_64.rpm /work/out/rpm/

# AppImage ------------------------------------------------------------------
# A terminal app that only needs glibc and libudev, which every desktop has,
# so nothing is bundled beyond the binary itself.
appdir=/work/RawTap.AppDir
cp -a "$stage" "$appdir"
ln -s usr/bin/rawtap "${appdir}/AppRun"
cp "${stage}/usr/share/applications/${app_id}.desktop" "${appdir}/${app_id}.desktop"
cp packaging/icons/rawtap-256.png "${appdir}/rawtap.png"
mkdir -p /work/out/appimage
ARCH=x86_64 appimagetool --no-appstream "$appdir" "/work/out/appimage/rawtap-${version}-x86_64.AppImage"

for format in deb rpm appimage; do
    rm -rf "/out/${format}"
    cp -a "/work/out/${format}" "/out/${format}"
done
echo "Linux release packages written to /out (glibc $(ldd --version | head -n1 | awk '{print $NF}'))"
