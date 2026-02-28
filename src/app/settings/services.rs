//! Jupiter API client and settings. Transactions are submitted via RPC only.

use crate::app::config;
use jupiter_swap_api_client::JupiterSwapApiClient;
use once_cell::sync::Lazy;

pub static JUPITER_ENDPOINT: Lazy<String> =
    Lazy::new(|| config::CONFIG.services.endpoint.clone());

pub static JUPITER_API_KEY: Lazy<Option<String>> = Lazy::new(|| {
    if config::CONFIG.services.auth_token.is_empty() {
        None
    } else {
        Some(config::CONFIG.services.auth_token.clone())
    }
});

pub static JUPITER_CLIENT: Lazy<JupiterSwapApiClient> =
    Lazy::new(|| JupiterSwapApiClient::new(JUPITER_ENDPOINT.clone(), JUPITER_API_KEY.clone()));
