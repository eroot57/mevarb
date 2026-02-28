//! Jupiter Lend flash loan instruction builders and trade submission.
//!
//! Constructs flash borrow and payback instructions for Jupiter Lend (Solend-compatible).
//! Flash loans borrow tokens without collateral, repaying within the same transaction.
//!
//! Transaction layout for flash-loan-wrapped arbitrage:
//!   [advance_nonce, compute_unit_limit, compute_unit_price, flash_borrow,
//!    ...setup_ixs..., swap_ix, flash_payback]

use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    instruction::{AccountMeta, Instruction},
    message::{v0, VersionedMessage},
    pubkey::Pubkey,
    signer::Signer,
    sysvar,
    system_instruction::advance_nonce_account,
    transaction::VersionedTransaction,
};
use spl_associated_token_account::{
    get_associated_token_address,
    instruction::create_associated_token_account_idempotent,
};
use tracing::{error, info};

use crate::{
    FEES, NONCE_ADDR, PRIVATE_KEY, PUBKEY, SUBMIT_CLIENT, TOKEN_PROGRAM_ID,
    fetch_alt, get_nonce, get_swap_ix_flash_loan,
};

// Solend-compatible flash loan instruction indices.
const FLASH_BORROW_IX_TAG: u8 = 14;
const FLASH_REPAY_IX_TAG: u8 = 15;

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

/// Accounts needed to build flash loan instructions for a specific reserve.
#[derive(Debug, Clone)]
pub struct ReserveInfo {
    /// Reserve state account.
    pub reserve: Pubkey,
    /// Token account holding the reserve's liquidity (source of borrowed tokens).
    pub liquidity_supply: Pubkey,
    /// Token account that receives flash loan fees.
    pub fee_receiver: Pubkey,
    /// SPL token mint of the asset (e.g. WSOL mint).
    pub token_mint: Pubkey,
}

/// Everything needed to build flash borrow / payback instructions.
#[derive(Debug, Clone)]
pub struct FlashLoanContext {
    /// Jupiter Lend (Solend-compatible) program ID.
    pub program_id: Pubkey,
    /// Lending market account.
    pub lending_market: Pubkey,
    /// Lending market authority PDA (derived from lending_market + program_id).
    pub lending_market_authority: Pubkey,
    /// Reserve details for the token being borrowed.
    pub reserve_info: ReserveInfo,
}

impl FlashLoanContext {
    pub fn new(program_id: Pubkey, lending_market: Pubkey, reserve_info: ReserveInfo) -> Self {
        let (authority, _bump) =
            Pubkey::find_program_address(&[lending_market.as_ref()], &program_id);
        Self {
            program_id,
            lending_market,
            lending_market_authority: authority,
            reserve_info,
        }
    }
}

// ---------------------------------------------------------------------------
// Instruction builders
// ---------------------------------------------------------------------------

/// Build a flash-borrow instruction (Solend instruction 14).
///
/// Borrows `amount` of the reserve's token from the liquidity supply into the
/// user's associated token account.
pub fn build_flash_borrow_ix(
    ctx: &FlashLoanContext,
    amount: u64,
    user: &Pubkey,
) -> Instruction {
    let user_token_account = get_associated_token_address(user, &ctx.reserve_info.token_mint);

    let mut data = Vec::with_capacity(9);
    data.push(FLASH_BORROW_IX_TAG);
    data.extend_from_slice(&amount.to_le_bytes());

    let accounts = vec![
        AccountMeta::new(ctx.reserve_info.liquidity_supply, false), // source liquidity
        AccountMeta::new(user_token_account, false),                // destination (user ATA)
        AccountMeta::new(ctx.reserve_info.reserve, false),          // reserve state
        AccountMeta::new_readonly(ctx.lending_market, false),       // lending market
        AccountMeta::new_readonly(ctx.lending_market_authority, false), // market authority PDA
        AccountMeta::new_readonly(sysvar::instructions::id(), false),  // sysvar instructions
        AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),            // SPL token program
    ];

    Instruction {
        program_id: ctx.program_id,
        accounts,
        data,
    }
}

/// Build a flash-payback instruction (Solend instruction 15).
///
/// Repays `amount` from the user's associated token account back to the
/// reserve's liquidity supply.
///
/// `borrow_instruction_index` is the position of the corresponding
/// flash-borrow instruction in the transaction's instruction array.
/// The on-chain program uses the Sysvar Instructions account to verify
/// atomicity (borrow + repay in the same tx).
pub fn build_flash_payback_ix(
    ctx: &FlashLoanContext,
    amount: u64,
    user: &Pubkey,
    borrow_instruction_index: u8,
) -> Instruction {
    let user_token_account = get_associated_token_address(user, &ctx.reserve_info.token_mint);

    let mut data = Vec::with_capacity(10);
    data.push(FLASH_REPAY_IX_TAG);
    data.extend_from_slice(&amount.to_le_bytes());
    data.push(borrow_instruction_index);

    let accounts = vec![
        AccountMeta::new(user_token_account, false),                // source (user ATA)
        AccountMeta::new(ctx.reserve_info.liquidity_supply, false), // destination (reserve supply)
        AccountMeta::new(ctx.reserve_info.fee_receiver, false),     // reserve fee receiver
        AccountMeta::new(ctx.reserve_info.fee_receiver, false),     // host fee receiver
        AccountMeta::new(ctx.reserve_info.reserve, false),          // reserve state
        AccountMeta::new_readonly(ctx.lending_market, false),       // lending market
        AccountMeta::new(*user, true),                              // user (signer)
        AccountMeta::new_readonly(sysvar::instructions::id(), false), // sysvar instructions
        AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),           // SPL token program
    ];

    Instruction {
        program_id: ctx.program_id,
        accounts,
        data,
    }
}

