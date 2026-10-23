# Termenbox Bootstrap Guide

This guide walks through setting up the cloud infrastructure (Cloudflare + GitHub OAuth) required to run Termenbox.

**You only need this guide to host your own client** (your domain, your GitHub OAuth app, your signed directory). To use the hosted client at app.termenbox.dev, skip it: see the README quick start (DECISION-0052).

What you set up once:
1. A domain on Cloudflare
2. A Cloudflare API token and a GitHub token
3. A GitHub OAuth app (manual, once)
4. `terraform apply` (Pages project, DNS, repository secrets) and a D1 database for the directory
5. Servers: the Add Server command in your own client installs them

---

## Prerequisites

- GitHub account
- Cloudflare account (free tier works)
- A domain managed by Cloudflare (e.g., `yourdomain.com`)
- Local machine with `git`, `docker`, `docker compose`, `terraform`

---

## 1. Prerequisites (One-time Setup)

### 1.1 Add Domain to Cloudflare
1. Add your domain to Cloudflare (if not already)
2. **No manual DNS needed** — Terraform creates `app.{zone}` / `api.{zone}` (or `app.{project}.{zone}` / `api.{project}.{zone}` if `pages_project_name` is set) automatically

### 1.2 Create Cloudflare API Token
1. Go to **My Profile → API Tokens → Create Token**
2. Use **Custom token** template:
   - **Permissions**:
     - Account → Cloudflare Pages → Edit
     - Account → D1 → Edit
     - Account → R2 → Edit
     - Zone → DNS → Edit
     - Zone → Zone → Read
   - **Account Resources**: Include your account
   - **Zone Resources**: Include your zone (`yourdomain.com`)
3. Save as `CLOUDFLARE_API_TOKEN`

### 1.2.1 Create R2 API Token (for Terraform state backend)
1. Go to **R2 → Manage R2 API tokens → Create API token**
2. **Permissions**: Object Read & Write (or Edit)
3. **Account Resources**: Include your account
4. Save **Access Key ID** as `R2_ACCESS_KEY_ID`
5. Save **Secret Access Key** as `R2_SECRET_ACCESS_KEY`

> **Note**: The main `CLOUDFLARE_API_TOKEN` (step 1.2) also needs **Account → R2 → Edit** permission to create the R2 bucket via Terraform. The R2 API token (step 1.2.1) is only used by Terraform's S3 backend for state storage.

### 1.3 Create GitHub Personal Access Tokens

You need **two tokens** for different purposes:

**A. Fine-grained PAT (for GitHub Actions secrets/variables via Terraform):**
1. Go to **GitHub Settings → Developer settings → Personal access tokens → Fine-grained tokens → Generate new token**
2. **Repository access**: `termenbox/termenbox` (or your fork)
3. **Permissions**:
   - Repository → Actions → Read/Write (for secrets/variables)
   - Repository → Administration → Read/Write (for repo settings)
   - Repository → Variables → Read/Write
3. Save as `TERMENBOX_GITHUB_ACTIONS_TOKEN` — used as `github_actions_token` in Terraform (set as secret `TERMENBOX_GITHUB_ACTIONS_TOKEN` in GitHub repo)

**B. Classic PAT (for GitHub CLI / manual API calls only):**
1. Go to **GitHub Settings → Developer settings → Personal access tokens → Tokens (classic) → Generate new token (classic)**
2. **Scopes**: `repo`, `admin:org` (for org-level operations if needed)
3. Save as `GITHUB_CLASSIC_TOKEN` — used for `gh` CLI and manual API calls

> **Note**: There is NO `admin:oauth_app` scope in GitHub tokens. GitHub OAuth Apps must be created manually (see step 1.4).

### 1.4 Create GitHub OAuth App (Manual — One-time)
1. Go to **GitHub Settings → Developer settings → OAuth Apps → New OAuth App**
2. Fill in:
   - **Application name**: `Termenbox` (or your preferred name)
   - **Homepage URL**: `https://app.${your_zone_name}` (or `https://app.${project}.${your_zone_name}` if using `pages_project_name`)
   - **Authorization callback URL**: `https://app.${your_zone_name}/callback` (or `https://app.${project}.${your_zone_name}/callback`)
