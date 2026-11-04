#!/bin/sh
# Writes winget manifests for a released version: sh make.sh 0.2.0
set -e
V=${1:?version, e.g. 0.2.0}
URL="https://github.com/NilsBriggen/leaf-notes/releases/download/v$V/LeafNotes-x86_64.msi"
SHA=$(curl -sL "$URL" | sha256sum | cut -d' ' -f1 | tr a-f A-F)
D=manifests/$V; mkdir -p "$D"
cat > "$D/NilsBriggen.LeafNotes.yaml" <<M
PackageIdentifier: NilsBriggen.LeafNotes
PackageVersion: $V
DefaultLocale: en-US
ManifestType: version
ManifestVersion: 1.6.0
M
cat > "$D/NilsBriggen.LeafNotes.installer.yaml" <<M
PackageIdentifier: NilsBriggen.LeafNotes
PackageVersion: $V
InstallerType: wix
Installers:
  - Architecture: x64
    InstallerUrl: $URL
    InstallerSha256: $SHA
ManifestType: installer
ManifestVersion: 1.6.0
M
cat > "$D/NilsBriggen.LeafNotes.locale.en-US.yaml" <<M
PackageIdentifier: NilsBriggen.LeafNotes
PackageVersion: $V
PackageLocale: en-US
Publisher: Nils Briggen
PackageName: Leaf Notes
License: GPL-3.0-only
ShortDescription: Notes with pen, maths, recordings and flashcards
PackageUrl: https://leafnotes.briggen.dev
PrivacyUrl: https://leafnotes.briggen.dev/privacy
ManifestType: defaultLocale
ManifestVersion: 1.6.0
M
echo "Wrote $D"
