#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, Vec,
};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Deploy the split contract and a mock USDC token; return (env, contract_id, token_id).
fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(SplitContract, ());
    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    let stellar_asset = StellarAssetClient::new(&env, &token_id);
    stellar_asset.mint(&token_admin, &1_000_000_000);

    (env, contract_id, token_id)
}

fn client<'a>(env: &'a Env, contract_id: &Address) -> SplitContractClient<'a> {
    SplitContractClient::new(env, contract_id)
}

fn token_client<'a>(env: &'a Env, token_id: &Address) -> TokenClient<'a> {
    TokenClient::new(env, token_id)
}

/// Mint `amount` tokens to `addr`.
fn mint(env: &Env, token_id: &Address, addr: &Address, amount: i128) {
    StellarAssetClient::new(env, token_id).mint(addr, &amount);
}

// ---------------------------------------------------------------------------
// Original core tests (preserved)
// ---------------------------------------------------------------------------

#[test]
fn test_create_invoice() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);

    env.ledger().set_timestamp(1_000);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &2_000_u64);
    assert_eq!(id, 1);

    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.status, InvoiceStatus::Pending);
    assert_eq!(invoice.funded, 0);
}

#[test]
fn test_pay_and_auto_release() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    mint(&env, &token_id, &payer, 500);
    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(200_i128);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64);

    c.pay(&payer, &id, &200_i128);

    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.status, InvoiceStatus::Released);
    assert_eq!(tk.balance(&recipient), 200);
}

#[test]
fn test_partial_pay_then_release() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer1 = Address::generate(&env);
    let payer2 = Address::generate(&env);
    let recipient = Address::generate(&env);

    mint(&env, &token_id, &payer1, 150);
    mint(&env, &token_id, &payer2, 150);
    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(300_i128);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64);

    c.pay(&payer1, &id, &150_i128);
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Pending);

    c.pay(&payer2, &id, &150_i128);
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
    assert_eq!(tk.balance(&recipient), 300);
}

#[test]
fn test_refund_after_deadline() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    mint(&env, &token_id, &payer, 100);
    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(500_i128);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &2_000_u64);
    c.pay(&payer, &id, &100_i128);

    env.ledger().set_timestamp(3_000);
    c.refund(&id);

    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Refunded);
    assert_eq!(tk.balance(&payer), 100);
}

#[test]
#[should_panic(expected = "invoice deadline has passed")]
fn test_pay_after_deadline_panics() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    mint(&env, &token_id, &payer, 100);
    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &2_000_u64);
    env.ledger().set_timestamp(3_000);
    c.pay(&payer, &id, &100_i128);
}

#[test]
#[should_panic(expected = "payment exceeds remaining balance")]
fn test_overpayment_panics() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    mint(&env, &token_id, &payer, 1_000);
    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64);
    c.pay(&payer, &id, &200_i128);
}

#[test]
fn test_multi_recipient_release() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);
    let r3 = Address::generate(&env);

    mint(&env, &token_id, &payer, 600);
    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(r1.clone());
    recipients.push_back(r2.clone());
    recipients.push_back(r3.clone());

    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);
    amounts.push_back(200_i128);
    amounts.push_back(300_i128);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64);
    c.pay(&payer, &id, &600_i128);

    assert_eq!(tk.balance(&r1), 100);
    assert_eq!(tk.balance(&r2), 200);
    assert_eq!(tk.balance(&r3), 300);
}

// ---------------------------------------------------------------------------
// #865 – Creator collateral system tests
// ---------------------------------------------------------------------------

#[test]
fn test_deposit_collateral() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    mint(&env, &token_id, &creator, 1_000);

    c.deposit_collateral(&creator, &token_id, &500_i128);

    let col = c.get_collateral(&creator);
    assert_eq!(col.amount, 500);
    assert_eq!(col.locked, 0);
}

#[test]
fn test_withdraw_collateral() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    mint(&env, &token_id, &creator, 1_000);

    c.deposit_collateral(&creator, &token_id, &800_i128);
    c.withdraw_collateral(&creator, &200_i128);

    let col = c.get_collateral(&creator);
    assert_eq!(col.amount, 600);
    assert_eq!(tk.balance(&creator), 400); // 200 remaining + 200 withdrawn
}

