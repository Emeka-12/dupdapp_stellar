#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, BytesN, Env, String};

use crate::{SettlementLedgerContract, SettlementLedgerContractClient};
use fee_calculator::{FeeCalculatorContract, FeeCalculatorContractClient, FeeTier};

fn pid(env: &Env, seed: u8) -> BytesN<32> {
    BytesN::from_array(env, &[seed; 32])
}

fn fiat_ref(env: &Env, s: &str) -> String {
    String::from_str(env, s)
}

fn setup() -> (Env, Address, SettlementLedgerContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register(SettlementLedgerContract, (&admin,));
    let client = SettlementLedgerContractClient::new(&env, &id);
    (env, admin, client)
}

fn record(
    client: &SettlementLedgerContractClient,
    admin: &Address,
    env: &Env,
    seed: u8,
    merchant: &Address,
    amount: i128,
    fee: i128,
) {
    client.record_settlement(
        admin,
        &pid(env, seed),
        merchant,
        &amount,
        &fee,
        &(amount - fee),
        &1_700_000_000u64,
        &fiat_ref(env, "REF-001"),
    );
}

// ── basic record + get ────────────────────────────────────────────────────────

#[test]
fn test_record_and_get() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);

    client.record_settlement(
        &admin,
        &pid(&env, 1),
        &merchant,
        &10_000,
        &200,
        &9_800,
        &1_700_000_000u64,
        &fiat_ref(&env, "TXN-ABC"),
    );

    let r = client.get_settlement(&pid(&env, 1));
    assert_eq!(r.amount, 10_000);
    assert_eq!(r.fee, 200);
    assert_eq!(r.net, 9_800);
    assert_eq!(r.timestamp, 1_700_000_000u64);
}

// ── immutability: duplicate write must panic ──────────────────────────────────

#[test]
fn test_duplicate_record_panics() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    record(&client, &admin, &env, 2, &merchant, 5_000, 100);

    let result = client.try_record_settlement(
        &admin,
        &pid(&env, 2),
        &merchant,
        &5_000,
        &100,
        &4_900,
        &1_700_000_001u64,
        &fiat_ref(&env, "DUP"),
    );
    assert!(result.is_err());
}

// ── fee + net must equal amount ───────────────────────────────────────────────

#[test]
fn test_invariant_fee_plus_net_equals_amount() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);

    let result = client.try_record_settlement(
        &admin,
        &pid(&env, 3),
        &merchant,
        &10_000,
        &300,
        &9_500, // wrong: 300 + 9500 != 10000
        &1_700_000_000u64,
        &fiat_ref(&env, "BAD"),
    );
    assert!(result.is_err());
}

// ── zero fee (fee-free merchant) ──────────────────────────────────────────────

#[test]
fn test_zero_fee() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);

    client.record_settlement(
        &admin,
        &pid(&env, 4),
        &merchant,
        &8_000,
        &0,
        &8_000,
        &1_700_000_000u64,
        &fiat_ref(&env, "FREE"),
    );

    let r = client.get_settlement(&pid(&env, 4));
    assert_eq!(r.fee, 0);
    assert_eq!(r.net, 8_000);
}

// ── pagination ────────────────────────────────────────────────────────────────

#[test]
fn test_list_settlements_pagination() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);

    // Record 25 settlements for the same merchant (seeds 10..34)
    for seed in 10u8..35 {
        record(&client, &admin, &env, seed, &merchant, 1_000, 20);
    }

    assert_eq!(client.settlement_count(&merchant), 25);

    let page0 = client.list_settlements(&merchant, &0);
    assert_eq!(page0.len(), 20); // full page

    let page1 = client.list_settlements(&merchant, &1);
    assert_eq!(page1.len(), 5); // remainder

    let page2 = client.list_settlements(&merchant, &2);
    assert_eq!(page2.len(), 0); // beyond end
}

// ── different merchants are isolated ─────────────────────────────────────────

#[test]
fn test_merchant_isolation() {
    let (env, admin, client) = setup();
    let m1 = Address::generate(&env);
    let m2 = Address::generate(&env);

    record(&client, &admin, &env, 50, &m1, 2_000, 40);
    record(&client, &admin, &env, 51, &m1, 2_000, 40);
    record(&client, &admin, &env, 52, &m2, 3_000, 60);

    assert_eq!(client.settlement_count(&m1), 2);
    assert_eq!(client.settlement_count(&m2), 1);
}

// ── get on unknown payment_id panics ─────────────────────────────────────────

#[test]
fn test_get_unknown_panics() {
    let (env, _admin, client) = setup();
    let result = client.try_get_settlement(&pid(&env, 99));
    assert!(result.is_err());
}

#[test]
fn test_calculated_fee_is_recorded_and_incorrect_fee_is_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    let tiers = soroban_sdk::vec![
        &env,
        FeeTier {
            threshold_usdc: 0,
            fee_bps: 150
        },
        FeeTier {
            threshold_usdc: 1_000,
            fee_bps: 120
        },
        FeeTier {
            threshold_usdc: 5_000,
            fee_bps: 100
        },
    ];

    let fee_calculator_id = env.register(FeeCalculatorContract, (&admin, tiers));
    let fee_calculator = FeeCalculatorContractClient::new(&env, &fee_calculator_id);
    let settlement_ledger_id = env.register(SettlementLedgerContract, (&admin,));
    let settlement_ledger = SettlementLedgerContractClient::new(&env, &settlement_ledger_id);

    let (first_fee, first_net, first_bps) = fee_calculator.calculate_fee(&merchant, &900);
    assert_eq!((first_fee, first_net, first_bps), (13, 887, 150));

    let (fee, net, bps) = fee_calculator.calculate_fee(&merchant, &200);
    assert_eq!((fee, net, bps), (2, 198, 120));

    let invalid_record = settlement_ledger.try_record_settlement(
        &admin,
        &pid(&env, 80),
        &merchant,
        &200,
        &(fee + 1),
        &net,
        &1_700_000_000u64,
        &fiat_ref(&env, "WRONG-FEE"),
    );
    assert!(invalid_record.is_err());

    settlement_ledger.record_settlement(
        &admin,
        &pid(&env, 80),
        &merchant,
        &200,
        &fee,
        &net,
        &1_700_000_000u64,
        &fiat_ref(&env, "TIER-2"),
    );
    let tier_two = settlement_ledger.get_settlement(&pid(&env, 80));
    assert_eq!(tier_two.amount, 200);
    assert_eq!(tier_two.fee, fee);
    assert_eq!(tier_two.net, net);

    let (tier_three_fee, tier_three_net, tier_three_bps) =
        fee_calculator.calculate_fee(&merchant, &4_000);
    assert_eq!(
        (tier_three_fee, tier_three_net, tier_three_bps),
        (40, 3_960, 100)
    );
    settlement_ledger.record_settlement(
        &admin,
        &pid(&env, 81),
        &merchant,
        &4_000,
        &tier_three_fee,
        &tier_three_net,
        &1_700_000_001u64,
        &fiat_ref(&env, "TIER-3"),
    );
    let tier_three = settlement_ledger.get_settlement(&pid(&env, 81));
    assert_eq!(tier_three.fee, tier_three_fee);
    assert_eq!(tier_three.net, tier_three_net);
}
