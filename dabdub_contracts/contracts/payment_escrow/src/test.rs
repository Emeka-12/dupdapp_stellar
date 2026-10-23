#![cfg(test)]

use crate::{AssetType, PaymentEscrowContract, PaymentEscrowContractClient, PaymentStatus};
use merchant_registry::{MerchantRegistryContract, MerchantRegistryContractClient};
use soroban_sdk::{
    testutils::Address as _, testutils::Ledger, token, Address, BytesN, Env, String,
};

const DEFAULT_PAYMENT_TTL: u32 = 100;
const EMERGENCY_COOLDOWN_LEDGERS: u32 = 50;

type Setup = (
    Env,
    PaymentEscrowContractClient<'static>,
    Address,
    Address,
    Address,
    Address,
    Address,
    Address,
    Address,
    Address,
    Address,
    Address,
);

fn deploy_escrow(
    env: &Env,
    admin: &Address,
    xlm: &Address,
    usdc: &Address,
    registry: Option<Address>,
    emergency_signers: &soroban_sdk::Vec<Address>,
    emergency_treasury: &Address,
) -> Address {
    env.register(
        PaymentEscrowContract,
        (
            admin,
            xlm,
            usdc,
            &DEFAULT_PAYMENT_TTL,
            &registry,
            emergency_signers,
            emergency_treasury,
            &EMERGENCY_COOLDOWN_LEDGERS,
        ),
    )
}

fn setup_env() -> Setup {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_sequence_number(10);

    let admin = Address::generate(&env);
    let customer = Address::generate(&env);
    let merchant = Address::generate(&env);
    let token_admin = Address::generate(&env);

    let usdc = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let xlm = env.register_stellar_asset_contract_v2(token_admin).address();

    let emergency_signer_one = Address::generate(&env);
    let emergency_signer_two = Address::generate(&env);
    let emergency_signer_three = Address::generate(&env);
    let emergency_treasury = Address::generate(&env);
    let emergency_signers = soroban_sdk::vec![
        &env,
        emergency_signer_one.clone(),
        emergency_signer_two.clone(),
        emergency_signer_three.clone(),
    ];

    let contract_id = deploy_escrow(
        &env,
        &admin,
        &xlm,
        &usdc,
        None,
        &emergency_signers,
        &emergency_treasury,
    );
    let client = PaymentEscrowContractClient::new(&env, &contract_id);

    token::StellarAssetClient::new(&env, &usdc).mint(&customer, &1_000_000_000i128);

    (
        env,
        client,
        contract_id,
        admin,
        customer,
        merchant,
        usdc,
        xlm,
        emergency_signer_one,
        emergency_signer_two,
        emergency_signer_three,
        emergency_treasury,
    )
}

fn setup_with_registry() -> (
    Env,
    PaymentEscrowContractClient<'static>,
    MerchantRegistryContractClient<'static>,
    Address,
    Address,
    Address,
    Address,
    Address,
    Address,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_sequence_number(10);

    let admin = Address::generate(&env);
    let customer = Address::generate(&env);
    let merchant = Address::generate(&env);
    let token_admin = Address::generate(&env);

    let usdc = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let xlm = env.register_stellar_asset_contract_v2(token_admin).address();

    let emergency_signer_one = Address::generate(&env);
    let emergency_signer_two = Address::generate(&env);
    let emergency_signer_three = Address::generate(&env);
    let emergency_treasury = Address::generate(&env);
    let emergency_signers = soroban_sdk::vec![
        &env,
        emergency_signer_one.clone(),
        emergency_signer_two.clone(),
        emergency_signer_three.clone(),
    ];

    let registry_id = env.register(MerchantRegistryContract, (&admin,));
    let registry = MerchantRegistryContractClient::new(&env, &registry_id);

    let escrow_id = deploy_escrow(
        &env,
        &admin,
        &xlm,
        &usdc,
        Some(registry_id.clone()),
        &emergency_signers,
        &emergency_treasury,
    );
    let escrow = PaymentEscrowContractClient::new(&env, &escrow_id);

    token::StellarAssetClient::new(&env, &usdc).mint(&customer, &1_000_000_000i128);

    (
        env,
        escrow,
        registry,
        admin,
        customer,
        merchant,
        usdc,
        emergency_signer_one,
        emergency_signer_two,
        emergency_signer_three,
        emergency_treasury,
    )
}

fn make_id(env: &Env, seed: u8) -> BytesN<32> {
    BytesN::from_array(env, &[seed; 32])
}

fn deposit_default_ttl(
    client: &PaymentEscrowContractClient<'_>,
    customer: &Address,
    payment_id: &BytesN<32>,
    merchant: &Address,
    amount: i128,
) {
    client.deposit(
        customer,
        payment_id,
        merchant,
        &amount,
        &DEFAULT_PAYMENT_TTL,
        &AssetType::Usdc,
    );
}

