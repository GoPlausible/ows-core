use crate::error::{PayError, PayErrorCode};
use crate::types::{DiscoveredService, DiscoveryResponse, Protocol, Service};

const CDP_DISCOVERY_URL: &str = "https://api.cdp.coinbase.com/platform/v2/x402/discovery/resources";

const TESTNETS: &[&str] = &[
    "base-sepolia",
    "eip155:84532",
    "eip155:11155111",
    "solana-devnet",
];

// ===========================================================================
// Unified discovery (public API)
// ===========================================================================

/// Discover payable services across all protocols.
///
/// Fetches service directories, filters testnets, and returns a unified list.
pub async fn discover_all(query: Option<&str>) -> Result<Vec<Service>, PayError> {
    let x402_result = match query {
        Some(q) => search_x402(q).await,
        None => fetch_x402(Some(100), None).await,
    };

    let mut services = Vec::new();

    // Convert x402 services (filter testnets).
    for svc in x402_result.unwrap_or_default() {
        let accept = match svc.accepts.first() {
            Some(a) => a,
            None => continue,
        };

        let is_testnet = TESTNETS.iter().any(|t| accept.network.contains(t));
        if is_testnet {
            continue;
        }

        let desc = accept
            .description
            .as_deref()
            .or_else(|| svc.metadata.as_ref().and_then(|m| m.description.as_deref()))
            .unwrap_or("");

        services.push(Service {
            protocol: Protocol::X402,
            name: svc.resource.clone(),
            url: svc.resource,
            description: truncate(desc, 80),
            price: format_usdc(&accept.amount),
            network: accept.network.clone(),
            tags: vec![],
        });
    }

    Ok(services)
}

// ===========================================================================
// x402 fetching (internal)
// ===========================================================================

pub(crate) async fn fetch_x402(
    limit: Option<u64>,
    offset: Option<u64>,
) -> Result<Vec<DiscoveredService>, PayError> {
    let client = reqwest::Client::new();
    let resp = client
        .get(CDP_DISCOVERY_URL)
        .query(&[
            ("limit", limit.unwrap_or(100).to_string()),
            ("offset", offset.unwrap_or(0).to_string()),
        ])
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(PayError::new(
            PayErrorCode::DiscoveryFailed,
            format!("x402 discovery returned {status}: {body}"),
        ));
    }

    let body: DiscoveryResponse = resp.json().await.map_err(|e| {
        PayError::new(
            PayErrorCode::DiscoveryFailed,
            format!("failed to parse x402 discovery: {e}"),
        )
    })?;

    Ok(body.items)
}

async fn search_x402(query: &str) -> Result<Vec<DiscoveredService>, PayError> {
    let all = fetch_x402(Some(100), None).await?;
    let q = query.to_lowercase();

    Ok(all
        .into_iter()
        .filter(|s| {
            let url_match = s.resource.to_lowercase().contains(&q);
            let accepts_desc = s
                .accepts
                .first()
                .and_then(|a| a.description.as_ref())
                .map(|d| d.to_lowercase().contains(&q))
                .unwrap_or(false);
            let meta_desc = s
                .metadata
                .as_ref()
                .and_then(|m| m.description.as_ref())
                .map(|d| d.to_lowercase().contains(&q))
                .unwrap_or(false);
            url_match || accepts_desc || meta_desc
        })
        .collect())
}

// ===========================================================================
// Formatting helpers
// ===========================================================================

pub(crate) fn format_usdc(amount_str: &str) -> String {
    let amount: u128 = amount_str.parse().unwrap_or(0);
    let whole = amount / 1_000_000;
    let frac = amount % 1_000_000;
    let frac_str = format!("{frac:06}");
    let trimmed = frac_str.trim_end_matches('0');
    let trimmed = if trimmed.is_empty() { "00" } else { trimmed };
    format!("${whole}.{trimmed}")
}

fn truncate(s: &str, max: usize) -> String {
    let first_line = s.lines().next().unwrap_or("");
    if first_line.len() > max {
        format!("{}...", &first_line[..max.saturating_sub(3)])
    } else {
        first_line.to_string()
    }
}