// ---------------------------------------------------------------------------
// Flash-loan-wrapped trade submission
// ---------------------------------------------------------------------------

/// Submit an arbitrage trade wrapped in a flash loan.
///
/// Builds the full transaction manually (instead of using ultra_submit) to
/// have precise control over instruction ordering:
///
/// ```text
/// [0] advance_nonce_account
/// [1] set_compute_unit_limit
/// [2] set_compute_unit_price
/// [3] create_ata_idempotent     (ensure user's ATA exists)
/// [4] flash_borrow              ← borrow_instruction_index
/// [5..N-1] setup + swap ixs    (Jupiter arbitrage)
/// [N] flash_payback             (references index 4)
/// ```
pub async fn submit_flash_loan_trade(
    in_res: jupiter_swap_api_client::quote::QuoteResponse,
    out_res: jupiter_swap_api_client::quote::QuoteResponse,
    min_profit_amount: f64,
    decimal: u8,
    flash_ctx: &FlashLoanContext,
) {
    let borrow_amount = in_res.in_amount;

    // Build swap instructions with wrap_and_unwrap_sol = false since the
    // flash loan provides WSOL directly (no need for Jupiter to wrap native SOL).
    let ix = match get_swap_ix_flash_loan(
        in_res,
        out_res,
        (min_profit_amount * 10_f64.powf(decimal as f64)) as u64,
    )
    .await
    {
        Ok(ix) => ix,
        Err(e) => {
            error!(error = %e, "Failed to build swap_ix for flash loan trade");
            return;
        }
    };

    // ── Assemble instructions ───────────────────────────────────────────

    let mut all_ixs: Vec<Instruction> = Vec::new();

    // [0] Advance nonce account
    all_ixs.push(advance_nonce_account(&NONCE_ADDR, &PUBKEY));

    // [1] Compute unit limit
    all_ixs.push(ComputeBudgetInstruction::set_compute_unit_limit(
        FEES.compute_units as u32,
    ));

    // [2] Compute unit price (priority fee)
    all_ixs.push(ComputeBudgetInstruction::set_compute_unit_price(
        FEES.priority_lamports,
    ));

    // [3] Ensure user's ATA for the borrowed token exists
    all_ixs.push(create_associated_token_account_idempotent(
        &PUBKEY,                            // payer
        &PUBKEY,                            // wallet (owner of the ATA)
        &flash_ctx.reserve_info.token_mint, // mint
        &TOKEN_PROGRAM_ID,                  // token program
    ));

    // [4] Flash borrow
    let borrow_ix_index = all_ixs.len() as u8; // should be 4
    all_ixs.push(build_flash_borrow_ix(flash_ctx, borrow_amount, &PUBKEY));

    // Destructure swap instruction response to avoid partial-move issues.
    let setup_instructions = ix.setup_instructions;
    let swap_instruction = ix.swap_instruction;
    let alt_addresses = ix.address_lookup_table_addresses;

    // [5..N-1] Jupiter swap setup + swap
    all_ixs.extend(setup_instructions);
    all_ixs.push(swap_instruction);

    // [N] Flash payback
    all_ixs.push(build_flash_payback_ix(
        flash_ctx,
        borrow_amount,
        &PUBKEY,
        borrow_ix_index,
    ));

    // ── Build and sign VersionedTransaction ─────────────────────────────

    let total_ix_count = all_ixs.len();

    let nonce_data = get_nonce();
    let recent_blockhash = nonce_data.blockhash();

    let alts = fetch_alt(alt_addresses).await;

    let v0_msg = match v0::Message::try_compile(&PUBKEY, &all_ixs, &alts, recent_blockhash) {
        Ok(m) => m,
        Err(e) => {
            error!(error = %e, "Failed to compile flash loan v0 message");
            return;
        }
    };

    let versioned_msg = VersionedMessage::V0(v0_msg);

    let tx = match VersionedTransaction::try_new(versioned_msg, &[&*PRIVATE_KEY]) {
        Ok(t) => t,
        Err(e) => {
            error!(error = %e, "Failed to sign flash loan transaction");
            return;
        }
    };

    // ── Submit via RPC ──────────────────────────────────────────────────

    info!(
        borrow_amount = borrow_amount,
        borrow_ix_index = borrow_ix_index,
        total_ixs = total_ix_count,
        "Submitting flash-loan-wrapped trade"
    );

    match SUBMIT_CLIENT.send_transaction(&tx).await {
        Ok(sig) => {
            info!(signature = %sig, "Flash loan trade submitted");
        }
        Err(e) => {
            error!(error = %e, "Flash loan trade submission failed");
        }
    }
}
