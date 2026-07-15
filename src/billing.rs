use anyhow::{Context, Result};
use hmac::{Hmac, Mac};
use reqwest::Client;
use serde_json::Value;
use sha2::Sha256;
use std::time::{SystemTime, UNIX_EPOCH};

const STRIPE_API_BASE: &str = "https://api.stripe.com/v1";
const WEBHOOK_TOLERANCE_SECONDS: i64 = 300;

pub fn is_admin_email(email: &str) -> bool {
    std::env::var("ADMIN_EMAIL")
        .ok()
        .is_some_and(|configured| email_matches_admin(&configured, email))
}

fn email_matches_admin(configured: &str, email: &str) -> bool {
    let configured = configured.trim();
    !configured.is_empty() && configured.eq_ignore_ascii_case(email.trim())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BillingPlan {
    UsdMonthly,
    UsdAnnual,
    EurMonthly,
    EurAnnual,
}

impl BillingPlan {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "usd_monthly" => Some(Self::UsdMonthly),
            "usd_annual" => Some(Self::UsdAnnual),
            "eur_monthly" => Some(Self::EurMonthly),
            "eur_annual" => Some(Self::EurAnnual),
            _ => None,
        }
    }

    fn lookup_key(self) -> &'static str {
        match self {
            Self::UsdMonthly => "individuate_plus_usd_monthly",
            Self::UsdAnnual => "individuate_plus_usd_annual",
            Self::EurMonthly => "individuate_plus_eur_monthly",
            Self::EurAnnual => "individuate_plus_eur_annual",
        }
    }
}

#[derive(Clone)]
pub struct StripeConfig {
    client: Client,
    secret_key: String,
    webhook_secret: Option<String>,
    app_base_url: String,
    automatic_tax: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StripeSubscription {
    pub id: String,
    pub customer_id: String,
    pub status: String,
    pub price_id: String,
    pub current_period_end: Option<i64>,
    pub cancel_at_period_end: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckoutResult {
    pub user_id: String,
    pub customer_id: String,
    pub subscription_id: String,
}

impl StripeConfig {
    pub fn billing_enabled() -> bool {
        std::env::var("BILLING_ENABLED")
            .ok()
            .map(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes"
                )
            })
            .unwrap_or(true)
    }

    pub fn from_env() -> Result<Option<Self>> {
        if !Self::billing_enabled() {
            return Ok(None);
        }

        let mode = required_env("STRIPE_MODE")?;
        let prefix = match mode.trim().to_ascii_lowercase().as_str() {
            "sandbox" | "test" => "STRIPE_SANDBOX",
            "live" => "STRIPE_LIVE",
            _ => anyhow::bail!("STRIPE_MODE must be 'sandbox' or 'live'"),
        };
        let secret_key = required_env(&format!("{prefix}_SECRET_KEY"))?;
        let webhook_secret = std::env::var(format!("{prefix}_WEBHOOK_SECRET"))
            .ok()
            .filter(|value| !value.trim().is_empty());
        if prefix == "STRIPE_LIVE" && webhook_secret.is_none() {
            anyhow::bail!("Set STRIPE_LIVE_WEBHOOK_SECRET before enabling live billing");
        }
        if webhook_secret.is_none() {
            tracing::warn!(
                "Stripe sandbox webhook secret is not configured; Checkout success can be tested, but subscription lifecycle webhooks are unavailable"
            );
        }
        let app_base_url = required_env("APP_BASE_URL")?
            .trim_end_matches('/')
            .to_string();
        if !(app_base_url.starts_with("https://")
            || cfg!(debug_assertions) && app_base_url.starts_with("http://localhost"))
        {
            anyhow::bail!("APP_BASE_URL must use HTTPS outside local development");
        }

        Ok(Some(Self {
            client: Client::new(),
            secret_key,
            webhook_secret,
            app_base_url,
            automatic_tax: env_bool("STRIPE_AUTOMATIC_TAX", false),
        }))
    }

    pub fn app_base_url(&self) -> &str {
        &self.app_base_url
    }

