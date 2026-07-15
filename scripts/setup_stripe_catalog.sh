#!/usr/bin/env bash
set -euo pipefail

mode="${1:-sandbox}"
if [[ "$mode" != "sandbox" && "$mode" != "live" ]]; then
    echo "Usage: $0 sandbox|live" >&2
    exit 2
fi

if [[ -f .env ]]; then
    set -a
    source .env
    set +a
fi

if [[ "$mode" == "live" ]]; then
    stripe_key="${STRIPE_LIVE_SECRET_KEY:-}"
else
    stripe_key="${STRIPE_SANDBOX_SECRET_KEY:-}"
fi
if [[ -z "$stripe_key" ]]; then
    echo "Missing Stripe secret key for $mode mode" >&2
    exit 1
fi

stripe_api="https://api.stripe.com/v1"

request() {
    curl -sS --fail-with-body -u "${stripe_key}:" "$@"
}

products="$(request -G "${stripe_api}/products" --data-urlencode "active=true" --data-urlencode "limit=100")"
product_id="$(jq -r '.data[] | select(.metadata.individuate_product == "plus") | .id' <<<"$products" | head -n1)"
if [[ -z "$product_id" ]]; then
    product="$(request -X POST "${stripe_api}/products" \
        --data-urlencode "name=IndividuateAI" \
        --data-urlencode "description=Reflective AI conversations with encrypted memory, editable maps, relationship context, and voice." \
        --data-urlencode "metadata[individuate_product]=plus")"
    product_id="$(jq -r '.id' <<<"$product")"
fi
request -X POST "${stripe_api}/products/${product_id}" \
    --data-urlencode "tax_code=txcd_10103000" >/dev/null

ensure_price() {
    local lookup_key="$1"
    local currency="$2"
    local amount="$3"
    local interval="$4"
    local tax_behavior="$5"
    local prices price_id
    prices="$(request -G "${stripe_api}/prices" --data-urlencode "lookup_keys[]=${lookup_key}" --data-urlencode "active=true")"
    price_id="$(jq -r '.data[0].id // empty' <<<"$prices")"
    if [[ -z "$price_id" ]]; then
        local price
        price="$(request -X POST "${stripe_api}/prices" \
            --data-urlencode "product=${product_id}" \
            --data-urlencode "currency=${currency}" \
            --data-urlencode "unit_amount=${amount}" \
            --data-urlencode "recurring[interval]=${interval}" \
            --data-urlencode "tax_behavior=${tax_behavior}" \
            --data-urlencode "lookup_key=${lookup_key}")"
        price_id="$(jq -r '.id' <<<"$price")"
    fi
    printf '%s\n' "$price_id"
}

usd_monthly="$(ensure_price individuate_plus_usd_monthly usd 2499 month exclusive)"
usd_annual="$(ensure_price individuate_plus_usd_annual usd 23900 year exclusive)"
eur_monthly="$(ensure_price individuate_plus_eur_monthly eur 2999 month inclusive)"
eur_annual="$(ensure_price individuate_plus_eur_annual eur 28900 year inclusive)"

portal_configs="$(request -G "${stripe_api}/billing_portal/configurations" --data-urlencode "limit=10")"
portal_id="$(jq -r '.data[] | select(.active == true) | .id' <<<"$portal_configs" | head -n1)"
if [[ -z "$portal_id" ]]; then
    portal="$(request -X POST "${stripe_api}/billing_portal/configurations" \
        --data-urlencode "default_return_url=https://individuateai.com/chat" \
        --data-urlencode "business_profile[headline]=Manage your IndividuateAI subscription" \
        --data-urlencode "business_profile[privacy_policy_url]=https://individuateai.com/privacy-and-security" \
        --data-urlencode "features[customer_update][enabled]=true" \
        --data-urlencode "features[customer_update][allowed_updates][]=address" \
        --data-urlencode "features[customer_update][allowed_updates][]=tax_id" \
        --data-urlencode "features[invoice_history][enabled]=true" \
        --data-urlencode "features[payment_method_update][enabled]=true" \
        --data-urlencode "features[subscription_cancel][enabled]=true" \
        --data-urlencode "features[subscription_cancel][mode]=at_period_end" \
        --data-urlencode "features[subscription_cancel][cancellation_reason][enabled]=true" \
        --data-urlencode "features[subscription_cancel][cancellation_reason][options][]=too_expensive" \
        --data-urlencode "features[subscription_cancel][cancellation_reason][options][]=missing_features" \
        --data-urlencode "features[subscription_cancel][cancellation_reason][options][]=switched_service" \
        --data-urlencode "features[subscription_cancel][cancellation_reason][options][]=unused" \
        --data-urlencode "features[subscription_cancel][cancellation_reason][options][]=other" \
        --data-urlencode "features[subscription_update][enabled]=false")"
    portal_id="$(jq -r '.id' <<<"$portal")"
fi

prefix="STRIPE_$(tr '[:lower:]' '[:upper:]' <<<"$mode" | tr -d '\n')"
printf '%s\n' \
    "${prefix}_PRODUCT_ID=${product_id}" \
    "${prefix}_PORTAL_CONFIGURATION_ID=${portal_id}" \
    "${prefix}_PRICE_USD_MONTHLY=${usd_monthly}" \
    "${prefix}_PRICE_USD_ANNUAL=${usd_annual}" \
    "${prefix}_PRICE_EUR_MONTHLY=${eur_monthly}" \
    "${prefix}_PRICE_EUR_ANNUAL=${eur_annual}"
