Name:           lanlink
Version:        0.1.0
Release:        1%{?dist}
Summary:        Cross-platform Radmin-VPN-style virtual LAN (daemon + tools)
License:        Apache-2.0
URL:            https://github.com/lanlink/lanlink
Source0:        %{url}/archive/v%{version}/%{name}-%{version}.tar.gz
Requires:       nftables iproute2
Recommends:     coturn
BuildRequires:  cargo
BuildRequires:  systemd-rpm-macros

ExclusiveArch:  %{rust_arches}

%description
LanLink is a free layer-2 mesh VPN aimed at gaming, remote access and
connecting distributed teams. It emulates a shared Ethernet segment (ARP,
broadcast, multicast) over encrypted peer-to-peer UDP with NAT traversal and
a content-blind relay fallback.

This package contains the lanlinkd daemon, lanlinkctl CLI, the
lanlink-relayd relay server and the lanlink-coordinator membership server.

%prep
%autosetup -n lanlink

%build
# Build only the daemon/core binaries. The GUI front-ends (Tauri/Web/TUI)
# are packaged separately via dist/deb/build-deb.sh and dist/aur/PKGBUILD.
cargo fetch --locked --target %{_arch}-unknown-linux-gnu
cargo build --release --frozen -p lanlinkd -p lanlinkctl -p lanlink-relay -p coordinator

%install
install -D -m 0755 target/release/lanlinkd %{buildroot}%{_bindir}/lanlinkd
install -D -m 0755 target/release/lanlinkctl %{buildroot}%{_bindir}/lanlinkctl
install -D -m 0755 target/release/lanlink-relayd %{buildroot}%{_bindir}/lanlink-relayd
install -D -m 0755 target/release/lanlink-coordinator %{buildroot}%{_bindir}/lanlink-coordinator
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
%{_unitdir}/lanlinkd@.service
%{_unitdir}/lanlink-coordinator.service
%{_unitdir}/lanlink-relayd.service
%config(noreplace) %{_sysconfdir}/lanlink/lanlinkd.example.toml
%attr(0750,lanlink,lanlink) %dir %{_sharedstatedir}/lanlink

%changelog
* Sun Oct 11 2026 LanLink Contributors <lanlink@example.com> - 0.1.0-1
- Initial package
