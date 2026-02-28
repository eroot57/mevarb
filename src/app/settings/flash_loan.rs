use std::collections::HashMap;

use once_cell::sync::Lazy;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

use crate::app::config;
use crate::{FlashLoanContext, ReserveInfo};

/// Pre-built flash loan contexts keyed by token mint string.
/// None if flash loans are disabled in config.
pub static FLASH_LOAN_CONTEXTS: Lazy<Option<HashMap<String, FlashLoanContext>>> = Lazy::new(|| {
    let fl_config = &config::CONFIG.flash_loan;
    if !fl_config.enabled {
        return None;
    }

    let program_id = match Pubkey::from_str(&fl_config.program_id) {
        Ok(pk) => pk,
        Err(e) => {
            tracing::error!(error = %e, "Invalid flash_loan.program_id");
            return None;
        }
    };

    let mut contexts = HashMap::new();
    for reserve_cfg in &fl_config.reserves {
        let parse = |name: &str, value: &str| -> Option<Pubkey> {
            match Pubkey::from_str(value) {
                Ok(pk) => Some(pk),
                Err(e) => {
                    tracing::warn!(
                        mint = %reserve_cfg.token_mint,
                        field = name,
                        error = %e,
                        "Skipping reserve: bad address"
                    );
                    None
                }
            }
        };

        let token_mint = match parse("token_mint", &reserve_cfg.token_mint) {
            Some(pk) => pk,
            None => continue,
        };
        let flashloan_token_reserves_liquidity = match parse(
            "flashloan_token_reserves_liquidity",
            &reserve_cfg.flashloan_token_reserves_liquidity,
        ) {
            Some(pk) => pk,
            None => continue,
        };
        let flashloan_borrow_position_on_liquidity = match parse(
            "flashloan_borrow_position_on_liquidity",
            &reserve_cfg.flashloan_borrow_position_on_liquidity,
        ) {
            Some(pk) => pk,
            None => continue,
        };
        let rate_model = match parse("rate_model", &reserve_cfg.rate_model) {
            Some(pk) => pk,
            None => continue,
        };
        let vault = match parse("vault", &reserve_cfg.vault) {
            Some(pk) => pk,
            None => continue,
        };
        let liquidity = match parse("liquidity", &reserve_cfg.liquidity) {
            Some(pk) => pk,
            None => continue,
        };
        let liquidity_program = match parse("liquidity_program", &reserve_cfg.liquidity_program) {
            Some(pk) => pk,
            None => continue,
        };

        let ctx = FlashLoanContext::new(
            program_id,
            ReserveInfo {
                token_mint,
                flashloan_token_reserves_liquidity,
                flashloan_borrow_position_on_liquidity,
                rate_model,
                vault,
                liquidity,
                liquidity_program,
            },
        );
        contexts.insert(reserve_cfg.token_mint.clone(), ctx);
    }

    if contexts.is_empty() {
        tracing::warn!("Flash loans enabled but no valid reserves configured");
        return None;
    }

    tracing::info!(
        reserves = contexts.len(),
        "Flash loan contexts initialized"
    );
    Some(contexts)
});
