//! Jupiter Lend flash loan instruction builders and trade submission.
//!
//! Constructs flash borrow and payback instructions for the Jupiter Lend Anchor program.
//! Program ID: jupgfSgfuAXv4B6R2Uxu85Z1qdzgju79s6MfZekN6XS
//!
//! Both `flashloan_borrow` and `flashloan_payback` take 14 accounts and a single
//! `amount: u64` argument, serialised as an 8-byte Anchor discriminator followed by
//! the borsh-encoded u64.
//!
//! Transaction layout for flash-loan-wrapped arbitrage:
//!   [advance_nonce, compute_unit_limit, compute_unit_price, create_ata,
//!    flash_borrow, ...setup_ixs..., swap_ix, flash_payback]

use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    instruction::{AccountMeta, Instruction},
    message::{v0, VersionedMessage},
    pubkey::Pubkey,
    signer::Signer,
    sysvar,
    system_instruction::advance_nonce_account,
    system_program,
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

/// Associated Token Program ID (constant from IDL).
const ASSOCIATED_TOKEN_PROGRAM_ID: Pubkey =
    Pubkey::from_str_const("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");

/// Anchor discriminator for `flashloan_borrow`.
const FLASH_BORROW_DISCRIMINATOR: [u8; 8] = [103, 19, 78, 24, 240, 9, 135, 63];

/// Anchor discriminator for `flashloan_payback`.
const FLASH_PAYBACK_DISCRIMINATOR: [u8; 8] = [213, 47, 153, 137, 84, 243, 94, 232];

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

/// Per-token account addresses needed for flash loan instructions.
#[derive(Debug, Clone)]
pub struct ReserveInfo {
    /// SPL token mint of the asset (e.g. WSOL mint).
    pub token_mint: Pubkey,
    /// Token reserves liquidity account (writable).
    pub flashloan_token_reserves_liquidity: Pubkey,
    /// Borrow position on liquidity account (writable).
    pub flashloan_borrow_position_on_liquidity: Pubkey,
    /// Rate model account (read-only).
    pub rate_model: Pubkey,
    /// Vault account (writable).
    pub vault: Pubkey,
    /// Liquidity account (read-only).
    pub liquidity: Pubkey,
    /// Liquidity program (read-only, must match flashloan_admin.liquidity_program).
    pub liquidity_program: Pubkey,
}

/// Everything needed to build flash borrow / payback instructions.
#[derive(Debug, Clone)]
pub struct FlashLoanContext {
    /// Jupiter Lend flash loan program ID.
    pub program_id: Pubkey,
    /// FlashloanAdmin PDA (derived from seed "flashloan_admin" + program_id).
    pub flashloan_admin: Pubkey,
    /// Reserve details for the token being borrowed.
    pub reserve_info: ReserveInfo,
}

impl FlashLoanContext {
    pub fn new(program_id: Pubkey, reserve_info: ReserveInfo) -> Self {
        let (flashloan_admin, _bump) =
            Pubkey::find_program_address(&[b"flashloan_admin"], &program_id);
        Self {
            program_id,
            flashloan_admin,
            reserve_info,
        }
    }
}

// ---------------------------------------------------------------------------
// Instruction builders
// ---------------------------------------------------------------------------

/// Build the 14-account list used by both borrow and payback instructions.
fn build_flash_loan_accounts(
    ctx: &FlashLoanContext,
    user: &Pubkey,
    user_token_account: &Pubkey,
) -> Vec<AccountMeta> {
    vec![
        AccountMeta::new(*user, true),                                                    // signer
        AccountMeta::new(ctx.flashloan_admin, false),                                     // flashloan_admin (PDA)
        AccountMeta::new(*user_token_account, false),                                     // signer_borrow_token_account (ATA)
        AccountMeta::new_readonly(ctx.reserve_info.token_mint, false),                    // mint
        AccountMeta::new(ctx.reserve_info.flashloan_token_reserves_liquidity, false),     // flashloan_token_reserves_liquidity
        AccountMeta::new(ctx.reserve_info.flashloan_borrow_position_on_liquidity, false), // flashloan_borrow_position_on_liquidity
        AccountMeta::new_readonly(ctx.reserve_info.rate_model, false),                    // rate_model
        AccountMeta::new(ctx.reserve_info.vault, false),                                  // vault
        AccountMeta::new_readonly(ctx.reserve_info.liquidity, false),                     // liquidity
        AccountMeta::new_readonly(ctx.reserve_info.liquidity_program, false),             // liquidity_program
        AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),                               // token_program
        AccountMeta::new_readonly(ASSOCIATED_TOKEN_PROGRAM_ID, false),                    // associated_token_program
        AccountMeta::new_readonly(system_program::id(), false),                           // system_program
        AccountMeta::new_readonly(sysvar::instructions::id(), false),                     // instruction_sysvar
    ]
}

/// Build Anchor instruction data: 8-byte discriminator + borsh u64.
fn build_flash_loan_data(discriminator: &[u8; 8], amount: u64) -> Vec<u8> {
    let mut data = Vec::with_capacity(16);
    data.extend_from_slice(discriminator);
    data.extend_from_slice(&amount.to_le_bytes());
    data
}

/// Build a `flashloan_borrow` instruction.
///
/// Borrows `amount` of the reserve's token from the liquidity pool into the
/// user's associated token account.
pub fn build_flash_borrow_ix(
    ctx: &FlashLoanContext,
    amount: u64,
    user: &Pubkey,
) -> Instruction {
    let user_token_account = get_associated_token_address(user, &ctx.reserve_info.token_mint);

    Instruction {
        program_id: ctx.program_id,
        accounts: build_flash_loan_accounts(ctx, user, &user_token_account),
        data: build_flash_loan_data(&FLASH_BORROW_DISCRIMINATOR, amount),
    }
}

/// Build a `flashloan_payback` instruction.
///
/// Repays `amount` from the user's associated token account back to the
/// liquidity pool. The on-chain program uses the Sysvar Instructions account
/// to verify that a matching borrow exists in the same transaction.
pub fn build_flash_payback_ix(
    ctx: &FlashLoanContext,
    amount: u64,
    user: &Pubkey,
) -> Instruction {
    let user_token_account = get_associated_token_address(user, &ctx.reserve_info.token_mint);

    Instruction {
        program_id: ctx.program_id,
        accounts: build_flash_loan_accounts(ctx, user, &user_token_account),
        data: build_flash_loan_data(&FLASH_PAYBACK_DISCRIMINATOR, amount),
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
/// [4] flash_borrow
/// [5..N-1] setup + swap ixs    (Jupiter arbitrage)
/// [N] flash_payback
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
    // flash loan provides tokens directly in SPL form.
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