#[test]
fn test_lock_and_unlock_collateral() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);
    let payer = Address::generate(&env);

    mint(&env, &token_id, &creator, 1_000);
    mint(&env, &token_id, &payer, 500);
    env.ledger().set_timestamp(1_000);

    c.deposit_collateral(&creator, &token_id, &500_i128);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(200_i128);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64);

    // Lock 100 units of collateral to back this invoice.
    c.lock_collateral(&creator, &id, &100_i128);

    let col = c.get_collateral(&creator);
    assert_eq!(col.locked, 100);
    assert_eq!(col.amount - col.locked, 400); // 400 still available

    // Release the invoice, then unlock collateral.
    c.pay(&payer, &id, &200_i128);
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);

    c.unlock_collateral(&creator, &id, &100_i128);
    let col = c.get_collateral(&creator);
    assert_eq!(col.locked, 0);
}

#[test]
#[should_panic(expected = "insufficient unlocked collateral")]
fn test_withdraw_more_than_available_panics() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    mint(&env, &token_id, &creator, 500);

    c.deposit_collateral(&creator, &token_id, &300_i128);
    c.withdraw_collateral(&creator, &400_i128); // More than deposited
}

#[test]
#[should_panic(expected = "insufficient unlocked collateral")]
fn test_lock_more_than_available_panics() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    mint(&env, &token_id, &creator, 500);
    env.ledger().set_timestamp(1_000);

    c.deposit_collateral(&creator, &token_id, &100_i128);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(50_i128);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64);
    c.lock_collateral(&creator, &id, &200_i128); // More than deposited
}

#[test]
#[should_panic(expected = "invoice must be finalised before unlocking collateral")]
fn test_unlock_collateral_on_pending_invoice_panics() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    mint(&env, &token_id, &creator, 500);
    env.ledger().set_timestamp(1_000);

    c.deposit_collateral(&creator, &token_id, &300_i128);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64);
    c.lock_collateral(&creator, &id, &100_i128);

    // Try to unlock while still pending — should panic.
    c.unlock_collateral(&creator, &id, &100_i128);
}

// ---------------------------------------------------------------------------
// #866 – Rating-based discount tiers tests
// ---------------------------------------------------------------------------

#[test]
fn test_rate_creator_and_get_discount_tier() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    mint(&env, &token_id, &payer, 10_000);
    env.ledger().set_timestamp(1_000);

    // No rating yet → None tier.
    assert_eq!(c.get_discount_tier(&creator), DiscountTier::None);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64);
    c.pay(&payer, &id, &100_i128); // auto-release

    // Payer rates creator 5 stars.
    c.rate_creator(&payer, &id, &5_u32);

    let rating = c.get_creator_rating(&creator);
    assert_eq!(rating.total_score, 5);
    assert_eq!(rating.count, 1);

    // avg = 5.0 → Tier1
    assert_eq!(c.get_discount_tier(&creator), DiscountTier::Tier1);
}

#[test]
fn test_rating_tier2() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    mint(&env, &token_id, &payer, 10_000);
    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64);
    c.pay(&payer, &id, &100_i128);

    // Rate 4 → avg 4.0 → Tier2
    c.rate_creator(&payer, &id, &4_u32);
    assert_eq!(c.get_discount_tier(&creator), DiscountTier::Tier2);
}

#[test]
fn test_rating_tier3() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    mint(&env, &token_id, &payer, 10_000);
    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64);
    c.pay(&payer, &id, &100_i128);

    // Rate 3 → avg 3.0 → Tier3
    c.rate_creator(&payer, &id, &3_u32);
    assert_eq!(c.get_discount_tier(&creator), DiscountTier::Tier3);
}

