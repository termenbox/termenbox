variable "cloudflare_api_token" {
  description = "Cloudflare API Token with Zone/Pages/DNS permissions"
  type        = string
  sensitive   = true
}

variable "cloudflare_account_id" {
  description = "Cloudflare Account ID"
  type        = string
}

variable "cloudflare_zone_name" {
  description = "Cloudflare zone (domain) for app.termenbox.dev"
  type        = string
}

variable "r2_access_key_id" {
  description = "R2 Access Key ID for Terraform state backend"
  type        = string
  sensitive   = true
}

variable "r2_secret_access_key" {
  description = "R2 Secret Access Key for Terraform state backend"
  type        = string
  sensitive   = true
}

variable "pages_project_name" {
  description = "Optional Pages project name. If set, DNS records become app.{name}.{zone} and api.{name}.{zone}. If null or empty, uses app.{zone} and api.{zone} directly."
  type        = string
  default     = null
}

variable "site_project_name" {
  description = "Pages project of the landing site (packages/site), served at the zone apex and www redirected to it. On a prefixed fork the site is served at site.{name}.{zone} and there is no www."
  type        = string
  default     = "termenbox-site"
}
