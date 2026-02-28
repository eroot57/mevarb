use std::collections::HashMap;

use once_cell::sync::Lazy;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

use crate::app::config;
use crate::engine::jupiter::flash_loan::{FlashLoanContext, ReserveInfo};

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
    let lending_market = match Pubkey::from_str(&fl_config.lending_market) {
        Ok(pk) => pk,
        Err(e) => {
            tracing::error!(error = %e, "Invalid flash_loan.lending_market");
            return None;
        }
    };

    let mut contexts = HashMap::new();
    for reserve_cfg in &fl_config.reserves {
        let token_mint = match Pubkey::from_str(&reserve_cfg.token_mint) {
            Ok(pk) => pk,
            Err(e) => {
                tracing::warn!(mint = %reserve_cfg.token_mint, error = %e, "Skipping reserve: bad token_mint");
                continue;
            }
        };
        let reserve = match Pubkey::from_str(&reserve_cfg.reserve) {
            Ok(pk) => pk,
            Err(e) => {
                tracing::warn!(mint = %reserve_cfg.token_mint, error = %e, "Skipping reserve: bad reserve");
                continue;
            }
        };
        let liquidity_supply = match Pubkey::from_str(&reserve_cfg.liquidity_supply) {
            Ok(pk) => pk,
            Err(e) => {
                tracing::warn!(mint = %reserve_cfg.token_mint, error = %e, "Skipping reserve: bad liquidity_supply");
                continue;
            }
        };
        let fee_receiver = match Pubkey::from_str(&reserve_cfg.fee_receiver) {
            Ok(pk) => pk,
            Err(e) => {
                tracing::warn!(mint = %reserve_cfg.token_mint, error = %e, "Skipping reserve: bad fee_receiver");
                continue;
            }
        };

        let ctx = FlashLoanContext::new(
            program_id,
            lending_market,
            ReserveInfo {
                reserve,
                liquidity_supply,
                fee_receiver,
                token_mint,
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
