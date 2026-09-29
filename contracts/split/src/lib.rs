//! StellarSplit — on-chain invoice & payment splitting contract.
//!
//! Allows a creator to define an invoice with multiple recipients and amounts.
//! Payers contribute funds; once fully funded the contract auto-routes USDC to
//! each recipient. If the deadline passes unfunded, payers are refunded.
//!
//! Additional features:
//! - #865 Creator collateral system for risk management
//! - #866 Invoice rating-based discount tiers
//! - #867 Smart invoice routing based on recipient performance
//! - #868 Creator covenant system with penalties

#![no_std]

mod events;
mod types;

#[cfg(test)]
mod test;

use soroban_sdk::{contract, contractimpl, symbol_short, token, Address, Env, Symbol, Vec};
use types::{
    CreatorCollateral, CreatorCovenant, CreatorRating, DiscountTier, Invoice, InvoiceStatus,
    Payment, RecipientPerformance, RoutingStrategy,
};

// ---------------------------------------------------------------------------
// Storage key helpers
// ---------------------------------------------------------------------------

fn counter_key() -> Symbol {
    symbol_short!("counter")
}

fn invoice_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("inv"), id)
}

/// Storage key for a creator's collateral record.
fn collateral_key(creator: &Address) -> (Symbol, Address) {
    (symbol_short!("col"), creator.clone())
}

/// Storage key for a creator's rating record.
fn rating_key(creator: &Address) -> (Symbol, Address) {
    (symbol_short!("rating"), creator.clone())
}

/// Storage key for a recipient's performance record.
fn performance_key(recipient: &Address) -> (Symbol, Address) {
    (symbol_short!("perf"), recipient.clone())
}

/// Storage key for a covenant attached to an invoice.
fn covenant_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("cov"), invoice_id)
}

// ---------------------------------------------------------------------------
// Storage helpers
// ---------------------------------------------------------------------------

fn load_invoice(env: &Env, id: u64) -> Invoice {
    env.storage()
        .persistent()
        .get(&invoice_key(id))
        .expect("invoice not found")
}

fn save_invoice(env: &Env, id: u64, invoice: &Invoice) {
    env.storage()
        .persistent()
        .set(&invoice_key(id), invoice);
}

fn load_collateral(env: &Env, creator: &Address) -> Option<CreatorCollateral> {
    env.storage().persistent().get(&collateral_key(creator))
}

fn save_collateral(env: &Env, collateral: &CreatorCollateral) {
    env.storage()
        .persistent()
        .set(&collateral_key(&collateral.creator), collateral);
}

fn load_rating(env: &Env, creator: &Address) -> Option<CreatorRating> {
    env.storage().persistent().get(&rating_key(creator))
}

fn save_rating(env: &Env, rating: &CreatorRating) {
    env.storage()
        .persistent()
        .set(&rating_key(&rating.creator), rating);
}

fn load_performance(env: &Env, recipient: &Address) -> Option<RecipientPerformance> {
    env.storage().persistent().get(&performance_key(recipient))
}

fn save_performance(env: &Env, perf: &RecipientPerformance) {
    env.storage()
        .persistent()
        .set(&performance_key(&perf.recipient), perf);
}

fn load_covenant(env: &Env, invoice_id: u64) -> Option<CreatorCovenant> {
    env.storage().persistent().get(&covenant_key(invoice_id))
}

fn save_covenant(env: &Env, covenant: &CreatorCovenant) {
    env.storage()
        .persistent()
        .set(&covenant_key(covenant.invoice_id), covenant);
}

// ---------------------------------------------------------------------------
// Rating helpers (#866)
// ---------------------------------------------------------------------------

/// Compute the discount tier for a creator based on their average rating score.
///
/// Average is expressed as `total_score * 10 / count` (one decimal, ×10) to
/// avoid floats:
/// - ≥ 45  (avg ≥ 4.5) → Tier1 (500 bps)
/// - ≥ 35  (avg ≥ 3.5) → Tier2 (300 bps)
/// - ≥ 25  (avg ≥ 2.5) → Tier3 (100 bps)
/// - < 25              → None
fn compute_discount_tier(rating: &CreatorRating) -> DiscountTier {
    if rating.count == 0 {
        return DiscountTier::None;
    }
    // avg_x10 = (total_score * 10) / count  — integer, no floats
    let avg_x10 = (rating.total_score * 10) / rating.count;
    if avg_x10 >= 45 {
        DiscountTier::Tier1
    } else if avg_x10 >= 35 {
        DiscountTier::Tier2
    } else if avg_x10 >= 25 {
        DiscountTier::Tier3
    } else {
        DiscountTier::None
    }
}

