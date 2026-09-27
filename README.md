# html2wp Desktop

Tauri 2 desktop application with a React interface, a Rust host and Codex App Server. Since 1.0.0 the app is only the interface: the html2wp plugin (its skill) owns the whole conversion, and the app starts it, shows its progress and saves what it delivered. The UI is in English.

This is the desktop app's [source repository](https://github.com/iOSDevSK/html2wp-app). `main` carries shared code; `macos`, `windows` and `linux` are platform development branches. A branch does not imply that its native build has been validated.

**Current desktop release: 1.0.15.** The contract with the plugin is the plugin's `docs/APP-CONTRACT.md`.

## Licence

The original desktop code is published under the [html2wp Desktop Source-Available Commercial Licence](LICENSE). Non-commercial use is permitted without a key; commercial use requires a current html2wp licence key. Redistribution or use to build another conversion product requires separate written permission. This is a source-available licence, not an open-source licence.

The separately licensed plugin, `vendor/` submodules (including the GPL-2.0-or-later Gutenberg skill), dependencies, and third-party notices retain their own terms. The desktop licence does not change them. See [licences and distribution](docs/desktop-licensing.md).

## Use the app

1. Copy `html2wp.app` from the macOS DMG into Applications. This local build is ad-hoc signed, not Apple-notarized. If macOS blocks it, use **System Settings → Privacy & Security → Open Anyway** for this app after trying to open it.
2. In **Settings**, click **Prepare environment**. The app downloads and starts Docker as needed, loads the prepared runtime (it contains the plugin) and pulls the WordPress/database images. Confirm any system and Docker licence prompts. See [automatic setup](docs/automatic-setup.md).
3. **Connect with ChatGPT**. Complete the OpenAI device-code sign-in in your browser. Then choose **Conversion model** and **Reasoning effort**, or keep their defaults. Codex usage and the html2wp conversion allowance are separate.
4. Use **Free** without a key, or enter your html2wp licence and choose **Save & check**. The app copies the saved licence into each project's container for the plugin.
5. Acknowledge the processing notice, import a project folder or ZIP, and choose its output type. The app works on a copy.
6. Choose **Flash** (the default: the AI runs every stage once) or **Full** for an HTML theme, or **Build Astro project** for an Astro 5 project. For a Gutenberg block theme, convert to an HTML theme first, then import its ZIP as **Gutenberg from an HTML theme**. The Overview shows the plugin's stages as it reports them, and the repairs it made on its own; the chat shows the assistant. **Stop conversion** stops the run and its container; **Continue** resumes it. When the plugin stopped the run, **Repair and continue** (or any chat message) lets the AI spend the plugin's repair attempts on the stage that stopped, then continue; it never starts a new run. Converting again from the original is **Start over from the original…** in the project's … menu.
7. When the plugin says it is done, its files are in **Exports**: save the theme ZIP and anything else it delivered. Earlier runs stay there as previous versions. The **Preview** tab opens the WordPress the plugin started, in your default browser. WordPress admin opens there too. The app does not download Chrome for Testing or install Shot2AI when opening a preview. Shot2AI can still be paired from a compatible browser where you installed it, using Settings → Chrome extension.

Chat messages are single turns: ask about the conversion, or for a change to the theme. They never start a new run. Once the theme is delivered, a message is a change made in the live preview in seconds, with no rebuild; the Overview counts the changes since the last ZIP, and **Make release** (in the chat and the Overview) packages the theme as the preview has it now (no AI) and opens Exports on that ZIP, ready to save. If something is broken, **Start over from the original…** in the project's … menu converts the site again from its source; it warns first, because every change made after delivery is lost.

## Manage projects

Open the **…** menu beside the conversion buttons. **Clean & restart** removes this project's containers, WordPress volumes, generated files and chat after confirmation, and keeps the original import. **Remove from workspace** hides the project while retaining its local data; use **Show removed projects → Restore** on the home screen to bring it back. **Delete project** permanently removes its local data and owned Docker resources after confirmation. Your original folder/ZIP and files exported outside the app are preserved. Shared runtime images and the Codex account remain available.

## Develop

