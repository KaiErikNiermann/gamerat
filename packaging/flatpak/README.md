# Flatpak

Reserved for the eventual `org.appulsauce.GameRat.yml` manifest. Flatpak
packaging is a known *hard* fit for a daemon that needs talk-to access
on the session bus, hidraw read/write via ratbagd, and focus tracking —
the manifest will land once the daemon has enough surface area to be
worth packaging.