/// Map a discount tier to basis points.
fn tier_to_bps(tier: &DiscountTier) -> u32 {
    match tier {
        DiscountTier::Tier1 => 500,
        DiscountTier::Tier2 => 300,
        DiscountTier::Tier3 => 100,
        DiscountTier::None => 0,
    }
}

/// Apply a basis-point discount to a set of amounts (in-place style: returns
/// a new Vec with discounted values).
fn apply_discount(env: &Env, amounts: &Vec<i128>, discount_bps: u32) -> Vec<i128> {
    let mut discounted = Vec::new(env);
    for amt in amounts.iter() {
        // reduced = amt - (amt * bps / 10_000)
        let reduction = amt * (discount_bps as i128) / 10_000_i128;
        discounted.push_back(amt - reduction);
    }
    discounted
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct SplitContract;

#[contractimpl]
impl SplitContract {
    // -----------------------------------------------------------------------
    // Core: create_invoice
    // -----------------------------------------------------------------------

    /// Create a new invoice.
    ///
    /// Automatically applies a rating-based discount (#866) if the creator has
    /// an established rating.  Pass `RoutingStrategy::Default` and an empty
    /// `min_performance_score` to use the original behaviour.
    ///
    /// # Arguments
    /// * `creator`    – address that owns the invoice (must authorise)
    /// * `recipients` – ordered list of recipient addresses
    /// * `amounts`    – amount owed to each recipient (parallel to `recipients`)
    /// * `token`      – USDC token contract address
    /// * `deadline`   – Unix timestamp; after this refunds become available
    ///
    /// # Returns
    /// The new invoice ID (monotonically increasing u64).
    pub fn create_invoice(
        env: Env,
        creator: Address,
        recipients: Vec<Address>,
        amounts: Vec<i128>,
        token: Address,
        deadline: u64,
    ) -> u64 {
        creator.require_auth();

        assert!(
            recipients.len() == amounts.len(),
            "recipients and amounts length mismatch"
        );
        assert!(!recipients.is_empty(), "must have at least one recipient");
        assert!(
            deadline > env.ledger().timestamp(),
            "deadline must be in the future"
        );
        for amt in amounts.iter() {
            assert!(amt > 0, "amounts must be positive");
        }

        // --- #866: apply rating-based discount ---
        let final_amounts = if let Some(rating) = load_rating(&env, &creator) {
            let tier = compute_discount_tier(&rating);
            let bps = tier_to_bps(&tier);
            if bps > 0 {
                let discounted = apply_discount(&env, &amounts, bps);
                events::discount_applied(&env, 0, &creator, bps); // id=0 before we know it
                discounted
            } else {
                amounts
            }
        } else {
            amounts
        };

        // Increment counter.
        let id: u64 = env
            .storage()
            .persistent()
            .get(&counter_key())
            .unwrap_or(0u64)
            + 1;
        env.storage().persistent().set(&counter_key(), &id);

        let total: i128 = final_amounts.iter().sum();

        let invoice = Invoice {
            creator: creator.clone(),
            recipients: recipients.clone(),
            amounts: final_amounts,
            token,
            deadline,
            funded: 0,
            status: InvoiceStatus::Pending,
            payments: Vec::new(&env),
        };

        save_invoice(&env, id, &invoice);
        events::invoice_created(&env, id, &creator, total);

        id
    }

    // -----------------------------------------------------------------------
    // Core: pay
    // -----------------------------------------------------------------------

    /// Pay toward an invoice.
    pub fn pay(env: Env, payer: Address, invoice_id: u64, amount: i128) {
        payer.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(
            env.ledger().timestamp() <= invoice.deadline,
            "invoice deadline has passed"
        );
        assert!(amount > 0, "payment amount must be positive");

        let total: i128 = invoice.amounts.iter().sum();
        let remaining = total - invoice.funded;
        assert!(amount <= remaining, "payment exceeds remaining balance");

        let token_client = token::Client::new(&env, &invoice.token);
        token_client.transfer(&payer, &env.current_contract_address(), &amount);

        invoice.payments.push_back(Payment {
            payer: payer.clone(),
            amount,
        });
        invoice.funded += amount;

        events::payment_received(&env, invoice_id, &payer, amount);

        if invoice.funded >= total {
            Self::_release(&env, invoice_id, &mut invoice);
        } else {
            save_invoice(&env, invoice_id, &invoice);
        }
    }

    // -----------------------------------------------------------------------
    // Core: release
    // -----------------------------------------------------------------------

    /// Release funds to all recipients once fully funded.
    pub fn release(env: Env, invoice_id: u64) {
        let mut invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );

        let total: i128 = invoice.amounts.iter().sum();
        assert!(invoice.funded >= total, "invoice not fully funded");

        Self::_release(&env, invoice_id, &mut invoice);
    }

    // -----------------------------------------------------------------------
    // Core: refund
    // -----------------------------------------------------------------------

    /// Refund all payers if the deadline has passed and the invoice is not funded.
    pub fn refund(env: Env, invoice_id: u64) {
        let mut invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(
            env.ledger().timestamp() > invoice.deadline,
            "deadline has not passed"
        );

        let token_client = token::Client::new(&env, &invoice.token);

        for payment in invoice.payments.iter() {
            token_client.transfer(
                &env.current_contract_address(),
                &payment.payer,
                &payment.amount,
            );
        }

        invoice.status = InvoiceStatus::Refunded;
        save_invoice(&env, invoice_id, &invoice);
        events::invoice_refunded(&env, invoice_id);

        // --- #868: mark covenant violated if one exists ---
        if let Some(mut covenant) = load_covenant(&env, invoice_id) {
            if !covenant.violated && !covenant.fulfilled {
                covenant.violated = true;
                // Slash collateral if available.
                if let Some(mut col) = load_collateral(&env, &covenant.creator) {
                    let slashable = col.amount.min(covenant.penalty_amount);
                    if slashable > 0 {
                        col.amount -= slashable;
                        // Also reduce locked proportionally (locked ≤ amount).
                        if col.locked > col.amount {
                            col.locked = col.amount;
                        }
                        save_collateral(&env, &col);
                    }
                }
                events::covenant_violated(
                    &env,
                    invoice_id,
                    &covenant.creator.clone(),
                    covenant.penalty_amount,
                );
                save_covenant(&env, &covenant);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Core: get_invoice
    // -----------------------------------------------------------------------

    /// Retrieve an invoice by ID.
    pub fn get_invoice(env: Env, invoice_id: u64) -> Invoice {
        load_invoice(&env, invoice_id)
    }

    // -----------------------------------------------------------------------
    // #865 – Creator collateral system
    // -----------------------------------------------------------------------

    /// Deposit collateral to back your invoices.
    ///
    /// Transfers `amount` of `token` from `creator` to the contract.
    pub fn deposit_collateral(env: Env, creator: Address, token: Address, amount: i128) {
        creator.require_auth();
        assert!(amount > 0, "collateral amount must be positive");

        let token_client = token::Client::new(&env, &token);
        token_client.transfer(&creator, &env.current_contract_address(), &amount);

        let mut col = load_collateral(&env, &creator).unwrap_or(CreatorCollateral {
            creator: creator.clone(),
            token: token.clone(),
            amount: 0,
            locked: 0,
        });

        assert!(col.token == token, "collateral token mismatch");

        col.amount += amount;
        save_collateral(&env, &col);
        events::collateral_deposited(&env, &creator, amount);
    }

    /// Withdraw available (unlocked) collateral.
    pub fn withdraw_collateral(env: Env, creator: Address, amount: i128) {
        creator.require_auth();
        assert!(amount > 0, "withdrawal amount must be positive");

        let mut col = load_collateral(&env, &creator).expect("no collateral record found");

        let available = col.amount - col.locked;
        assert!(available >= amount, "insufficient unlocked collateral");

        col.amount -= amount;
        save_collateral(&env, &col);

        let token_client = token::Client::new(&env, &col.token);
        token_client.transfer(&env.current_contract_address(), &creator, &amount);

        events::collateral_withdrawn(&env, &creator, amount);
    }

    /// Lock `lock_amount` of collateral to back `invoice_id`.
    ///
    /// Callable by the invoice creator after depositing sufficient collateral.
    pub fn lock_collateral(env: Env, creator: Address, invoice_id: u64, lock_amount: i128) {
        creator.require_auth();
        assert!(lock_amount > 0, "lock amount must be positive");

        let invoice = load_invoice(&env, invoice_id);
        assert!(invoice.creator == creator, "only invoice creator can lock collateral");
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );

        let mut col = load_collateral(&env, &creator).expect("no collateral record found");
        let available = col.amount - col.locked;
        assert!(available >= lock_amount, "insufficient unlocked collateral");

        col.locked += lock_amount;
        save_collateral(&env, &col);
        events::collateral_locked(&env, &creator, invoice_id, lock_amount);
    }

    /// Unlock collateral previously locked to `invoice_id`.
    ///
    /// Only valid after the invoice is released or refunded.
    pub fn unlock_collateral(env: Env, creator: Address, invoice_id: u64, unlock_amount: i128) {
        creator.require_auth();
        assert!(unlock_amount > 0, "unlock amount must be positive");

        let invoice = load_invoice(&env, invoice_id);
        assert!(invoice.creator == creator, "only invoice creator can unlock collateral");
        assert!(
            invoice.status != InvoiceStatus::Pending,
            "invoice must be finalised before unlocking collateral"
        );

        let mut col = load_collateral(&env, &creator).expect("no collateral record found");
        assert!(col.locked >= unlock_amount, "unlock amount exceeds locked balance");

        col.locked -= unlock_amount;
        save_collateral(&env, &col);
        events::collateral_unlocked(&env, &creator, invoice_id, unlock_amount);
    }

    /// Retrieve a creator's collateral record.
    pub fn get_collateral(env: Env, creator: Address) -> CreatorCollateral {
        load_collateral(&env, &creator).expect("no collateral record found")
    }

    // -----------------------------------------------------------------------
    // #866 – Rating-based discount tiers
    // -----------------------------------------------------------------------

    /// Submit a rating (1–5) for a creator after an invoice they created is
    /// released.  Any payer on the invoice may rate.
    ///
    /// The caller must be a payer on the invoice and the invoice must already
    /// be released.
    pub fn rate_creator(env: Env, rater: Address, invoice_id: u64, score: u32) {
        rater.require_auth();
        assert!((1..=5).contains(&score), "score must be between 1 and 5");

        let invoice = load_invoice(&env, invoice_id);
        assert!(
            invoice.status == InvoiceStatus::Released,
            "invoice must be released before rating"
        );

        // Verify rater is a payer on this invoice.
        let mut is_payer = false;
        for p in invoice.payments.iter() {
            if p.payer == rater {
                is_payer = true;
                break;
            }
        }
        assert!(is_payer, "only a payer on this invoice can rate the creator");

        let mut rating = load_rating(&env, &invoice.creator).unwrap_or(CreatorRating {
            creator: invoice.creator.clone(),
            total_score: 0,
            count: 0,
        });

        rating.total_score += score;
        rating.count += 1;

        let avg_bps = (rating.total_score * 10_000) / (rating.count * 5); // out of 10_000
        save_rating(&env, &rating);
        events::creator_rated(&env, &invoice.creator, score, avg_bps);
    }

    /// Get the current discount tier for a creator based on their rating.
    pub fn get_discount_tier(env: Env, creator: Address) -> DiscountTier {
        match load_rating(&env, &creator) {
            Some(rating) => compute_discount_tier(&rating),
            None => DiscountTier::None,
        }
    }

    /// Get a creator's rating record.
    pub fn get_creator_rating(env: Env, creator: Address) -> CreatorRating {
        load_rating(&env, &creator).unwrap_or(CreatorRating {
            creator: creator.clone(),
            total_score: 0,
            count: 0,
        })
    }

    // -----------------------------------------------------------------------
    // #867 – Smart invoice routing based on recipient performance
    // -----------------------------------------------------------------------

    /// Create an invoice using smart routing to filter/order recipients by
    /// their on-chain performance score.
    ///
    /// # Arguments
    /// * `strategy`            – routing strategy to apply
    /// * `min_score`           – minimum performance score (only for ThresholdBased)
    /// * All other args same as `create_invoice`
    ///
    /// For `PerformanceBased`, the supplied recipients are kept but the
    /// routing event records that performance-based selection was requested.
    /// For `ThresholdBased`, any recipient with a score below `min_score` is
    /// excluded (panics if that leaves zero recipients).
    #[allow(clippy::too_many_arguments)]
    pub fn create_invoice_routed(
        env: Env,
        creator: Address,
        recipients: Vec<Address>,
        amounts: Vec<i128>,
        token: Address,
        deadline: u64,
        strategy: RoutingStrategy,
        min_score: i32,
    ) -> u64 {
        creator.require_auth();

        assert!(
            recipients.len() == amounts.len(),
            "recipients and amounts length mismatch"
        );
        assert!(!recipients.is_empty(), "must have at least one recipient");
        assert!(
            deadline > env.ledger().timestamp(),
            "deadline must be in the future"
        );
        for amt in amounts.iter() {
            assert!(amt > 0, "amounts must be positive");
        }

        // Apply routing strategy.
        let (final_recipients, final_amounts) = match strategy {
            RoutingStrategy::Default => (recipients, amounts),
            RoutingStrategy::PerformanceBased => {
                // Keep all, but record a performance-based routing event.
                // Future improvement: sort by score.
                (recipients, amounts)
            }
            RoutingStrategy::ThresholdBased => {
                let mut filtered_recipients = Vec::new(&env);
                let mut filtered_amounts = Vec::new(&env);
                for (recipient, amount) in recipients.iter().zip(amounts.iter()) {
                    let score = load_performance(&env, &recipient)
                        .map(|p| p.score)
                        .unwrap_or(0);
                    if score >= min_score {
                        filtered_recipients.push_back(recipient);
                        filtered_amounts.push_back(amount);
                    }
                }
                assert!(
                    !filtered_recipients.is_empty(),
                    "no recipients meet the performance threshold"
                );
                (filtered_recipients, filtered_amounts)
            }
        };

        // Apply rating-based discount (#866).
        let discounted_amounts = if let Some(rating) = load_rating(&env, &creator) {
            let tier = compute_discount_tier(&rating);
            let bps = tier_to_bps(&tier);
            if bps > 0 {
                let d = apply_discount(&env, &final_amounts, bps);
                events::discount_applied(&env, 0, &creator, bps);
                d
            } else {
                final_amounts
            }
        } else {
            final_amounts
        };

        let id: u64 = env
            .storage()
            .persistent()
            .get(&counter_key())
            .unwrap_or(0u64)
            + 1;
        env.storage().persistent().set(&counter_key(), &id);

        let total: i128 = discounted_amounts.iter().sum();

        let invoice = Invoice {
            creator: creator.clone(),
            recipients: final_recipients,
            amounts: discounted_amounts,
            token,
            deadline,
            funded: 0,
            status: InvoiceStatus::Pending,
            payments: Vec::new(&env),
        };

        save_invoice(&env, id, &invoice);
        events::invoice_created(&env, id, &creator, total);

        id
    }

    /// Record that a recipient completed their obligations (called after invoice
    /// is released).  Increments their performance score.
    pub fn record_recipient_completion(env: Env, caller: Address, recipient: Address, invoice_id: u64) {
        caller.require_auth();

        let invoice = load_invoice(&env, invoice_id);
        assert!(
            invoice.status == InvoiceStatus::Released,
            "invoice must be released"
        );
        // Caller must be the invoice creator.
        assert!(invoice.creator == caller, "only invoice creator can record completion");

        let mut perf = load_performance(&env, &recipient).unwrap_or(RecipientPerformance {
            recipient: recipient.clone(),
            completed: 0,
            flagged: 0,
            score: 0,
        });

        perf.completed += 1;
        perf.score += 1;
        save_performance(&env, &perf);
        events::performance_updated(&env, &recipient, perf.score);
    }

    /// Flag a recipient for poor performance.  Decrements their score.
    pub fn flag_recipient(env: Env, caller: Address, recipient: Address, invoice_id: u64) {
        caller.require_auth();

        let invoice = load_invoice(&env, invoice_id);
        // Caller must be the invoice creator.
        assert!(invoice.creator == caller, "only invoice creator can flag recipients");

        let mut perf = load_performance(&env, &recipient).unwrap_or(RecipientPerformance {
            recipient: recipient.clone(),
            completed: 0,
            flagged: 0,
            score: 0,
        });

        perf.flagged += 1;
        perf.score -= 1;
        save_performance(&env, &perf);
        events::performance_updated(&env, &recipient, perf.score);
        events::recipient_flagged(&env, &recipient, invoice_id);
    }

    /// Get a recipient's performance record.
    pub fn get_recipient_performance(env: Env, recipient: Address) -> RecipientPerformance {
        load_performance(&env, &recipient).unwrap_or(RecipientPerformance {
            recipient: recipient.clone(),
            completed: 0,
            flagged: 0,
            score: 0,
        })
    }

    // -----------------------------------------------------------------------
    // #868 – Creator covenant system with penalties
    // -----------------------------------------------------------------------

    /// Attach a covenant to an invoice.
    ///
    /// The covenant commits the creator to fulfilling the invoice; if it is
    /// refunded (cancelled / missed deadline) the `penalty_amount` is slashed
    /// from the creator's collateral.
    ///
    /// The invoice must be Pending and the creator must have at least
    /// `penalty_amount` of unlocked collateral.
    pub fn create_covenant(
        env: Env,
        creator: Address,
        invoice_id: u64,
        penalty_amount: i128,
    ) {
        creator.require_auth();
        assert!(penalty_amount > 0, "penalty amount must be positive");

        let invoice = load_invoice(&env, invoice_id);
        assert!(invoice.creator == creator, "only invoice creator can create a covenant");
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice must be pending"
        );
        assert!(
            load_covenant(&env, invoice_id).is_none(),
            "covenant already exists for this invoice"
        );

        // Creator must have sufficient unlocked collateral.
        let col = load_collateral(&env, &creator).expect("creator must have collateral to create a covenant");
        let available = col.amount - col.locked;
        assert!(
            available >= penalty_amount,
            "insufficient unlocked collateral for covenant"
        );

        let covenant = CreatorCovenant {
            invoice_id,
            creator: creator.clone(),
            penalty_amount,
            violated: false,
            fulfilled: false,
        };

        save_covenant(&env, &covenant);
        events::covenant_created(&env, invoice_id, &creator, penalty_amount);
    }

    /// Mark a covenant as fulfilled once the invoice is released.
    ///
    /// Can be called by anyone after the invoice reaches Released status.
    pub fn fulfill_covenant(env: Env, invoice_id: u64) {
        let invoice = load_invoice(&env, invoice_id);
        assert!(
            invoice.status == InvoiceStatus::Released,
            "invoice must be released before fulfilling covenant"
        );

        let mut covenant = load_covenant(&env, invoice_id).expect("no covenant found for this invoice");
        assert!(!covenant.violated, "covenant already violated");
        assert!(!covenant.fulfilled, "covenant already fulfilled");

        covenant.fulfilled = true;
        save_covenant(&env, &covenant);
        events::covenant_fulfilled(&env, invoice_id, &covenant.creator.clone());
    }

    /// Get the covenant attached to an invoice.
    pub fn get_covenant(env: Env, invoice_id: u64) -> CreatorCovenant {
        load_covenant(&env, invoice_id).expect("no covenant found for this invoice")
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Route funds to all recipients and mark the invoice as released.
    fn _release(env: &Env, invoice_id: u64, invoice: &mut Invoice) {
        let token_client = token::Client::new(env, &invoice.token);

        for (recipient, amount) in invoice.recipients.iter().zip(invoice.amounts.iter()) {
            token_client.transfer(&env.current_contract_address(), &recipient, &amount);
        }

        invoice.status = InvoiceStatus::Released;
        save_invoice(env, invoice_id, invoice);
        events::invoice_released(env, invoice_id, &invoice.recipients);

        // --- #867: update recipient performance scores ---
        for recipient in invoice.recipients.iter() {
            let mut perf = load_performance(env, &recipient).unwrap_or(RecipientPerformance {
                recipient: recipient.clone(),
                completed: 0,
                flagged: 0,
                score: 0,
            });
            perf.completed += 1;
            perf.score += 1;
            save_performance(env, &perf);
            events::performance_updated(env, &recipient, perf.score);
        }

        // --- #868: auto-fulfill covenant if one exists ---
        if let Some(mut covenant) = load_covenant(env, invoice_id) {
            if !covenant.violated && !covenant.fulfilled {
                covenant.fulfilled = true;
                events::covenant_fulfilled(env, invoice_id, &covenant.creator.clone());
                save_covenant(env, &covenant);
            }
        }
    }
}
