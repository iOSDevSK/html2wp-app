# Licences and distribution

## Public desktop source

The original desktop application source is public under the repository's
[commercial source-available licence](../LICENSE). A current html2wp key permits
commercial operation within its scope; publishing the source does not permit
redistributing the desktop app or building a competing conversion product.
GitHub users retain their GitHub Terms rights to view and fork the repository.

This desktop licence covers only material BELNEM s.r.o. can license. It does not
replace the separate html2wp plugin licence, the GPL-2.0-or-later licence of
`vendor/html2wp-to-gutenberg`, or the terms of dependencies and other
third-party material recorded under `notices/`. Generated themes remain the
user's Output under the desktop and plugin licences, subject to rights in their
inputs and other third-party content.

## User licence management

Settings supports **Free**, **Save & check**, **Refresh allowance**, and **Remove key · use Free**. **Buy a licence** opens the official [pricing page](https://html2wp.dev/pricing/) in the system browser. Saving/removing is blocked during an active conversion. A key is saved privately; it is never returned to the webview, inserted into model instructions, or added to a report/theme. The native allowance request sends it in `x-html2wp-key` over HTTPS with redirects disabled. Conversion clients read a private mounted key file.

The existing plugin's authoritative allowance endpoint is `GET https://api.html2wp.dev/v1/allowance`. Its checked-in client documents `credit.line` and `note`; these are displayed verbatim as data, without inventing counts or dates. Service-side limits remain authoritative at conversion creation.

The optional structured adapter accepts `licence` (or `license`) with `status`, `valid`, `expiresAt`, `neverExpires` and `plan`; root `status`, `valid` and `expiresAt` are also supported. **These optional fields still need confirmation from the API owner.** Test payloads demonstrate the UI contract, not an observed licensed response. No key is labelled active from the mere fact it was saved. Missing expiry is “Not supplied by service”; an offline response displays unavailable and labels cached credit as last-known.

The native Docker acceptance run used the existing saved licence through the normal private client path. The service returned “Licensed (Pro): no conversion, page or re-run limit, WooCommerce included.” No validity or expiry fields were supplied; those remain unconfirmed. The public [licence page](https://html2wp.dev/licenses/) describes Free and licensed access, but marketing pages are not used to calculate a user's current allowance.

## Actual bundled desktop material

| Material | Source/version | Treatment |
| --- | --- | --- |
| html2wp skill and scripts | Upstream commit in `runtime/versions.json` | Preserve the original source-available licence. Desktop patch file lists modifications; no original upstream file is rewritten in vendor/. |
| React, Tauri JavaScript APIs, Markdown renderer, icons and dependencies | Exact `package-lock.json` | Licence texts and declared expressions under `notices/`. |
| Rust/Tauri/SQLite/HTTP/crypto/archive dependencies | Exact `src-tauri/Cargo.lock` | Collected from filtered ARM64 Cargo metadata into `notices/`. |
| Protocol bindings | Generated from Codex 0.154.0 | Include provenance headers; Codex CLI is contained in the optional bundled runtime archive. |

`notices/desktop.cdx.json` is a CycloneDX desktop inventory, with conservative inclusion of some build/test dependencies. `notices/THIRD_PARTY_NOTICES.md` lists packages whose top-level notice could not be collected (28 in this run). This inventory is not a statement that every redistribution obligation is resolved.

## Runtime acquired after user action

The publisher prepares a Docker image at build/release time. Since desktop 1.0.13, **Prepare environment** downloads it from a public Docker Hub repository by immutable digest, or uses an already verified local image. The app bundle contains no Docker image archive. The image contains Node from the pinned base digest, Codex 0.157.0 from npm, Python/OS tools from Debian, Playwright 1.55.0, Pillow, Chromium through Playwright, and hash-verified WP-CLI. WordPress and MariaDB are separate digest-pinned images. WooCommerce is requested only for a shop, following the [official plugin release](https://wordpress.org/plugins/woocommerce/). An editor ZIP is used only when actually supplied by the conversion service. In the Lovable acceptance run the service reported Pro conversion allowance but offered the Free editor without a Pro ZIP; neither editor installation nor licence expiry is inferred from conversion credit.

Before publishing runtime images, generate an image SBOM and collect corresponding notices/sources for OS packages, Chromium and its third-party dependencies, PHP, Python, Node, Playwright, Pillow, WordPress, WooCommerce, MariaDB, WP-CLI and Codex. Preserve Codex's Apache-2.0 notices and inspect its actual distribution. Debian package versions are not fully reproducible until the resulting runtime digest is published.

Automatic setup downloads Docker Desktop directly from its vendor when missing, verifies the pinned checksum, and invokes the system installer. It does not accept Docker terms for the user. OS authorization and licence prompts remain visible. The html2wp Free/Pro licence does not grant a Docker subscription. See `automatic-setup.md` for actual platform test and distribution status. The desktop source and binary release are public under the distinct terms described above; input assets retain their original rights.

## Regenerate inventory

```sh
mkdir -p .cache
cargo metadata --manifest-path src-tauri/Cargo.toml --locked --filter-platform aarch64-apple-darwin --format-version 1 > .cache/cargo-metadata.json
node scripts/notices.mjs
```

Repeat for each release target and for actual runtime images. The macOS beta bundles the collected desktop notices and upstream licence.
