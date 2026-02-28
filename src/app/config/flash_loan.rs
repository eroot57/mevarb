use serde::Deserialize;

fn default_false() -> bool {
    false
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct FlashLoanConfig {
    #[serde(default = "default_false")]
    pub enabled: bool,
    /// Jupiter Lend (Solend-compatible) program ID.
    #[serde(default)]
    pub program_id: String,
    /// Lending market account address.
    #[serde(default)]
    pub lending_market: String,
    /// Per-token reserve configuration for flash loans.
    #[serde(default)]
    pub reserves: Vec<FlashLoanReserveConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct FlashLoanReserveConfig {
    /// The SPL token mint (e.g. WSOL mint).
    pub token_mint: String,
    /// The reserve state account address.
    pub reserve: String,
    /// The reserve's liquidity supply token account (source of borrowed tokens).
    pub liquidity_supply: String,
    /// The reserve's fee receiver token account.
    pub fee_receiver: String,
}
