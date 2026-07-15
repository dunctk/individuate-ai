#!/usr/bin/env bash
set -euo pipefail

mode="${1:-}"
base_url="${2:-${APP_BASE_URL:-}}"
if [[ "$mode" != "sandbox" && "$mode" != "live" ]] || [[ -z "$base_url" ]]; then
    echo "Usage: $0 sandbox|live https://your-domain.example" >&2
    exit 2
fi
if [[ ! "$base_url" =~ ^https:// ]]; then
    echo "The webhook base URL must use HTTPS" >&2
    exit 2
fi

if [[ -f .env ]]; then
    set -a
    source .env
    set +a
fi
if [[ "$mode" == "live" ]]; then
    stripe_key="${STRIPE_LIVE_SECRET_KEY:-}"
    secret_name="STRIPE_LIVE_WEBHOOK_SECRET"
else
    stripe_key="${STRIPE_SANDBOX_SECRET_KEY:-}"
    secret_name="STRIPE_SANDBOX_WEBHOOK_SECRET"
fi
if [[ -z "$stripe_key" ]]; then
    echo "Missing Stripe secret key for $mode mode" >&2
    exit 1
fi

webhook_url="${base_url%/}/api/stripe/webhook"
existing="$(curl -sS --fail-with-body -u "${stripe_key}:" -G \
    https://api.stripe.com/v1/webhook_endpoints --data-urlencode "limit=100")"
if jq -e --arg url "$webhook_url" '.data[] | select(.url == $url)' <<<"$existing" >/dev/null; then
    echo "A webhook already exists at $webhook_url." >&2
    echo "Use Stripe Workbench to reveal or rotate its signing secret." >&2
    exit 1
fi

webhook="$(curl -sS --fail-with-body -u "${stripe_key}:" \
    https://api.stripe.com/v1/webhook_endpoints \
    --data-urlencode "url=${webhook_url}" \
    --data-urlencode "description=IndividuateAI subscription access" \
    --data-urlencode "enabled_events[]=checkout.session.completed" \
    --data-urlencode "enabled_events[]=customer.subscription.created" \
    --data-urlencode "enabled_events[]=customer.subscription.updated" \
    --data-urlencode "enabled_events[]=customer.subscription.deleted")"

printf '%s=%s\n' "$secret_name" "$(jq -r '.secret' <<<"$webhook")"
echo "Save that value in the matching runtime environment now; Stripe only returns it at creation." >&2
