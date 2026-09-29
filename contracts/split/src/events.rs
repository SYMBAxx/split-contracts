use soroban_sdk::{symbol_short, Address, Env, Vec};

// ---------------------------------------------------------------------------
// Core invoice events
// ---------------------------------------------------------------------------

/// Emitted when a new invoice is created.
pub fn invoice_created(env: &Env, invoice_id: u64, creator: &Address, total: i128) {
    env.events().publish(
        (symbol_short!("inv_crt"), invoice_id),
        (creator.clone(), total),
    );
}

/// Emitted when a payment is received toward an invoice.
pub fn payment_received(env: &Env, invoice_id: u64, payer: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("inv_pay"), invoice_id),
        (payer.clone(), amount),
    );
}

/// Emitted when an invoice is fully funded and funds are released.
pub fn invoice_released(env: &Env, invoice_id: u64, recipients: &Vec<Address>) {
    env.events().publish(
        (symbol_short!("inv_rel"), invoice_id),
        recipients.clone(),
    );
}

/// Emitted when an invoice is refunded after deadline.
pub fn invoice_refunded(env: &Env, invoice_id: u64) {
    env.events()
        .publish((symbol_short!("inv_ref"), invoice_id), ());
}

// ---------------------------------------------------------------------------
// #865 – Creator collateral events
// ---------------------------------------------------------------------------

/// Emitted when a creator deposits collateral.
pub fn collateral_deposited(env: &Env, creator: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("col_dep"), creator.clone()),
        amount,
    );
}

/// Emitted when a creator withdraws collateral.
pub fn collateral_withdrawn(env: &Env, creator: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("col_wth"), creator.clone()),
        amount,
    );
}

/// Emitted when collateral is locked to back an invoice.
pub fn collateral_locked(env: &Env, creator: &Address, invoice_id: u64, amount: i128) {
    env.events().publish(
        (symbol_short!("col_lck"), creator.clone()),
        (invoice_id, amount),
    );
}

/// Emitted when collateral is unlocked after invoice completion.
pub fn collateral_unlocked(env: &Env, creator: &Address, invoice_id: u64, amount: i128) {
    env.events().publish(
        (symbol_short!("col_ulk"), creator.clone()),
        (invoice_id, amount),
    );
}

// ---------------------------------------------------------------------------
// #866 – Rating / discount events
// ---------------------------------------------------------------------------

/// Emitted when a creator receives a new rating.
pub fn creator_rated(env: &Env, creator: &Address, score: u32, new_avg_bps: u32) {
    env.events().publish(
        (symbol_short!("rat_sub"), creator.clone()),
        (score, new_avg_bps),
    );
}

/// Emitted when a discount is applied to an invoice based on creator rating.
pub fn discount_applied(env: &Env, invoice_id: u64, creator: &Address, discount_bps: u32) {
    env.events().publish(
        (symbol_short!("disc_app"), invoice_id),
        (creator.clone(), discount_bps),
    );
}

// ---------------------------------------------------------------------------
// #867 – Recipient performance / routing events
// ---------------------------------------------------------------------------

/// Emitted when a recipient's performance score is updated.
pub fn performance_updated(env: &Env, recipient: &Address, score: i32) {
    env.events().publish(
        (symbol_short!("perf_upd"), recipient.clone()),
        score,
    );
}

/// Emitted when a recipient is flagged for poor performance.
pub fn recipient_flagged(env: &Env, recipient: &Address, invoice_id: u64) {
    env.events().publish(
        (symbol_short!("rec_flg"), recipient.clone()),
        invoice_id,
    );
}

// ---------------------------------------------------------------------------
// #868 – Covenant events
// ---------------------------------------------------------------------------

/// Emitted when a covenant is created for an invoice.
pub fn covenant_created(env: &Env, invoice_id: u64, creator: &Address, penalty_amount: i128) {
    env.events().publish(
        (symbol_short!("cov_crt"), invoice_id),
        (creator.clone(), penalty_amount),
    );
}

/// Emitted when a covenant is fulfilled (invoice released successfully).
pub fn covenant_fulfilled(env: &Env, invoice_id: u64, creator: &Address) {
    env.events().publish(
        (symbol_short!("cov_ful"), invoice_id),
        creator.clone(),
    );
}

/// Emitted when a covenant is violated and a penalty is applied.
pub fn covenant_violated(env: &Env, invoice_id: u64, creator: &Address, penalty_amount: i128) {
    env.events().publish(
        (symbol_short!("cov_vio"), invoice_id),
        (creator.clone(), penalty_amount),
    );
}