3. Click **Register application**
4. **Generate a new client secret**
5. Save both **Client ID** and **Client Secret** — you'll need them for Terraform

> **Note**: Use the actual domain from Terraform output `client_url` after running `terraform apply`. You can update the OAuth App later if needed.

---

## 2. Run Terraform (Automates Everything Else)

```bash
cd infra/termenbox-cloud

# Create terraform.tfvars
cat > terraform.tfvars <<EOF
cloudflare_api_token         = "your-cf-api-token"
cloudflare_account_id        = "your-cf-account-id"
cloudflare_zone_name         = "yourdomain.com"
cloudflare_pages_deploy_token = "your-pages-deploy-token"
github_actions_token         = "your-fine-grained-pat"
github_oauth_client_id       = "your-github-oauth-client-id"
github_oauth_client_secret   = "your-github-oauth-client-secret"
github_repository            = "termenbox"
# Optional: Pages project name (enables app.{project}.{zone} / api.{project}.{zone})
# pages_project_name = "myapp"
# R2 backend credentials (for Terraform state)
r2_access_key_id     = "your-r2-access-key-id"
r2_secret_access_key = "your-r2-secret-access-key"
EOF

# Initialize and apply (creates everything below)
terraform init
terraform plan
terraform apply
```

**Terraform creates automatically:**
- ✅ Cloudflare Pages project (`termenbox` by default, or `pages_project_name` if set)
- ✅ Custom domain: `app.{zone}` (default) or `app.{project}.{zone}` (if `pages_project_name` set)
- ✅ DNS records: `app.{zone}` / `api.{zone}` (default) or `app.{project}.{zone}` / `api.{project}.{zone}`
- ✅ GitHub Actions secrets: `CLOUDFLARE_API_TOKEN`, `CLOUDFLARE_ACCOUNT_ID`
- ✅ GitHub Actions secret: `TERMENBOX_GITHUB_CLIENT_ID` (from Terraform input)

**You provide manually (from step 1.4):**
- `oauth_client_id` (GitHub OAuth App Client ID)
- `oauth_client_secret` (GitHub OAuth App Client Secret)

---

## 3. Create the directory database and set the Pages Function secrets

The client's Pages project also hosts the signed server directory (DECISION-0056), which needs a D1 database.
Your API token needs the D1 Edit permission.

```bash
wrangler d1 create termanch-directory          # note the database_id
# GitHub repo variable read by the deploy workflow:
gh variable set TERMENBOX_D1_DATABASE_ID --body <database_id>
```

The deploy workflow applies `packages/client/migrations/` for you.

If you deploy by hand, or upgrade an existing directory, apply every migration before deploying the new functions: `wrangler d1 migrations apply termanch-directory --remote`. Migration `0002_owner_servers.sql` (account links for "Delete my data") is required: without it every server announce fails with a database error.

The client's Pages Function exchanges the GitHub login code, so it needs the OAuth app credentials.
The servers need no secrets.

The deploy workflow syncs these from the GitHub secrets `TERMENBOX_GITHUB_CLIENT_ID`, `TERMENBOX_GITHUB_CLIENT_SECRET`, `TERMENBOX_CERT_SIGNING_KEY` and `TERMENBOX_DIRECTORY_SALT`. To set them by hand instead:

```bash
wrangler pages secret put GITHUB_CLIENT_ID --project-name <pages project>
wrangler pages secret put GITHUB_CLIENT_SECRET --project-name <pages project>
# Ed25519 key that signs device certificates (DECISION-0062); its public half goes in packages/server/cert-keys.txt
wrangler pages secret put CERT_SIGNING_KEY --project-name <pages project>
# Random string (for example `openssl rand -base64 32`) that salts the address and account hashes in the directory's logs and rate limits
wrangler pages secret put DIRECTORY_SALT --project-name <pages project>
```

---

## 4. Add servers

Open your client (the Terraform output `client_url`, for example `https://app.yourdomain.com`), log in with GitHub and choose **Add Server**. The command it shows is the same one as on the hosted client, with your client's address:

