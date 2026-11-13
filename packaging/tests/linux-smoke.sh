#!/bin/bash
# Destructive package tests ONLY in a disposable container. Never starts a root service or GUI.
# Usage: TORRO_INSTALLER_SMOKE=1 linux-smoke.sh <deb|rpm|archlinux> <older-dir> <newer-dir>
set -euo pipefail
[ "${TORRO_INSTALLER_SMOKE:-}" = 1 ] && [ "$(id -u)" = 0 ]
[ -f /.dockerenv ] || [ -f /run/.containerenv ] || { echo "Disposable container required" >&2; exit 1; }
[ "$#" = 3 ] || exit 1
format="$1" older="$2" newer="$3"
arch="$(uname -m)"
[ ! -e /usr/bin/torroworkplace ] || { echo "Existing installation; refusing" >&2; exit 1; }
files() {
  case "$format:$arch" in
    deb:x86_64) find "$1" -maxdepth 1 -name '*_amd64.deb' ;;
    deb:aarch64) find "$1" -maxdepth 1 -name '*_arm64.deb' ;;
    rpm:*) find "$1" -maxdepth 1 -name "*.$arch.rpm" ;;
    archlinux:*) find "$1" -maxdepth 1 -name "*-$arch.pkg.tar.zst" ;;
    *) exit 1 ;;
  esac
}
mapfile -t old_files < <(files "$older")
mapfile -t new_files < <(files "$newer")
[ "${#old_files[@]}" = 9 ] && [ "${#new_files[@]}" = 9 ]
case "$format" in
  deb)
    dpkg --compare-versions 0.1.0~rc.10-1 lt 0.1.0-1
    for file in "${new_files[@]}"; do dpkg-deb --info "$file"; done
    dpkg-deb -f "$(find "$newer" -name 'torro-inbox-mail-bin_*deb' | grep "_$(dpkg --print-architecture).deb$")" Depends | grep 'torroinbox-bin (>= '
    install_packages() { apt-get install -y "$@"; }
    remove_packages() { apt-get remove -y "$@"; }
    ;;
  rpm)
    [ "$(rpm --eval '%{lua:print(rpm.vercmp("0.1.0~rc.10", "0.1.0"))}')" = -1 ]
    for file in "${new_files[@]}"; do rpm -qpi "$file"; done
    rpm -qp --requires "$(find "$newer" -name "torro-inbox-mail-bin-*.$arch.rpm")" | grep 'torroinbox-bin >= '
    install_packages() { dnf install -y "$@"; }
    remove_packages() { dnf remove -y --setopt=clean_requirements_on_remove=False "$@"; }
    ;;
  archlinux)
    [ "$(vercmp 0.1.0rc.10-1 0.1.0-1)" = -1 ]
    for file in "${new_files[@]}"; do pacman -Qip "$file"; done
    pacman -Qip "$(find "$newer" -name "torro-inbox-mail-bin-*-$arch.pkg.tar.zst")" | grep 'torroinbox-bin>='
    install_packages() { pacman --noconfirm -U "$@"; }
    remove_packages() { pacman --noconfirm -R "$@"; }
    ;;
  *) exit 1 ;;
esac
install_packages "${old_files[@]}"
install_packages "${new_files[@]}"
# A second install must not create a second owner, launcher, service or marker.
install_packages "${new_files[@]}"
mkdir -p /home/smoke/.local/share/torrotasks
printf 'personal data\n' > /home/smoke/.local/share/torrotasks/preserve.txt
test -f /usr/lib/systemd/user/torroworkplace.service
test -f /usr/share/applications/torro-workplace.desktop
for program in torroworkplace torroinbox torro-inbox-mail torro-inbox-notifications torrocalendar torrotasks torroassistant torrochat; do
  test -x "/usr/bin/$program"
  test -f "/usr/bin/$program.torro-package.json"
  "/usr/bin/$program" --version
done
remove_packages torro-inbox-mail-bin torro-inbox-notifications-bin
test -x /usr/bin/torroinbox
test -x /usr/bin/torrocalendar
remove_packages torroinbox-bin torrocalendar-bin torrotasks-bin torroassistant-bin torrochat-bin torro-workplace
test -x /usr/bin/torroworkplace
test ! -e /usr/share/applications/torro-workplace.desktop
remove_packages torroworkplace-bin
test ! -e /usr/bin/torroworkplace
test ! -e /usr/bin/torroworkplace.torro-package.json
test -f /home/smoke/.local/share/torrotasks/preserve.txt
echo "PASS: $format $arch metadata/install/repeat/upgrade/remove; data preserved; no service started"