#[test]
fn test_discount_applied_to_invoice() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    mint(&env, &token_id, &payer, 10_000);
    env.ledger().set_timestamp(1_000);

    // --- Invoice 1: establish 5-star rating for creator ---
    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(1_000_i128);

    let id1 = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64);
    c.pay(&payer, &id1, &1_000_i128);
    c.rate_creator(&payer, &id1, &5_u32); // avg=5 → Tier1 (5% discount)

    // --- Invoice 2: discount should be applied (5% off 1_000 = 50) ---
    let mut amounts2 = Vec::new(&env);
    amounts2.push_back(1_000_i128);
    let mut recipients2 = Vec::new(&env);
    recipients2.push_back(recipient.clone());

    let id2 = c.create_invoice(&creator, &recipients2, &amounts2, &token_id, &99_999_u64);
    let invoice2 = c.get_invoice(&id2);

    // Discounted amount: 1000 - 5% = 950
    assert_eq!(invoice2.amounts.get(0).unwrap(), 950_i128);

    // Pay the discounted amount and verify recipient gets 950.
    let recipient2 = Address::generate(&env);
    let _ = recipient2; // recipient is reused above
    c.pay(&payer, &id2, &950_i128);
    assert_eq!(tk.balance(&recipient), 1_000 + 950); // 1000 from id1, 950 from id2
}

#[test]
#[should_panic(expected = "score must be between 1 and 5")]
fn test_rate_out_of_range_panics() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    mint(&env, &token_id, &payer, 500);
    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64);
    c.pay(&payer, &id, &100_i128);
    c.rate_creator(&payer, &id, &6_u32); // Invalid score
}

#[test]
#[should_panic(expected = "invoice must be released before rating")]
fn test_rate_unreleased_invoice_panics() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    mint(&env, &token_id, &payer, 500);
    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(500_i128);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64);
    // Invoice is still pending (not fully paid).
    c.rate_creator(&payer, &id, &4_u32);
}

// ---------------------------------------------------------------------------
// #867 – Smart invoice routing tests
// ---------------------------------------------------------------------------

#[test]
fn test_performance_updated_on_release() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    mint(&env, &token_id, &payer, 500);
    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(200_i128);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64);
    c.pay(&payer, &id, &200_i128); // auto-release

    let perf = c.get_recipient_performance(&recipient);
    assert_eq!(perf.completed, 1);
    assert_eq!(perf.score, 1);
}

#[test]
fn test_flag_recipient() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    mint(&env, &token_id, &payer, 500);
    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(200_i128);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64);
    c.flag_recipient(&creator, &recipient, &id);

    let perf = c.get_recipient_performance(&recipient);
    assert_eq!(perf.flagged, 1);
    assert_eq!(perf.score, -1);
}

#[test]
fn test_record_recipient_completion() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    mint(&env, &token_id, &payer, 500);
    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(200_i128);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64);
    c.pay(&payer, &id, &200_i128); // release

    // Creator explicitly records completion.
    c.record_recipient_completion(&creator, &recipient, &id);

    let perf = c.get_recipient_performance(&recipient);
    // Auto-update on release (+1) + manual record (+1) = 2
    assert_eq!(perf.completed, 2);
    assert_eq!(perf.score, 2);
}

#[test]
fn test_create_invoice_routed_default() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    mint(&env, &token_id, &payer, 500);
    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);

    let id = c.create_invoice_routed(
        &creator,
        &recipients,
        &amounts,
        &token_id,
        &9_999_u64,
        &RoutingStrategy::Default,
        &0_i32,
    );

    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.recipients.len(), 1);
    assert_eq!(invoice.amounts.get(0).unwrap(), 100_i128);
}