#[test]
fn test_constructor() {
    let (env, client, _contract_id, admin, _customer, _merchant, usdc, xlm, ..) = setup_env();

    assert_eq!(client.get_admin(), admin);
    assert_eq!(client.get_usdc_token(), usdc);
    assert_eq!(client.get_xlm_token(), xlm);
    assert_eq!(client.get_default_ttl_ledgers(), DEFAULT_PAYMENT_TTL);
    assert_eq!(client.get_version(), 1);
    assert_eq!(client.get_registry(), None);
}

#[test]
fn test_deposit_happy_path() {
    let (env, client, contract_id, _admin, customer, merchant, usdc, ..) = setup_env();
    let payment_id = make_id(&env, 1);

    client.deposit(
        &customer,
        &payment_id,
        &merchant,
        &250_000_000i128,
        &DEFAULT_PAYMENT_TTL,
        &AssetType::Usdc,
    );

    let payment = client.get_payment(&payment_id);
    assert_eq!(payment.amount, 250_000_000);
    assert_eq!(payment.released_amount, 0);
    assert_eq!(payment.customer, customer);
    assert_eq!(payment.merchant, merchant);
    assert_eq!(payment.status, PaymentStatus::Pending);
    assert_eq!(payment.expiry, 110);
    assert_eq!(payment.dispute_window_end, 110);
    assert_eq!(payment.dispute_reason, None);
    assert_eq!(payment.asset_type, AssetType::Usdc);
    assert_eq!(client.get_balance(&payment_id), 250_000_000);
    assert_eq!(client.get_expiry(&payment_id), 110);

    let token_client = token::Client::new(&env, &usdc);
    assert_eq!(token_client.balance(&customer), 750_000_000);
    assert_eq!(token_client.balance(&contract_id), 250_000_000);
}

#[test]
#[should_panic(expected = "Payment ID already exists")]
fn test_deposit_duplicate_payment_id() {
    let (env, client, _contract_id, _admin, customer, merchant, _usdc, ..) = setup_env();
    let payment_id = make_id(&env, 1);

    deposit_default_ttl(&client, &customer, &payment_id, &merchant, 250_000_000i128);
    deposit_default_ttl(&client, &customer, &payment_id, &merchant, 250_000_000i128);
}

#[test]
#[should_panic(expected = "Amount must be > 0")]
fn test_deposit_zero_amount() {
    let (env, client, _contract_id, _admin, customer, merchant, _usdc, ..) = setup_env();
    client.deposit(
        &customer,
        &make_id(&env, 1),
        &merchant,
        &0i128,
        &DEFAULT_PAYMENT_TTL,
        &AssetType::Usdc,
    );
}

#[test]
#[should_panic(expected = "TTL must be > 0")]
fn test_deposit_zero_ttl() {
    let (env, client, _contract_id, _admin, customer, merchant, _usdc, ..) = setup_env();
    client.deposit(
        &customer,
        &make_id(&env, 1),
        &merchant,
        &250_000_000i128,
        &0u32,
        &AssetType::Usdc,
    );
}

#[test]
#[should_panic(expected = "TTL exceeds maximum")]
fn test_deposit_excessive_ttl() {
    let (env, client, _contract_id, _admin, customer, merchant, _usdc, ..) = setup_env();
    let ttl = client.get_max_ttl_ledgers() + 1;
    client.deposit(
        &customer,
        &make_id(&env, 1),
        &merchant,
        &250_000_000i128,
        &ttl,
        &AssetType::Usdc,
    );
}

// ---------------------------------------------------------------------------
// Fuzz-style boundary tests for i128 deposit / release arithmetic (#1252)
// ---------------------------------------------------------------------------

/// Deterministic pseudo-random generator so the fuzz cases are reproducible
/// without pulling in an external fuzzing dependency.
struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        Lcg(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }

    /// Returns a value in `[0, max]` (inclusive).
    fn next_in_range(&mut self, max: u64) -> u64 {
        if max == 0 {
            return 0;
        }
        self.next_u64() % (max + 1)
    }
}

/// Deposit with an amount near `i128::MAX` must not panic on arithmetic; it
/// either succeeds (when the token can fund it) or returns a clean error.
#[test]
fn fuzz_deposit_near_i128_max() {
    let (env, client, _contract_id, _admin, customer, merchant, usdc, ..) = setup_env();
    let token_admin = token::StellarAssetClient::new(&env, &usdc);

    let mut rng = Lcg::new(0x1252_0001);
    let candidates = [
        i128::MAX,
        i128::MAX - 1,
        i128::MAX / 2,
        i128::MAX / 2 + 1,
        i128::MAX - 1_000_000,
    ];

    for (i, amount) in candidates.iter().enumerate() {
        // Fund the customer with exactly the amount under test so the token
        // transfer itself cannot be the source of an overflow.
        token_admin.mint(&customer, amount);

        let payment_id = make_id(&env, (i as u8) + 10);
        let result = client.try_deposit(
            &customer,
            &payment_id,
            &merchant,
            amount,
            &DEFAULT_PAYMENT_TTL,
            &AssetType::Usdc,
        );

        // Must not panic: either a clean Ok or a clean Err is acceptable.
        match result {
            Ok(Ok(())) => {
                let payment = client.get_payment(&payment_id);
                assert_eq!(payment.amount, *amount);
                assert_eq!(payment.released_amount, 0);
                assert_eq!(client.get_balance(&payment_id), *amount);
            }
            Ok(Err(_)) | Err(_) => {
                // Rejected cleanly without panicking.
            }
        }

        // Exercise a random near-max amount as well.
        let jitter = rng.next_in_range(1_000_000) as i128;
        let random_amount = i128::MAX - jitter;
        token_admin.mint(&customer, &random_amount);
        let random_id = make_id(&env, (i as u8) + 40);
        let _ = client.try_deposit(
            &customer,
            &random_id,
            &merchant,
            &random_amount,
            &DEFAULT_PAYMENT_TTL,
            &AssetType::Usdc,
        );
    }
}

