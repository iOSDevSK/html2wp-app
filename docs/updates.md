# The plugin and the runtime

## The html2wp plugin

The app does not carry the plugin. It reads `VERSION` from the `main` branch of [iOSDevSK/html2wp-codex-plugin](https://github.com/iOSDevSK/html2wp-codex-plugin), clones the tag `v<VERSION>` (never `main`) with `git` from the runtime image into `<app data>/plugin/<version>/`, and records the version and commit (`active-plugin-version`, `active-plugin-commit`). HTTPS from github.com at a tag is the check; there is no signature. The previous plugin folder stays (`previous-plugin`); older ones are removed.

- **When:** Prepare environment (and a check that finds none installed) fetches it when none is installed; the first run needs internet. Once per launch, after the environment is ready and while no conversion runs, a different upstream version is fetched, switched to, and the owner told ("The html2wp plugin was updated from X to Y."). Offline or a failed fetch: the installed plugin stays and nothing is said.
- **Where:** `plugins/html2wp` of the clone is mounted read-only at `/opt/html2wp` into the Codex container and every project container (the plugin's repair never edits its own scripts). The h2g sandbox and the Cloudflare helper do not read it and do not mount it.
- **Containers:** each is labelled `dev.html2wp.plugin=<commit>`; one made with another commit, or none (an older app's), is made again, like one made from another image. Projects record the installed plugin commit and move to it at their next step.
- **Compatibility:** `appContract` in the plugin's `.codex-plugin/plugin.json` must equal the app's `SUPPORTED_CONTRACT`; otherwise the plugin is refused ("Update the app to use plugin X"). A plugin without the field is taken to speak the app's contract.

`vendor/html2wp` may stay as a development checkout; nothing from it goes into the DMG or the image.

## The runtime image

`scripts/bundle-runtime.py` names the image `html2wp-runtime:desktop-<app version>-<12 hex of the functional build-context fingerprint>`, builds or reuses a byte-for-byte verified local image, runs its self-check, pushes it to Docker Hub, and pins the public registry digest and accepted local image IDs in `runtime/runtime-release.json`. Prepare environment pulls that immutable digest if it is not already cached. The Codex in the Codex container updates itself once per launch (`runtime::update_codex`).
