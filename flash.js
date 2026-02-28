/**
 * Jupiter Lend Flash Loan - WSOL Version (Fixed)
 *
 * Based on the working patterns from giraphant/Hachimedes
 * Uses @jup-ag/lend/flashloan with the correct API (asset mint, not vaultId)
 *
 * Flow:
 *   1. Flash Borrow WSOL
 *   2. [Your operations here]
 *   3. Flash Payback WSOL
 *
 * Dependencies:
 *   "@jup-ag/lend": "^0.0.101"
 *   "@solana/web3.js": "^1.95.8"
 *   "bn.js": "^5.2.1"
 *   "bs58": "4.0.1"
 */

const {
  Connection,
  Keypair,
  PublicKey,
  TransactionMessage,
  VersionedTransaction,
  ComputeBudgetProgram,
  LAMPORTS_PER_SOL,
} = require('@solana/web3.js');
const { getFlashBorrowIx, getFlashPaybackIx } = require('@jup-ag/lend/flashloan');
const BN = require('bn.js');
const bs58 = require('bs58');

// ─── Configuration ───────────────────────────────────────────────────────────
// Use environment variables for sensitive data!
const RPC_URL = process.env.RPC_URL || 'https://ellette-cyy4xd-fast-mainnet.helius-rpc.com';
const PRIVATE_KEY = process.env.PRIVATE_KEY || '5NrK9aCNBscbBXGqMUZhzmPJuzhhnBRzpcf82h8J8g2iBMRwTB8nJxHEWsTLszxKnvzZEVo2UFDVMH3uDgDAqrDs';

// WSOL mint address on Solana mainnet
const WSOL_MINT = new PublicKey('So11111111111111111111111111111111111111112');