```bash
curl -fsSL https://app.yourdomain.com/i | sh
```

It carries no secret. The client build writes your address into the installer it serves at `/i` as the default, so the server pairs with *your* client and directory: set the repository variable `TERMENBOX_CLIENT_ORIGIN` to `https://app.yourdomain.com` (`gh variable set TERMENBOX_CLIENT_ORIGIN --body https://app.yourdomain.com`) before the first deploy. Left unset, `/i` pairs with the hosted client.

Run it on the VPS. See the [install guide](guide/install.md) for what it does, firewall notes and updating/removing. The image comes from `ghcr.io/termenbox/termenbox:latest`; to use your own build set `TERMENBOX_IMAGE` before the command.

---

## 5. Verify end to end

1. Log in, choose Add Server, run the command on a VPS.
2. The server appears in the list within a minute. Tap it: a terminal opens, `ls` works.
3. Run `docker restart termenbox` on the VPS: the app reconnects by itself within about 10 seconds.

---

## 6. CI/CD

**On push to `master`:**
- CI: `cargo fmt`, `clippy`, build, tests (with tmux), installer syntax check, client build and tests.
- Docker image built for amd64 and arm64 and pushed to `ghcr.io/<owner>/termenbox:latest` (pull requests build it without pushing).
- Client, Pages Functions (OAuth exchange, directory) and the installer (`/i`) deployed to Cloudflare Pages; D1 migrations applied.

Each release channel deploys its own client (stable `app.`, beta `beta.app.`, nightly `nightly.app.`, Pages projects `termenbox`, `termenbox-beta`, `termenbox-nightly`) from its release workflow, never from a push to a branch. Create the GitHub Environments `stable`, `beta` and `nightly`; each holds its own `TERMENBOX_GITHUB_CLIENT_ID` and `TERMENBOX_GITHUB_CLIENT_SECRET` (one GitHub OAuth app per origin) and a `TERMENBOX_CLIENT_ORIGIN` variable naming its address, the rest stays repository-level.

Required repository settings: secrets `CLOUDFLARE_API_TOKEN`, `CLOUDFLARE_ACCOUNT_ID`, `TERMENBOX_GITHUB_CLIENT_ID`, `TERMENBOX_GITHUB_CLIENT_SECRET`, `TERMENBOX_CERT_SIGNING_KEY`, `TERMENBOX_DIRECTORY_SALT` (the last four are synced to the Pages Function on each deploy); variables `TERMENBOX_PAGES_PROJECT_NAME`, `TERMENBOX_D1_DATABASE_ID`. Terraform sets most of them.

**Manual Terraform:**
```bash
gh workflow run deploy-infra.yaml -f action=plan -f environment=cloud
gh workflow run deploy-infra.yaml -f action=apply -f environment=cloud
```

---

## Troubleshooting

| Issue | Solution |
|-------|----------|
| "Server unreachable on UDP port N" | Open that UDP port in the provider firewall / security group; some networks block UDP (tunnel fallback is on the roadmap, issue #26) |
| Server never appears after the command | `docker logs termenbox` on the VPS; the claim secret works for 15 minutes, run Add Server again for a new command |
| "This server belongs to another GitHub user" | Log in with the account that added it, or run a new Add Server command on the VPS: root on the host may re-claim |
| Login fails | Check `VITE_GITHUB_CLIENT_ID` (repo variable) and the Pages secrets `GITHUB_CLIENT_ID` / `GITHUB_CLIENT_SECRET` |
| Directory returns 500 | The D1 database binding is missing: check `TERMENBOX_D1_DATABASE_ID` and the deploy log |
| Terraform apply fails | Check API token permissions; verify the domain is on Cloudflare |

---

## Security notes

- Secrets live in GitHub Actions secrets / Pages secrets; the server holds no OAuth secret.
- Terminal traffic is end to end (DTLS) between browser and server; the directory only stores signed addresses.
- Claim secrets are single use and expire after 15 minutes; the directory never learns them (it sees a hash and an HMAC proof).
- No secrets in logs or images.
