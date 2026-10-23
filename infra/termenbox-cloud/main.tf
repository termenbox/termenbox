# Termenbox Cloud Infrastructure
# Manages ONLY our hosted services: Cloudflare Pages (client) and DNS.
# GitHub Actions secrets and variables are set by hand: the PAT cannot write them, and an apply with a missing
# value would overwrite the real secret.
# Does NOT manage user VPSes — users self-host server via Docker
# GitHub OAuth App is created MANUALLY (no admin:oauth_app scope exists for PATs)

terraform {
  required_version = ">= 1.6"
  required_providers {
    cloudflare = {
      source  = "cloudflare/cloudflare"
      version = "~> 4.0"
    }
  }

  backend "s3" {
    bucket         = "termenbox-terraform-state"
    key            = "cloud/terraform.tfstate"
    region         = "auto"
    skip_credentials_validation = true
    skip_metadata_api_check     = true
    skip_region_validation      = true
    skip_requesting_account_id  = true
  }
}

provider "cloudflare" {
  api_token = var.cloudflare_api_token
}

locals {
  pages_project_name = coalesce(var.pages_project_name, "termenbox")
  prefixed           = var.pages_project_name != null && var.pages_project_name != ""
  dns_prefix         = local.prefixed ? "${var.pages_project_name}." : ""
  dns_name_prefix    = local.prefixed ? ".${var.pages_project_name}" : ""
  pages_subdomain    = "${local.pages_project_name}.pages.dev"
}

data "cloudflare_zone" "main" {
  name = var.cloudflare_zone_name
}

# Cloudflare Pages project for client hosting
resource "cloudflare_pages_project" "client" {
  account_id = var.cloudflare_account_id
  name       = local.pages_project_name
  production_branch = "master"

  # No build_config and no source: the client is built on GitHub (deploy-pages.yaml) and uploaded directly.
  # Cloudflare never builds this project. Do not connect it to Git in the dashboard.

  deployment_configs {
  }
}

# Custom domain for Pages
resource "cloudflare_pages_domain" "app" {
  account_id = var.cloudflare_account_id
  project_name = cloudflare_pages_project.client.name
  domain = "app.${local.dns_prefix}${var.cloudflare_zone_name}"
}

# DNS record for app subdomain (CNAME to Pages)
resource "cloudflare_record" "app" {
  zone_id = data.cloudflare_zone.main.id
  name    = "app${local.dns_name_prefix}"
  type    = "CNAME"
  content = local.pages_subdomain
  proxied = true
  ttl     = 1
}

# The beta and nightly clients (decision 0134): a service worker, its caches, localStorage and the OAuth callback are bound
# to an origin, so each channel has its own Pages project and its own host, never a /beta/ path. Built and uploaded
# by deploy-pages.yaml from the release workflow of the channel; they share the D1 database, the signing key and the salt
# of the stable project, and each has its own GitHub OAuth app (set by hand). Free-plan budget: fact 0022.
locals {
  preview_channels = toset(["beta", "nightly"])
}

resource "cloudflare_pages_project" "channel" {
  for_each          = local.preview_channels
  account_id        = var.cloudflare_account_id
  name              = "${local.pages_project_name}-${each.key}"
  production_branch = "master"

  # No build_config and no source, like the stable project.

  deployment_configs {
  }
}

resource "cloudflare_pages_domain" "channel" {
  for_each     = local.preview_channels
  account_id   = var.cloudflare_account_id
  project_name = cloudflare_pages_project.channel[each.key].name
  domain       = "${each.key}.app.${local.dns_prefix}${var.cloudflare_zone_name}"
}

resource "cloudflare_record" "channel" {
  for_each = local.preview_channels
  zone_id  = data.cloudflare_zone.main.id
  name     = "${each.key}.app${local.dns_name_prefix}"
  type     = "CNAME"
  content  = "${local.pages_project_name}-${each.key}.pages.dev"
  proxied  = true
  ttl      = 1
}

# Landing site (packages/site): static, built nowhere, uploaded by deploy-site.yaml. The apex is canonical; www is
# redirected to it at the edge (a Pages _redirects file cannot match a host).
locals {
  site_domain = local.prefixed ? "site.${local.dns_prefix}${var.cloudflare_zone_name}" : var.cloudflare_zone_name
  site_host   = local.prefixed ? "site${local.dns_name_prefix}" : "@"
}

resource "cloudflare_pages_project" "site" {
  account_id        = var.cloudflare_account_id
  name              = var.site_project_name
  production_branch = "master"

  # No build_config and no source, like the client project.

  deployment_configs {
  }
}

resource "cloudflare_pages_domain" "site" {
  account_id   = var.cloudflare_account_id
  project_name = cloudflare_pages_project.site.name
  domain       = local.site_domain
}

# CNAME at the apex: Cloudflare flattens it.
resource "cloudflare_record" "site" {
  zone_id = data.cloudflare_zone.main.id
  name    = local.site_host
  type    = "CNAME"
  content = "${var.site_project_name}.pages.dev"
  proxied = true
  ttl     = 1
}

resource "cloudflare_record" "www" {
  count   = local.prefixed ? 0 : 1
  zone_id = data.cloudflare_zone.main.id
  name    = "www"
  type    = "CNAME"
  content = "${var.site_project_name}.pages.dev"
  proxied = true
  ttl     = 1
}

# www.{zone} -> {zone}, permanent, path and query kept. Needs the token to edit zone rulesets.
resource "cloudflare_ruleset" "www_to_apex" {
  count   = local.prefixed ? 0 : 1
  zone_id = data.cloudflare_zone.main.id
  name    = "www to apex"
  kind    = "zone"
  phase   = "http_request_dynamic_redirect"

  rules {
    action      = "redirect"
    description = "Redirect www to the apex"
    enabled     = true
    expression  = "(http.host eq \"www.${var.cloudflare_zone_name}\")"

    action_parameters {
      from_value {
        status_code           = 301
        preserve_query_string = true
        target_url {
          expression = "concat(\"https://${var.cloudflare_zone_name}\", http.request.uri.path)"
        }
      }
    }
  }
}

output "site_url" {
  value = "https://${local.site_domain}"
}

output "client_url" {
  value = "https://app.${local.dns_prefix}${var.cloudflare_zone_name}"
}

output "channel_urls" {
  value = merge(
    { stable = "https://app.${local.dns_prefix}${var.cloudflare_zone_name}" },
    { for c in local.preview_channels : c => "https://${c}.app.${local.dns_prefix}${var.cloudflare_zone_name}" },
  )
}
