# RPM spec of the LayoutCraft package (docs/36-packaging.md). It packs a ready-made file tree (tools/packaging/linux/package.sh stages it), so there is
# no %prep or %build. package.sh passes: lc_version (RPM version, "~" for a pre-release), lc_root (the staged tree), lc_summary.
# The binaries are built and stripped by cargo already, hence no debuginfo package and no brp post-processing (which would also need the foreign
# architecture's tools when the aarch64 package is made on an x86_64 machine).
%global debug_package %{nil}
%global __os_install_post %{nil}
%global _build_id_links none

Name:           layoutcraft
Version:        %{lc_version}
Release:        1
Summary:        %{lc_summary}
License:        MIT OR Apache-2.0
URL:            https://github.com/taron-sys/LayoutCraft
AutoReqProv:    no
# The app loads these at run time (so they are not found automatically): the keyboard library and OpenGL / EGL for the window.
Requires:       libxkbcommon.so.0()(64bit)
Requires:       (libEGL.so.1()(64bit) or libGL.so.1()(64bit))

%description
LayoutCraft is a diagram editor that opens and saves .vsdx files. This package has the desktop app (layoutcraft-app) and
the MCP server (layoutcraft-mcp) that an AI assistant can start to read and edit drawings.

%install
mkdir -p %{buildroot}
cp -a %{lc_root}/. %{buildroot}/

%files
%defattr(-,root,root,-)
/usr/bin/layoutcraft-app
/usr/bin/layoutcraft-mcp
/usr/share/applications/layoutcraft.desktop
/usr/share/icons/hicolor/256x256/apps/layoutcraft.png
/usr/share/icons/hicolor/scalable/apps/layoutcraft.svg
/usr/share/mime/packages/layoutcraft.xml
/usr/share/doc/layoutcraft
