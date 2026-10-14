Name:           lanlink
# NOTE: the build script (dist/rpm/build-rpm.sh) substitutes this with the
# workspace version from Cargo.toml before invoking rpmbuild.
Version:        0.1.0
Release:        1%{?dist}
Summary:        Cross-platform Radmin-VPN-style virtual LAN (daemon + tools)
License:        Apache-2.0
URL:            https://github.com/lanlink/lanlink
Source0:        %{url}/archive/v%{version}/%{name}-%{version}.tar.gz
Requires:       nftables iproute2 webkit2gtk4.1
Recommends:     coturn
BuildRequires:  cargo
BuildRequires:  systemd-rpm-macros
BuildRequires:  webkit2gtk4.1-devel

ExclusiveArch:  %{rust_arches}

%description
LanLink is a free layer-2 mesh VPN aimed at gaming, remote access and
connecting distributed teams. It emulates a shared Ethernet segment (ARP,
broadcast, multicast) over encrypted peer-to-peer UDP with NAT traversal and
a content-blind relay fallback.

This package contains the lanlinkd daemon, lanlinkctl CLI, the
lanlink-relayd relay server, the lanlink-coordinator membership server and the
lanlink-desktop / lanlink-web / lanlink-tui / lanlink-gui front-ends.

%prep
%autosetup -n lanlink

%build
# Build the daemon/core binaries plus the GUI front-ends by default.
cargo fetch --locked --target %{_arch}-unknown-linux-gnu
cargo build --release --frozen -p lanlinkd -p lanlinkctl -p lanlink-relay -p coordinator -p lanlink-desktop -p lanlink-web -p lanlink-tui -p lanlink-gui

%install
install -D -m 0755 target/release/lanlinkd %{buildroot}%{_bindir}/lanlinkd
install -D -m 0755 target/release/lanlinkctl %{buildroot}%{_bindir}/lanlinkctl
install -D -m 0755 target/release/lanlink-relayd %{buildroot}%{_bindir}/lanlink-relayd
install -D -m 0755 target/release/lanlink-coordinator %{buildroot}%{_bindir}/lanlink-coordinator
install -D -m 0755 target/release/lanlink-desktop %{buildroot}%{_bindir}/lanlink-desktop
install -D -m 0755 target/release/lanlink-web %{buildroot}%{_bindir}/lanlink-web
install -D -m 0755 target/release/lanlink-tui %{buildroot}%{_bindir}/lanlink-tui
install -D -m 0755 target/release/lanlink-gui %{buildroot}%{_bindir}/lanlink-gui
install -D -m 0644 dist/systemd/lanlinkd@.service %{buildroot}%{_unitdir}/lanlinkd@.service
install -D -m 0644 dist/systemd/lanlink-coordinator.service %{buildroot}%{_unitdir}/lanlink-coordinator.service
install -D -m 0644 dist/systemd/lanlink-relayd.service %{buildroot}%{_unitdir}/lanlink-relayd.service
install -D -m 0644 dist/lanlinkd.example.toml %{buildroot}%{_sysconfdir}/lanlink/lanlinkd.example.toml
install -d -m 0750 %{buildroot}%{_sharedstatedir}/lanlink

%pre
getent group lanlink >/dev/null || groupadd -r lanlink
getent passwd lanlink >/dev/null || useradd -r -g lanlink -s /sbin/nologin -d / %{name}

%post
%systemd_post lanlink-coordinator.service lanlink-relayd.service
%systemd_user_post 2>/dev/null || true

%preun
%systemd_preun lanlink-coordinator.service lanlink-relayd.service
systemctl stop 'lanlinkd@*' >/dev/null 2>&1 || :

%postun
%systemd_postun lanlink-coordinator.service lanlink-relayd.service

%files
%license LICENSE
%doc README.md
%{_bindir}/lanlinkd
%{_bindir}/lanlinkctl
%{_bindir}/lanlink-relayd
%{_bindir}/lanlink-coordinator
%{_bindir}/lanlink-desktop
%{_bindir}/lanlink-web
%{_bindir}/lanlink-tui
%{_bindir}/lanlink-gui
%{_unitdir}/lanlinkd@.service
%{_unitdir}/lanlink-coordinator.service
%{_unitdir}/lanlink-relayd.service
%config(noreplace) %{_sysconfdir}/lanlink/lanlinkd.example.toml
%attr(0750,lanlink,lanlink) %dir %{_sharedstatedir}/lanlink

%changelog
* Sun Oct 11 2026 LanLink Contributors <lanlink@example.com> - 0.1.0-1
- Initial package