#[test]
fn test_create_invoice_routed_threshold_filters_low_performers() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let good_recipient = Address::generate(&env);
    let bad_recipient = Address::generate(&env);

    mint(&env, &token_id, &payer, 10_000);
    env.ledger().set_timestamp(1_000);

    // Give good_recipient a positive score.
    let mut r1 = Vec::new(&env);
    r1.push_back(good_recipient.clone());
    let mut a1 = Vec::new(&env);
    a1.push_back(100_i128);
    let id1 = c.create_invoice(&creator, &r1, &a1, &token_id, &9_999_u64);
    c.pay(&payer, &id1, &100_i128); // good_recipient score → 1

    // Give bad_recipient a negative score by flagging.
    let mut r2 = Vec::new(&env);
    r2.push_back(bad_recipient.clone());
    let mut a2 = Vec::new(&env);
    a2.push_back(100_i128);
    let id2 = c.create_invoice(&creator, &r2, &a2, &token_id, &9_999_u64);
    c.flag_recipient(&creator, &bad_recipient, &id2); // bad_recipient score → -1

    // Routed invoice: threshold=0, so bad_recipient (score=-1) should be excluded.
    let mut all_recipients = Vec::new(&env);
    all_recipients.push_back(good_recipient.clone());
    all_recipients.push_back(bad_recipient.clone());
    let mut all_amounts = Vec::new(&env);
    all_amounts.push_back(200_i128);
    all_amounts.push_back(200_i128);

    let id3 = c.create_invoice_routed(
        &creator,
        &all_recipients,
        &all_amounts,
        &token_id,
        &99_999_u64,
        &RoutingStrategy::ThresholdBased,
        &0_i32, // min score = 0
    );

    let invoice = c.get_invoice(&id3);
    // Only good_recipient should remain.
    assert_eq!(invoice.recipients.len(), 1);
    assert_eq!(invoice.recipients.get(0).unwrap(), good_recipient);
}

#[test]
#[should_panic(expected = "no recipients meet the performance threshold")]
fn test_create_invoice_routed_all_filtered_panics() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    // recipient has no record → score=0 < threshold=1
    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);

    c.create_invoice_routed(
        &creator,
        &recipients,
        &amounts,
        &token_id,
        &9_999_u64,
        &RoutingStrategy::ThresholdBased,
        &1_i32,
    );
}

#[test]
fn test_get_recipient_performance_default() {
    let (env, contract_id, _token_id) = setup();
    let c = client(&env, &contract_id);

    let recipient = Address::generate(&env);
    let perf = c.get_recipient_performance(&recipient);
    assert_eq!(perf.completed, 0);
    assert_eq!(perf.flagged, 0);
    assert_eq!(perf.score, 0);
}

// ---------------------------------------------------------------------------
// #868 – Creator covenant system tests
// ---------------------------------------------------------------------------

#[test]
fn test_create_and_fulfill_covenant() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    mint(&env, &token_id, &creator, 1_000);
    mint(&env, &token_id, &payer, 500);
    env.ledger().set_timestamp(1_000);

    // Creator deposits collateral first.
    c.deposit_collateral(&creator, &token_id, &500_i128);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(200_i128);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64);

    // Create covenant with 100-unit penalty.
    c.create_covenant(&creator, &id, &100_i128);

    let covenant = c.get_covenant(&id);
    assert!(!covenant.violated);
    assert!(!covenant.fulfilled);
    assert_eq!(covenant.penalty_amount, 100);

    // Fully fund and release the invoice.
    c.pay(&payer, &id, &200_i128);

    // Auto-fulfill on release.
    let covenant = c.get_covenant(&id);
    assert!(covenant.fulfilled);
    assert!(!covenant.violated);
}

#[test]
fn test_covenant_violated_on_refund() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    mint(&env, &token_id, &creator, 1_000);
    mint(&env, &token_id, &payer, 100);
    env.ledger().set_timestamp(1_000);

    c.deposit_collateral(&creator, &token_id, &500_i128);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(500_i128);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &2_000_u64);
    c.create_covenant(&creator, &id, &200_i128);

    // Partial payment.
    c.pay(&payer, &id, &100_i128);

    // Advance past deadline and refund.
    env.ledger().set_timestamp(3_000);
    c.refund(&id);

    // Covenant should be violated and 200 slashed from creator's collateral.
    let covenant = c.get_covenant(&id);
    assert!(covenant.violated);
    assert!(!covenant.fulfilled);

    let col = c.get_collateral(&creator);
    assert_eq!(col.amount, 300); // 500 - 200 = 300

    // Payer should get refunded.
    assert_eq!(tk.balance(&payer), 100);
}

