#![cfg(test)]

use crate::{FeeCalculatorContract, FeeCalculatorContractClient, FeeTier};
use soroban_sdk::{testutils::{Address as _, Ledger}, vec, Address, Env};

fn setup_env() -> (Env, FeeCalculatorContractClient<'static>, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|li| {
        li.min_temp_entry_ttl = 300_000;
        li.min_persistent_entry_ttl = 300_000;
        li.max_entry_ttl = 300_000;
    });

    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    let settlement = Address::generate(&env);
    let tiers = vec![
        &env,
        FeeTier {
            threshold_usdc: 0,
            fee_bps: 150,
        },
        FeeTier {
            threshold_usdc: 1_000,
            fee_bps: 120,
        },
        FeeTier {
            threshold_usdc: 10_000,
            fee_bps: 100,
        },
    ];

    let contract_id = env.register(FeeCalculatorContract, (&admin, tiers));
    let client = FeeCalculatorContractClient::new(&env, &contract_id);
    client.set_settlement_caller(&admin, &settlement);

    (env, client, admin, merchant, settlement)
}

#[test]
fn test_fee_rate_drops_when_volume_crosses_tier_threshold() {
    let (_env, client, _admin, merchant, settlement) = setup_env();

    let (_, _, bps_before) = client.calculate_fee(&settlement, &merchant, &900);
    assert_eq!(bps_before, 150);

    let (_, _, bps_after) = client.calculate_fee(&settlement, &merchant, &100);
    assert_eq!(bps_after, 120);
}

#[test]
fn test_highest_tier_applies_at_exact_boundary() {
    let (_env, client, _admin, merchant, settlement) = setup_env();

    client.calculate_fee(&settlement, &merchant, &9_999);
    let (_, _, bps) = client.calculate_fee(&settlement, &merchant, &1);
    assert_eq!(bps, 100);
}

#[test]
fn test_volume_resets_after_30_days_by_ledger_count() {
    let (env, client, _admin, merchant, settlement) = setup_env();

    client.calculate_fee(&settlement, &merchant, &2_000);
    let (_, _, bps_before_reset) = client.calculate_fee(&settlement, &merchant, &1);
    assert_eq!(bps_before_reset, 120);

    env.ledger().with_mut(|li| li.sequence_number += 172_800);

    let (_, _, bps_after_reset) = client.calculate_fee(&settlement, &merchant, &100);
    assert_eq!(bps_after_reset, 150);
}

#[test]
fn test_admin_can_update_fee_tiers() {
    let (env, client, admin, _merchant, _settlement) = setup_env();

    let new_tiers = vec![
        &env,
        FeeTier {
            threshold_usdc: 0,
            fee_bps: 200,
        },
        FeeTier {
            threshold_usdc: 5_000,
            fee_bps: 80,
        },
    ];

    client.set_fee_tiers(&admin, &new_tiers);
    let stored = client.get_fee_tiers();
    assert_eq!(stored, new_tiers);
}

#[test]
#[should_panic(expected = "Not admin")]
fn test_non_admin_cannot_update_fee_tiers() {
    let (env, client, _admin, _merchant, _settlement) = setup_env();
    let random = Address::generate(&env);

    let new_tiers = vec![
        &env,
        FeeTier {
            threshold_usdc: 0,
            fee_bps: 100,
        },
    ];

    client.set_fee_tiers(&random, &new_tiers);
}

#[test]
fn test_admin_can_set_settlement_caller() {
    let (env, client, admin, _merchant, _settlement) = setup_env();
    let new_settlement = Address::generate(&env);
    client.set_settlement_caller(&admin, &new_settlement);
    // Confirm the new settlement caller is accepted by calculate_fee
    let merchant = Address::generate(&env);
    let (fee, net, bps) = client.calculate_fee(&new_settlement, &merchant, &500);
    assert_eq!(bps, 150);
    assert!(fee > 0);
    assert!(net > 0);
}

#[test]
#[should_panic(expected = "caller is not the authorized settlement contract")]
fn test_non_settlement_caller_cannot_calculate_fee() {
    let (env, client, _admin, merchant, _settlement) = setup_env();
    let random = Address::generate(&env);
    client.calculate_fee(&random, &merchant, &500);
}