    pub async fn create_checkout_session(
        &self,
        user_id: &str,
        email: &str,
        plan: BillingPlan,
        existing_customer: Option<&str>,
    ) -> Result<String> {
        let price_id = self.resolve_price(plan).await?;
        let success_url = format!(
            "{}/billing/success?session_id={{CHECKOUT_SESSION_ID}}",
            self.app_base_url
        );
        let cancel_url = format!("{}/subscribe?checkout=cancelled", self.app_base_url);
        let mut form = vec![
            ("mode".to_string(), "subscription".to_string()),
            ("client_reference_id".to_string(), user_id.to_string()),
            ("line_items[0][price]".to_string(), price_id),
            ("line_items[0][quantity]".to_string(), "1".to_string()),
            ("success_url".to_string(), success_url),
            ("cancel_url".to_string(), cancel_url),
            ("metadata[user_id]".to_string(), user_id.to_string()),
            (
                "subscription_data[metadata][user_id]".to_string(),
                user_id.to_string(),
            ),
            ("allow_promotion_codes".to_string(), "false".to_string()),
            (
                "automatic_tax[enabled]".to_string(),
                self.automatic_tax.to_string(),
            ),
        ];
        if let Some(customer) = existing_customer.filter(|value| !value.trim().is_empty()) {
            form.push(("customer".to_string(), customer.to_string()));
        } else {
            form.push(("customer_email".to_string(), email.to_string()));
        }
        let payload = self.post_form("/checkout/sessions", &form).await?;
        value_string(&payload, "url").context("Stripe Checkout response did not include a URL")
    }

    pub async fn retrieve_checkout(&self, session_id: &str) -> Result<CheckoutResult> {
        ensure_stripe_id(session_id, "cs_")?;
        let payload = self
            .get(&format!("/checkout/sessions/{session_id}"))
            .await?;
        if payload.get("payment_status").and_then(Value::as_str) != Some("paid") {
            anyhow::bail!("Checkout payment is not complete");
        }
        Ok(CheckoutResult {
            user_id: value_string(&payload, "client_reference_id")
                .context("Checkout session is missing its user reference")?,
            customer_id: expandable_id(payload.get("customer"))
                .context("Checkout session is missing its customer")?,
            subscription_id: expandable_id(payload.get("subscription"))
                .context("Checkout session is missing its subscription")?,
        })
    }

    pub async fn retrieve_subscription(&self, subscription_id: &str) -> Result<StripeSubscription> {
        ensure_stripe_id(subscription_id, "sub_")?;
        let payload = self
            .get(&format!("/subscriptions/{subscription_id}"))
            .await?;
        subscription_from_value(&payload)
    }

    pub async fn create_portal_session(&self, customer_id: &str) -> Result<String> {
        ensure_stripe_id(customer_id, "cus_")?;
        let form = vec![
            ("customer".to_string(), customer_id.to_string()),
            (
                "return_url".to_string(),
                format!("{}/chat", self.app_base_url),
            ),
        ];
        let payload = self.post_form("/billing_portal/sessions", &form).await?;
        value_string(&payload, "url").context("Stripe portal response did not include a URL")
    }

    pub fn verify_webhook(&self, body: &[u8], signature: &str) -> Result<()> {
        let secret = self
            .webhook_secret
            .as_deref()
            .context("Stripe webhook signing secret is not configured")?;
        verify_webhook_signature(body, signature, secret, unix_timestamp())
    }

    async fn get(&self, path: &str) -> Result<Value> {
        let response = self
            .client
            .get(format!("{STRIPE_API_BASE}{path}"))
            .bearer_auth(&self.secret_key)
            .send()
            .await
            .context("Stripe request failed")?;
        stripe_json(response).await
    }

    async fn resolve_price(&self, plan: BillingPlan) -> Result<String> {
        let response = self
            .client
            .get(format!("{STRIPE_API_BASE}/prices"))
            .bearer_auth(&self.secret_key)
            .query(&[("lookup_keys[]", plan.lookup_key()), ("active", "true")])
            .send()
            .await
            .context("Stripe price lookup failed")?;
        let payload = stripe_json(response).await?;
        payload
            .pointer("/data/0/id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .with_context(|| format!("Stripe price '{}' does not exist", plan.lookup_key()))
    }

    async fn post_form(&self, path: &str, form: &[(String, String)]) -> Result<Value> {
        let response = self
            .client
            .post(format!("{STRIPE_API_BASE}{path}"))
            .bearer_auth(&self.secret_key)
            .form(form)
            .send()
            .await
            .context("Stripe request failed")?;
        stripe_json(response).await
    }
}

pub fn subscription_from_value(value: &Value) -> Result<StripeSubscription> {
    let price_id = value
        .pointer("/items/data/0/price")
        .and_then(|price| expandable_id(Some(price)))
        .unwrap_or_default();
    Ok(StripeSubscription {
        id: value_string(value, "id").context("Subscription is missing its ID")?,
        customer_id: expandable_id(value.get("customer"))
            .context("Subscription is missing its customer")?,
        status: value_string(value, "status").context("Subscription is missing its status")?,
        price_id,
        current_period_end: value.get("current_period_end").and_then(Value::as_i64),
        cancel_at_period_end: value
            .get("cancel_at_period_end")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

pub fn event_user_id(object: &Value) -> Option<String> {
    object
        .pointer("/metadata/user_id")
        .and_then(Value::as_str)
        .or_else(|| object.get("client_reference_id").and_then(Value::as_str))
        .map(str::to_string)
}

fn required_env(name: &str) -> Result<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .with_context(|| format!("Set {name}"))
}

fn env_bool(name: &str, default: bool) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes"
            )
        })
        .unwrap_or(default)
}