/// Negative and zero deposit amounts must be rejected with an error, never
/// panic or wrap around.
#[test]
fn fuzz_deposit_non_positive_amounts() {
    let (env, client, _contract_id, _admin, customer, merchant, _usdc, ..) = setup_env();

    let mut rng = Lcg::new(0x1252_0002);
    let mut amounts: soroban_sdk::Vec<i128> = soroban_sdk::vec![&env, 0i128, -1i128, i128::MIN, i128::MIN + 1];
    for _ in 0..8 {
        let raw = rng.next_u64() as i128;
        amounts.push_back(-raw - 1);
    }

    for (i, amount) in amounts.iter().enumerate() {
        let payment_id = make_id(&env, (i as u8) + 60);
        let result = client.try_deposit(
            &customer,
            &payment_id,
            &merchant,
            &amount,
            &DEFAULT_PAYMENT_TTL,
            &AssetType::Usdc,
        );
        assert!(
            result.is_err() || matches!(result, Ok(Err(_))),
            "non-positive amount {amount} must be rejected"
        );
    }
}

/// Partial releases that sum to `i128::MAX` must succeed without overflow and
/// leave a zero balance.
#[test]
fn fuzz_partial_releases_sum_to_i128_max() {
    let (env, client, _contract_id, _admin, customer, merchant, usdc, ..) = setup_env();
    let token_admin = token::StellarAssetClient::new(&env, &usdc);

    let total = i128::MAX;
    token_admin.mint(&customer, &total);

    let payment_id = make_id(&env, 200);
    client.deposit(
        &customer,
        &payment_id,
        &merchant,
        &total,
        &DEFAULT_PAYMENT_TTL,
        &AssetType::Usdc,
    );

    // Split `total` into a handful of partial releases that sum exactly to it.
    let mut rng = Lcg::new(0x1252_0003);
    let parts = 5u32;
    let mut remaining = total;
    for i in 0..parts {
        let amount = if i == parts - 1 {
            remaining
        } else {
            let cap = (remaining / (parts - i) as i128).max(1);
            let pick = (rng.next_in_range(cap as u64) as i128).max(1).min(remaining);
            pick
        };
        remaining -= amount;

        let result = client.try_release(&payment_id, &amount);
        assert!(result.is_ok(), "partial release {i} must succeed");
    }

    assert_eq!(remaining, 0);
    let payment = client.get_payment(&payment_id);
    assert_eq!(payment.released_amount, total);
    assert_eq!(client.get_balance(&payment_id), 0);
    assert_eq!(payment.status, PaymentStatus::Released);
}

/// Multiple partial releases that would push `released_amount` past
/// `i128::MAX` must be rejected cleanly rather than overflow.
#[test]
fn fuzz_released_amount_overflow_from_partial_releases() {
    let (env, client, _contract_id, _admin, customer, merchant, usdc, ..) = setup_env();
    let token_admin = token::StellarAssetClient::new(&env, &usdc);

    let total = i128::MAX;
    token_admin.mint(&customer, &total);

    let payment_id = make_id(&env, 201);
    client.deposit(
        &customer,
        &payment_id,
        &merchant,
        &total,
        &DEFAULT_PAYMENT_TTL,
        &AssetType::Usdc,
    );

    // Release a large chunk, then attempt to release more than remains.
    let first = total / 2;
    client.release(&payment_id, &first);

    let remaining = total - first;
    // Attempting to release `remaining + 1` must be rejected, not overflow.
    let overflow_attempt = remaining + 1;
    let result = client.try_release(&payment_id, &overflow_attempt);
    assert!(
        result.is_err() || matches!(result, Ok(Err(_))),
        "release exceeding balance must be rejected"
    );

    // The released amount must be unchanged after the rejected attempt.
    let payment = client.get_payment(&payment_id);
    assert_eq!(payment.released_amount, first);
    assert_eq!(client.get_balance(&payment_id), remaining);

    // Releasing exactly the remaining balance must succeed and zero out.
    client.release(&payment_id, &remaining);
    let payment = client.get_payment(&payment_id);
    assert_eq!(payment.released_amount, total);
    assert_eq!(client.get_balance(&payment_id), 0);
}

#[test]
fn test_release_happy_path() {
    let (env, client, contract_id, admin, customer, merchant, usdc, ..) = setup_env();
    let p