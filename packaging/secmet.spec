# Spec RPM SecMet. Les binaires sont construits par packaging/build_rpm.sh
# (cargo, en dehors de rpmbuild) et livrés dans Source0 sous forme
# d'arborescence usr/ prête à installer.

%global debug_package %{nil}
%global _build_id_links none
# TestDisk embarqué et ses bibliothèques privées : ni Provides ni Requires
# automatiques (sinon le paquet « fournirait » libntfs-3g, etc.).
%global __requires_exclude_from ^/usr/lib/secmet/.*$
%global __provides_exclude_from ^/usr/lib/secmet/.*$
%global __brp_check_rpaths %{nil}

Name:           secmet
Version:        %{secmet_version}
Release:        1%{?dist}
Summary:        Effacement sécurisé et certifié de disques (GTK4, NIST SP 800-88)
License:        GPL-3.0-or-later
URL:            https://github.com/hclcdev12/secmet
Source0:        secmet-%{version}.tar.gz

Requires:       gtk4
Requires:       polkit
Recommends:     testdisk
Recommends:     evince
Conflicts:      shred-graphique

%description
Interface graphique d'effacement sécurisé de disques selon NIST SP 800-88 :
commandes firmware ATA/NVMe (sanitize, Security Erase, Format NVM) et
écriture hôte vérifiée par relecture, exécutées nativement par un helper
privilégié (PolicyKit). Certificats PDF avec attestation JSON signée Ed25519,
vérifiables hors ligne avec secmet-verify. TestDisk embarqué sous
/usr/lib/secmet pour la validation optionnelle.

%prep
%setup -q -c -n secmet-%{version}

%build
# Rien : binaires déjà construits (cargo) par build_rpm.sh.

%install
mkdir -p %{buildroot}
cp -a usr %{buildroot}/

%post
rm -f /usr/bin/shred-graphique /usr/bin/shred-graphique-helper
rm -f /usr/share/applications/shred-graphique.desktop
rm -f /usr/share/polkit-1/actions/shred-graphique.policy
rm -f /usr/share/icons/hicolor/scalable/apps/shred-graphique.svg

%files
%{_bindir}/secmet
%{_bindir}/secmet-helper
%{_bindir}/secmet-verify
%{_bindir}/secmet-release
%{_datadir}/applications/secmet.desktop
%{_datadir}/icons/hicolor/scalable/apps/secmet.svg
%{_datadir}/polkit-1/actions/secmet.policy
/usr/lib/secmet/
%dir %{_datadir}/doc/secmet
%{_datadir}/doc/secmet/*

%changelog
* Fri Oct 30 2026 SecMet Contributors <noreply@example.com> - 0.3.1-1
- Certificats PDF dans la langue de l'interface (fr, en, de).
- Nom de l'opérateur pré-enregistré dans les Réglages.
- Le disque système reste non cochable après une session d'effacement.

* Fri Oct 09 2026 SecMet Contributors <noreply@example.com> - 0.3.0-1
- Accès disque natifs, moteur NIST, attestation signée, AppImage.