async function executeJupiterFlashLoan() {
  console.log('╔════════════════════════════════════════════════════════╗');
  console.log('║   Jupiter Lend Flash Loan – WSOL (Fixed Version)      ║');
  console.log('╚════════════════════════════════════════════════════════╝\n');

  // ── 1. Validate config ──────────────────────────────────────────────────
  if (!PRIVATE_KEY) {
    console.error('❌ PRIVATE_KEY environment variable is not set.');
    console.error('   Usage: PRIVATE_KEY=<your-bs58-key> RPC_URL=<rpc> node flash.js');
    process.exit(1);
  }

  // ── 2. Setup connection and wallet ──────────────────────────────────────
  const connection = new Connection(RPC_URL, 'confirmed');

  // bs58 v4.0.1: bs58.decode() is a direct function that returns a Buffer
  const wallet = Keypair.fromSecretKey(bs58.decode(PRIVATE_KEY));

  console.log('📍 Wallet Address:', wallet.publicKey.toBase58());

  const balance = await connection.getBalance(wallet.publicKey);
  console.log('💰 Current Balance:', (balance / LAMPORTS_PER_SOL).toFixed(6), 'SOL\n');

  if (balance < 0.0005 * LAMPORTS_PER_SOL) {
    console.log('⚠️  You need at least 0.005 SOL for transaction fees.');
    return;
  }

  // ── 3. Flash loan parameters ────────────────────────────────────────────
  // Borrow 0.01 SOL (= 10_000_000 lamports) as WSOL
  const flashLoanAmountRaw = 10_000_000; // lamports
  const flashLoanAmount = new BN(flashLoanAmountRaw);

  console.log('📊 Flash Loan Configuration:');
  console.log('   Asset: WSOL (Wrapped SOL)');
  console.log('   Mint:', WSOL_MINT.toBase58());
  console.log('   Amount:', flashLoanAmountRaw, 'lamports');
  console.log('   =', (flashLoanAmountRaw / LAMPORTS_PER_SOL).toFixed(6), 'SOL');
  console.log('   💸 Fee: FREE (0%)\n');

  try {
    // ── 4. Build Flash Borrow instruction ───────────────────────────────
    // The correct API uses `asset` (mint PublicKey), NOT `vaultId`.
    // See: giraphant/Hachimedes lib/deleverage-swap-flashloan.ts lines 50-55
    console.log('[1/2] Building Flash Borrow instruction...');
    const flashBorrowIx = await getFlashBorrowIx({
      asset: WSOL_MINT,
      amount: flashLoanAmount,
      signer: wallet.publicKey,
      connection,
    });
    console.log('   ✓ Flash Borrow instruction created');

    // ── 5. Build Flash Payback instruction ──────────────────────────────
    // Must pass the same `amount` and `asset` as the borrow.
    // See: giraphant/Hachimedes lib/deleverage-swap-flashloan.ts lines 125-130
    console.log('[2/2] Building Flash Payback instruction...');
    const flashPaybackIx = await getFlashPaybackIx({
      asset: WSOL_MINT,
      amount: flashLoanAmount,
      signer: wallet.publicKey,
      connection,
    });
    console.log('   ✓ Flash Payback instruction created\n');

    // ── 6. Assemble the transaction ─────────────────────────────────────
    // The SDK returns proper TransactionInstruction objects — no need
    // to manually reconstruct them.
    const computeBudgetIx = ComputeBudgetProgram.setComputeUnitLimit({
      units: 400_000,
    });

    const instructions = [
      computeBudgetIx,
      flashBorrowIx,

      // ════════════════════════════════════════════════════════════════
      //  YOUR OPERATIONS GO HERE
      //  Examples:
      //    - Jupiter swap instructions (arbitrage)
      //    - Liquidation instructions
      //    - Any DeFi operation that returns WSOL before payback
      // ════════════════════════════════════════════════════════════════

      flashPaybackIx,
    ];

    // ── 7. Build versioned transaction ──────────────────────────────────
    const { blockhash, lastValidBlockHeight } = await connection.getLatestBlockhash('confirmed');

    const messageV0 = new TransactionMessage({
      payerKey: wallet.publicKey,
      recentBlockhash: blockhash,
      instructions,
    }).compileToV0Message();

    const transaction = new VersionedTransaction(messageV0);
    transaction.sign([wallet]);

    // Check transaction size
    const serialized = transaction.serialize();
    console.log('📦 Transaction size:', serialized.length, '/ 1232 bytes');

    // ── 8. Simulate ─────────────────────────────────────────────────────
    console.log('\n🔄 Simulating transaction...');
    const simulation = await connection.simulateTransaction(transaction, {
      commitment: 'confirmed',
    });

    if (simulation.value.err) {
      console.log('\n❌ Simulation failed!');
      console.log('Error:', JSON.stringify(simulation.value.err, null, 2));
      console.log('\n📋 Logs:');
      simulation.value.logs?.forEach((log) => console.log('   ', log));
      return;
    }

    console.log('✅ Simulation successful!\n');

    // Show relevant logs
    console.log('📋 Transaction logs:');
    simulation.value.logs?.slice(0, 10).forEach((log) => console.log('   ', log));

    console.log('\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━');
    console.log('✅ TRANSACTION READY');
    console.log('━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n');

    // ── 9. (Optional) Send transaction ──────────────────────────────────
    // Uncomment the block below to actually broadcast on-chain.
    
    console.log('💫 Sending transaction to network...');

    const signature = await connection.sendTransaction(transaction, {
      skipPreflight: false,
      maxRetries: 3,
    });

    console.log('📝 Signature:', signature);
    console.log('🔗 Solscan:  ', `https://solscan.io/tx/${signature}`);

    console.log('\n⏳ Confirming...');
    const confirmation = await connection.confirmTransaction(
      { signature, blockhash, lastValidBlockHeight },
      'confirmed'
    );

    if (confirmation.value.err) {
      console.log('❌ Transaction failed:', confirmation.value.err);
    } else {
      console.log('✅ Transaction confirmed!');
      console.log('🎉 Flash loan executed successfully!');
    }
    

    console.log('💡 What this transaction does (atomically):');
    console.log('   1. Borrow', (flashLoanAmountRaw / LAMPORTS_PER_SOL).toFixed(6), 'WSOL from Jupiter Lend (FREE)');
    console.log('   2. [Your operations would go here]');
    console.log('   3. Repay', (flashLoanAmountRaw / LAMPORTS_PER_SOL).toFixed(6), 'WSOL');
    console.log('\n🎯 Next steps:');
    console.log('   • Add Jupiter swap / arbitrage instructions between borrow & payback');
    console.log('   • Use address lookup tables for complex transactions');
    console.log('   • Only send if profitable!');
  } catch (error) {
    console.error('\n❌ Error:', error.message);
    if (error.stack) {
      console.error('\nStack trace:', error.stack);
    }
  }
}

// ─── Run ─────────────────────────────────────────────────────────────────────
executeJupiterFlashLoan()
  .then(() => {
    console.log('\n✨ Script completed.');
    process.exit(0);
  })
  .catch((error) => {
    console.error('\n❌ Fatal error:', error);
    process.exit(1);
  });
