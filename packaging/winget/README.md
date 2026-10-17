# winget packaging

Reference manifests for `Volko61.Merge360` (portable app inside a zip). The installer
zips must be flat (`merge360.exe` at the root); the release workflow builds them that way
from v0.3.1 on, so these manifests only apply once a release with a flat zip exists.

One-time setup:

1. Cut a release (tag `vX.Y.Z`) so the flat zips exist.
2. Fill in the `InstallerSha256` values (from `SHA256SUMS.txt` on the release) and the
   version/URLs, validate with `winget validate .`, test with
   `winget install --manifest .`, then submit the folder as a PR to
   [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs) under
   `manifests/v/Volko61/Merge360/X.Y.Z/` (or run `wingetcreate new <zip URLs>`).
3. Add a classic GitHub PAT with `public_repo` scope as the repository secret `WINGET_TOKEN`.

After that, every non-prerelease tag makes the `winget` job in `release.yml` open the
update PR automatically, and users get it with `winget upgrade Volko61.Merge360`.
