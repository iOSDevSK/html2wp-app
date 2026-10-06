# Windows code signing (Azure Artifact Signing)

The Windows workflow (`.github/workflows/windows.yml`) signs the installer, the
app, the uninstaller and the NSIS plugins as **BELNEM s.r.o.** through Azure
Artifact Signing. It logs in to Azure through OIDC, so no secret is stored.

## What stays and what is deleted between releases

The Artifact Signing account costs USD 9.99 for every month in which it exists
(not pro-rated). To pay only in release months, delete the **account** after a
release and recreate it before the next one. Everything below survives that:

| Resource | Where | Survives deleting the account |
|---|---|---|
| Subscription | Azure subscription 1 (`8d1ecacb-dc18-4b8b-8485-4102b27efb23`), pay-as-you-go | yes |
| Resource group | `html2wp-signing` (North Europe) | yes |
| Identity validation | BELNEM s.r.o., Public, id `9c882c41-d8f5-4700-a1b8-ba027cd0151d`, valid until 1 Aug 2029 | yes (it belongs to the subscription and is deleted only explicitly; if it ever disappears, a new one passed in about an hour with D-U-N-S `496762354`) |
| App registration | `html2wp-github-signing`, client `ec72d4ad-fb89-438b-bd68-d84fd9daa119`, tenant `25f93520-09d4-4803-9268-b711c052d703` | yes |
| Federated credential | GitHub `iOSDevSK/html2wp-app`, branch `windows` | yes |
| Role *Artifact Signing Certificate Profile Signer* for the app registration | on the resource group | yes |
| GitHub variables | `AZURE_CLIENT_ID`, `AZURE_TENANT_ID`, `AZURE_SUBSCRIPTION_ID` | yes |
| **Artifact Signing account** | `html2wpsigning`, North Europe, Basic | **deleted** |
| **Certificate profile** | `html2wp`, Public Trust, without street or postal code | **deleted with the account** |

While the account is deleted, every push to `windows` fails at the signing step;
that is expected.

## Recreate before a release (about 5 minutes)

Portal (signed in as hello@html2wp.dev):

1. **Artifact Signing Accounts › Create**: subscription *Azure subscription 1*,
   resource group `html2wp-signing`, name `html2wpsigning`, region **North
   Europe**, pricing **Basic**. (West Europe refused new customers on
   6 Oct 2026.)
2. In the account: **Objects › Identity validations** must list BELNEM s.r.o. as
   *Completed*.
3. **Objects › Certificate profiles › Create › Public Trust**: name `html2wp`,
   Verified CN and O *BELNEM s.r.o.*, street and postal code **not** included.
   The subject must read
   `CN=BELNEM s.r.o., O=BELNEM s.r.o., L=Bratislava, S=Bratislava, C=SK`.
4. Re-run the Windows workflow (`gh workflow run "Windows x64" --ref windows`)
   and read the *Verify signatures* table.

The same with the Azure CLI (`brew install azure-cli`, `az login`):

```bash
az provider register --namespace Microsoft.CodeSigning
az extension add --name artifact-signing
az artifact-signing create -g html2wp-signing -n html2wpsigning -l northeurope --sku Basic
az artifact-signing certificate-profile create -g html2wp-signing --account-name html2wpsigning \
  -n html2wp --profile-type PublicTrust \
  --identity-validation-id 9c882c41-d8f5-4700-a1b8-ba027cd0151d
```

If the name `html2wpsigning` cannot be reused, create the account under a new
name or in another EU region and set the repository variables
`ARTIFACT_SIGNING_ACCOUNT`, `ARTIFACT_SIGNING_ENDPOINT` (for example
`https://plc.codesigning.azure.net` for Poland Central) and, if the profile name
changes, `ARTIFACT_SIGNING_PROFILE`. The workflow falls back to
`html2wpsigning`, `https://neu.codesigning.azure.net` and `html2wp`.

## Delete after a release

Artifact Signing Accounts › `html2wpsigning` › **Delete**. Do not delete the
identity validation, the resource group or the app registration. Files that
were already signed stay valid: every signature carries a timestamp from
`timestamp.acs.microsoft.com`, and the three-day certificates are not revoked.
