# Meta formula for the tap mahype/homebrew-torro: `brew install mahype/torro/torro-suite`.
# Copy to Formula/torro-suite.rb in the tap repository ONCE. The three app formulae
# (torroworkplace, torrocalendar, torrotasks) are generated and updated by `dist` on every release.
# Needs no version bump: it only depends on the other formulae.
class TorroSuite < Formula
  desc "TorroWorkplace suite: core with tray icon, Torro Calendar and Torro Tasks"
  homepage "https://github.com/mahype/TorroWorkplace"
  # A formula needs a url; the meta formula points at the (tiny) LICENSE file of a fixed tag.
  # TODO(maintainer): set url and sha256 after the first release exists.
  url "https://github.com/mahype/TorroWorkplace/archive/refs/tags/v0.1.0.tar.gz"
  sha256 "0000000000000000000000000000000000000000000000000000000000000000"
  license any_of: ["MIT", "Apache-2.0"]

  depends_on "mahype/torro/torroworkplace"
  depends_on "mahype/torro/torrocalendar"
  depends_on "mahype/torro/torrotasks"

  def install
    # Nothing to install; the dependencies carry the binaries.
    (prefix/"README").write "Meta formula of the TorroWorkplace suite.\n"
  end

  test do
    assert_predicate prefix/"README", :exist?
  end
end