Tested toolchain: Node 22, npm 10, Rust 1.90, macOS ARM64. Platform build prerequisites are described in the [Tauri documentation](https://v2.tauri.app/start/prerequisites/).

```sh
npm ci
npm run prepare:runtime
npm run desktop
```

```sh
npm test
npm run test:runtime
cargo test --manifest-path src-tauri/Cargo.toml --locked
# Optional: installed Codex 0.154.0, empty auth home, no model calls
H2WP_TEST_CODEX_CLI=codex cargo test --manifest-path src-tauri/Cargo.toml --locked local_codex_protocol_smoke -- --ignored
python3 tests/code_mode_smoke.py
```

In a restricted workspace, use `CARGO_HOME="$PWD/.cache/cargo"` and `npm ci --cache .cache/npm` so dependency caches remain writable.

## Build

```sh
npm run bundle:mac
# On Windows with its native prerequisites
npm run bundle:windows
# On Linux with its native prerequisites
npm run tauri -- build --target x86_64-unknown-linux-gnu --bundles appimage,deb
```

Build output is under `src-tauri/target/<target>/release/bundle/`. The **Desktop builds** workflow currently builds macOS ARM64. Windows and Linux have configuration, not completed validation in this workspace. macOS distributable signing/notarization requires the owner's Apple credentials; this local build is not notarized.

## Version and GitHub release

The desktop app uses a product version such as `1.0.15`; the conversion runtime is published separately on Docker Hub and pinned by digest. Increase the product version for every distributable build. `version:check` is also a CI gate.

```sh
npm run version:set -- 1.0.15
python3 scripts/bundle-runtime.py --namespace <docker-hub-namespace> --reuse-image <verified-local-image>
npm run version:check
TAURI_SIGNING_PRIVATE_KEY=~/.config/html2wp/tauri-updater.key TAURI_SIGNING_PRIVATE_KEY_PASSWORD='' npm run bundle:mac
python3 scripts/prepare-desktop-release.py
```

The preparation script checks that the published runtime digest is embedded, the app and updater archive contain no Docker image, and both are below 200 MB. It stages the DMG, signed `.app.tar.gz`, signature, `latest.json`, checksums and dependency inventory under `release-assets/<version>/`. Publish these files in the public binary-only [desktop releases repository](https://github.com/iOSDevSK/html2wp-desktop-releases); the desktop source repository is public under its own commercial source-available licence. Existing pre-updater installations need one manual DMG installation. See [automatic setup](docs/automatic-setup.md) for the update and Docker Hub flow.

If `~/.ssh/github` exists, the publisher uses it as the dedicated SSH identity. Its matching `~/.ssh/github.pub` is the public key registered with GitHub; the `.pub` file alone cannot authenticate a push. Run `gh auth login --hostname github.com` before publishing if `gh auth status --active --hostname github.com` reports an invalid token.

Windows ZIP users: follow [WINDOWS_BUILD.md](WINDOWS_BUILD.md) or run `build-windows.cmd` after installing the prerequisites. The ZIP includes the pinned plugin source; Git initialization is optional for a local build.

## Keep the plugin current

The plugin is not bundled. The app fetches it from [iOSDevSK/html2wp-codex-plugin](https://github.com/iOSDevSK/html2wp-codex-plugin) at the tag `v<VERSION>` named by `VERSION` on `main`, keeps it under `<app data>/plugin/<version>/` and mounts `plugins/html2wp` read-only at `/opt/html2wp` into the Codex container and every project container. Once per launch, while no conversion runs, it installs a newer release by itself and says so. A plugin whose `.codex-plugin/plugin.json` names another `appContract` than the app supports (`plugin::SUPPORTED_CONTRACT`) is refused. `vendor/html2wp` is a development checkout only; nothing from it goes into the app or the runtime image.

The Git checkout registers the plugin as a submodule. Clone with `--recurse-submodules`, or run `git submodule update --init --recursive` after cloning. The portable source ZIP already contains the pinned submodule files and can be built without Git history. See [updates.md](docs/updates.md) for release signing, compatibility and rollback.

## Documents

- [Automatic setup](docs/automatic-setup.md)
- [Licence integration and third-party distribution notes](docs/desktop-licensing.md)
- [Updates and compatibility](docs/updates.md)
- [Desktop dependency inventory](notices/desktop.cdx.json)

Gutenberg from an HTML theme is hidden by default in New project. Enable it under **Settings → Experimental → Gutenberg conversion**. The choice is saved across app restarts; switching it off hides the import card and keeps existing Gutenberg projects accessible.

The Exports tab offers **Download original ZIP**, including before conversion. The project menu offers **Download diagnostic logs**. New ZIP imports are retained byte for byte outside the agent sandbox. Folder imports and older projects download the imported files as a new ZIP (files excluded during import cannot be reconstructed). Diagnostic ZIPs contain bounded reports, recent chat/activity and recorded tool output; known credential fields are redacted. Review diagnostics before sharing.

Preview prepares an installed Visual Edit plugin through the html2wp plugin before opening the page; an active Pro installation is preserved. Conversion reports include recorded model/effort selections and elapsed duration, with missing historical data labelled. Compare offers **Refresh selected page** as well as **Regenerate all pages**. Settings allows up to six parallel conversions, with two as the default.