fn ensure_stripe_id(value: &str, prefix: &str) -> Result<()> {
    if value.starts_with(prefix)
        && value.len() <= 255
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        Ok(())
    } else {
        anyhow::bail!("Invalid Stripe object ID")
    }
}

fn value_string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

fn expandable_id(value: Option<&Value>) -> Option<String> {
    value.and_then(|value| {
        value
            .as_str()
            .map(str::to_string)
            .or_else(|| value.get("id").and_then(Value::as_str).map(str::to_string))
    })
}

async fn stripe_json(response: reqwest::Response) -> Result<Value> {
    let status = response.status();
    let payload: Value = response
        .json()
        .await
        .context("Invalid response from Stripe")?;
    if status.is_success() {
        Ok(payload)
    } else {
        let message = payload
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or("Stripe rejected the request");
        anyhow::bail!("{message}")
    }
}

fn unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

fn verify_webhook_signature(body: &[u8], signature: &str, secret: &str, now: i64) -> Result<()> {
    let mut timestamp = None;
    let mut signatures = Vec::new();
    for part in signature.split(',').map(str::trim) {
        if let Some(value) = part.strip_prefix("t=") {
            timestamp = value.parse::<i64>().ok();
        } else if let Some(value) = part.strip_prefix("v1=") {
            if let Ok(decoded) = hex::decode(value) {
                signatures.push(decoded);
            }
        }
    }
    let timestamp = timestamp.context("Stripe signature is missing its timestamp")?;
    if now.abs_diff(timestamp) > WEBHOOK_TOLERANCE_SECONDS as u64 {
        anyhow::bail!("Stripe webhook timestamp is outside the allowed tolerance");
    }
    let mut signed = timestamp.to_string().into_bytes();
    signed.push(b'.');
    signed.extend_from_slice(body);
    let valid = signatures.into_iter().any(|candidate| {
        Hmac::<Sha256>::new_from_slice(secret.as_bytes())
            .map(|mut mac| {
                mac.update(&signed);
                mac.verify_slice(&candidate).is_ok()
            })
            .unwrap_or(false)
    });
    if !valid {
        anyhow::bail!("Invalid Stripe webhook signature");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_known_plans() {
        assert_eq!(
            BillingPlan::parse("usd_monthly"),
            Some(BillingPlan::UsdMonthly)
        );
        assert_eq!(
            BillingPlan::parse("eur_annual"),
            Some(BillingPlan::EurAnnual)
        );
        assert_eq!(BillingPlan::parse("free"), None);
    }

    #[test]
    fn admin_email_matching_is_trimmed_and_case_insensitive() {
        assert!(email_matches_admin(" D@uncan.net ", "d@UNCAN.NET"));
        assert!(!email_matches_admin("", "d@uncan.net"));
        assert!(!email_matches_admin("other@example.com", "d@uncan.net"));
    }

    #[test]
    fn verifies_a_current_webhook_signature() {
        let body = br#"{"id":"evt_test"}"#;
        let timestamp = 1_700_000_000;
        let mut mac = Hmac::<Sha256>::new_from_slice(b"whsec_test").unwrap();
        mac.update(format!("{timestamp}.").as_bytes());
        mac.update(body);
        let signature = format!(
            "t={timestamp},v1={}",
            hex::encode(mac.finalize().into_bytes())
        );
        assert!(verify_webhook_signature(body, &signature, "whsec_test", timestamp).is_ok());
        assert!(verify_webhook_signature(body, &signature, "wrong", timestamp).is_err());
    }

    #[test]
    fn rejects_old_webhook_signatures() {
        let signature = "t=100,v1=00";
        assert!(verify_webhook_signature(b"{}", signature, "secret", 1000).is_err());
    }
}