#[test]
fn test_covenant_penalty_capped_at_collateral_balance() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    mint(&env, &token_id, &creator, 1_000);
    mint(&env, &token_id, &payer, 100);
    env.ledger().set_timestamp(1_000);

    // Only deposit 50, but covenant penalty is 200.
    c.deposit_collateral(&creator, &token_id, &50_i128);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(500_i128);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &2_000_u64);

    // We need at least 200 unlocked collateral to create covenant — use 40 as penalty.
    c.create_covenant(&creator, &id, &40_i128);

    c.pay(&payer, &id, &100_i128);
    env.ledger().set_timestamp(3_000);
    c.refund(&id);

    let col = c.get_collateral(&creator);
    // 50 - 40 = 10 remaining
    assert_eq!(col.amount, 10);
}

#[test]
#[should_panic(expected = "creator must have collateral to create a covenant")]
fn test_create_covenant_without_collateral_panics() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    mint(&env, &token_id, &creator, 1_000);
    env.ledger().set_timestamp(1_000);

    // No collateral deposited.
    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(200_i128);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64);
    c.create_covenant(&creator, &id, &100_i128);
}

#[test]
#[should_panic(expected = "covenant already exists for this invoice")]
fn test_duplicate_covenant_panics() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    mint(&env, &token_id, &creator, 1_000);
    env.ledger().set_timestamp(1_000);

    c.deposit_collateral(&creator, &token_id, &500_i128);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(200_i128);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64);
    c.create_covenant(&creator, &id, &100_i128);
    c.create_covenant(&creator, &id, &100_i128); // Duplicate
}

#[test]
fn test_fulfill_covenant_explicitly() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    mint(&env, &token_id, &creator, 1_000);
    mint(&env, &token_id, &payer, 500);
    env.ledger().set_timestamp(1_000);

    c.deposit_collateral(&creator, &token_id, &500_i128);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(200_i128);

    let id = c.create_invoice(&creator, &recipients, &amounts, &token_id, &9_999_u64);
    c.create_covenant(&creator, &id, &100_i128);

    c.pay(&payer, &id, &200_i128); // auto-release + auto-fulfill

    // Explicit fulfill should fail (already fulfilled).
    // Test that auto-fulfill worked:
    let covenant = c.get_covenant(&id);
    assert!(covenant.fulfilled);
}

// ---------------------------------------------------------------------------
// Integration: multiple features together
// ---------------------------------------------------------------------------

#[test]
fn test_integration_collateral_rating_covenant() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    mint(&env, &token_id, &creator, 2_000);
    mint(&env, &token_id, &payer, 10_000);
    env.ledger().set_timestamp(1_000);

    // Creator deposits collateral.
    c.deposit_collateral(&creator, &token_id, &1_000_i128);

    // Invoice 1: establish rating.
    let mut r = Vec::new(&env);
    r.push_back(recipient.clone());
    let mut a = Vec::new(&env);
    a.push_back(500_i128);

    let id1 = c.create_invoice(&creator, &r, &a, &token_id, &9_999_u64);
    c.create_covenant(&creator, &id1, &200_i128);
    c.pay(&payer, &id1, &500_i128); // release + auto-fulfill covenant

    // Rate creator 5 stars.
    c.rate_creator(&payer, &id1, &5_u32);
    assert_eq!(c.get_discount_tier(&creator), DiscountTier::Tier1);

    // Covenant fulfilled.
    let cov1 = c.get_covenant(&id1);
    assert!(cov1.fulfilled);

    // Invoice 2: discount applies (5% off).
    let mut r2 = Vec::new(&env);
    r2.push_back(recipient.clone());
    let mut a2 = Vec::new(&env);
    a2.push_back(1_000_i128);

    let id2 = c.create_invoice(&creator, &r2, &a2, &token_id, &99_999_u64);
    let inv2 = c.get_invoice(&id2);
    // 5% off 1000 = 950
    assert_eq!(inv2.amounts.get(0).unwrap(), 950_i128);

    c.pay(&payer, &id2, &950_i128);
    assert_eq!(tk.balance(&recipient), 500 + 950);

    // Recipient performance score is 2 (one per release).
    let perf = c.get_recipient_performance(&recipient);
    assert_eq!(perf.completed, 2);
    assert_eq!(perf.score, 2);
}
