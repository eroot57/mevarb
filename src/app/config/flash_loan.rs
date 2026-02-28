use serde::Deserialize;

fn default_false() -> bool {
    false
}

fn default_program_id() -> String {
    "jupgfSgfuAXv4B6R2Uxu85Z1qdzgju79s6MfZekN6XS".to_string()
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct FlashLoanConfig {
    #[serde(default = "default_false")]
    pub enabled: bool,
    /// Jupiter Lend flash loan program ID.
    /// Defaults to jupgfSgfuAXv4B6R2Uxu85Z1qdzgju79s6MfZekN6XS.
    #[serde(default = "default_program_id")]
    pub program_id: String,
    /// Per-token reserve configuration for flash loans.
    #[serde(default)]
    pub reserves: Vec<FlashLoanReserveConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct FlashLoanReserveConfig {
    /// The SPL token mint (e.g. WSOL mint).
    pub token_mint: String,
    /// Token reserves liquidity account.
    pub flashloan_token_reserves_liquidity: String,
    /// Borrow position on liquidity account.
    pub flashloan_borrow_position_on_liquidity: String,
    /// Rate model account.
    pub rate_model: String,
    /// Vault account.
    pub vault: String,
    /// Liquidity account.
    pub liquidity: String,
    /// Liquidity program account.
    pub liquidity_program: String,
}
