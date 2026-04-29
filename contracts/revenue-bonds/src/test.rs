#![cfg(test)]
use super::*;
use soroban_sdk::{testutils::Address as _, token, Address, Env, String};

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn create_token_contract<'a>(env: &Env, admin: &Address) -> token::StellarAssetClient<'a> {
    token::StellarAssetClient::new(
        env,
        &env.register_stellar_asset_contract_v2(admin.clone()).address(),
    )
}

fn setup_test() -> (Env, Address, Address, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.mock_all_auths_allowing_non_root_auth();

    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let owner = Address::generate(&env);
    let token_admin = Address::generate(&env);

    let token_client = create_token_contract(&env, &token_admin);
    let token = token_client.address.clone();
    token_client.mint(&issuer, &100_000_000);

    let attestation_contract = Address::generate(&env);
    (env, admin, issuer, owner, token, attestation_contract)
}

/// Set the mock revenue for a (business, period) pair in the test environment.
fn set_mock_revenue(env: &Env, contract: &Address, business: &Address, period: &str, revenue: i128) {
    let p = String::from_str(env, period);
    env.as_contract(contract, || {
        env.storage().temporary().set(
            &(soroban_sdk::symbol_short!("rev"), business.clone(), p),
            &revenue,
        );
    });
}

/// Mark an attestation as revoked in the test environment.
fn set_mock_revoked(env: &Env, contract: &Address, business: &Address, period: &str) {
    let p = String::from_str(env, period);
    env.as_contract(contract, || {
        env.storage().temporary().set(
            &(soroban_sdk::symbol_short!("rvkd"), business.clone(), p),
            &true,
        );
    });
}

fn issue_period(env: &Env) -> String {
    String::from_str(env, "2026-01")
}

// ─── Initialization ───────────────────────────────────────────────────────────

#[test]
fn test_initialize() {
    let (env, admin, _, _, _, _) = setup_test();
    let contract_id = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    assert_eq!(client.get_admin(), admin);
}

#[test]
#[should_panic(expected = "already initialized")]
fn test_double_initialize_panics() {
    let (env, admin, _, _, _, _) = setup_test();
    let contract_id = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    client.initialize(&admin);
}

// ─── Bond issuance ────────────────────────────────────────────────────────────

#[test]
fn test_issue_bond_fixed_structure() {
    let (env, admin, issuer, owner, token, attestation_contract) = setup_test();
    let contract_id = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let bond_id = client.issue_bond(
        &issuer, &owner, &BondTerms {
            face_value: 10_000_000,
            structure: BondStructure::Fixed,
            revenue_share_bps: 0,
            min_payment_per_period: 500_000,
            max_payment_per_period: 500_000,
            maturity_periods: 12,
            grace_period_seconds: 0,
            issue_period: issue_period(&env),
        }, &attestation_contract, &token,
    );
    assert_eq!(bond_id, 0);
    let bond = client.get_bond(&bond_id).unwrap();
    assert_eq!(bond.face_value, 10_000_000);
    assert_eq!(bond.structure, BondStructure::Fixed);
    assert_eq!(bond.status, BondStatus::Active);
}

#[test]
fn test_issue_bond_revenue_linked() {
    let (env, admin, issuer, owner, token, attestation_contract) = setup_test();
    let contract_id = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let bond_id = client.issue_bond(
        &issuer, &owner, &BondTerms {
            face_value: 5_000_000,
            structure: BondStructure::RevenueLinked,
            revenue_share_bps: 1000,
            min_payment_per_period: 100_000,
            max_payment_per_period: 1_000_000,
            maturity_periods: 24,
            grace_period_seconds: 0,
            issue_period: issue_period(&env),
        }, &attestation_contract, &token,
    );
    let bond = client.get_bond(&bond_id).unwrap();
    assert_eq!(bond.structure, BondStructure::RevenueLinked);
    assert_eq!(bond.revenue_share_bps, 1000);
}

#[test]
fn test_issue_bond_hybrid() {
    let (env, admin, issuer, owner, token, attestation_contract) = setup_test();
    let contract_id = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let bond_id = client.issue_bond(
        &issuer, &owner, &BondTerms {
            face_value: 8_000_000,
            structure: BondStructure::Hybrid,
            revenue_share_bps: 500,
            min_payment_per_period: 200_000,
            max_payment_per_period: 800_000,
            maturity_periods: 18,
            grace_period_seconds: 0,
            issue_period: issue_period(&env),
        }, &attestation_contract, &token,
    );
    let bond = client.get_bond(&bond_id).unwrap();
    assert_eq!(bond.structure, BondStructure::Hybrid);
}

#[test]
#[should_panic(expected = "face_value must be positive")]
fn test_issue_bond_invalid_face_value() {
    let (env, admin, issuer, owner, token, attestation_contract) = setup_test();
    let contract_id = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    client.issue_bond(
        &issuer, &owner, &BondTerms {
            face_value: 0,
            structure: BondStructure::Fixed,
            revenue_share_bps: 0,
            min_payment_per_period: 500_000,
            max_payment_per_period: 500_000,
            maturity_periods: 12,
            grace_period_seconds: 0,
            issue_period: issue_period(&env),
        }, &attestation_contract, &token,
    );
}

#[test]
#[should_panic(expected = "revenue_share_bps must be <= 10000")]
fn test_issue_bond_invalid_revenue_share() {
    let (env, admin, issuer, owner, token, attestation_contract) = setup_test();
    let contract_id = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    client.issue_bond(
        &issuer, &owner, &BondTerms {
            face_value: 10_000_000,
            structure: BondStructure::RevenueLinked,
            revenue_share_bps: 10001,
            min_payment_per_period: 100_000,
            max_payment_per_period: 1_000_000,
            maturity_periods: 12,
            grace_period_seconds: 0,
            issue_period: issue_period(&env),
        }, &attestation_contract, &token,
    );
}

#[test]
#[should_panic(expected = "max must be >= min")]
fn test_issue_bond_invalid_payment_range() {
    let (env, admin, issuer, owner, token, attestation_contract) = setup_test();
    let contract_id = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    client.issue_bond(
        &issuer, &owner, &BondTerms {
            face_value: 10_000_000,
            structure: BondStructure::Fixed,
            revenue_share_bps: 0,
            min_payment_per_period: 1_000_000,
            max_payment_per_period: 500_000,
            maturity_periods: 12,
            grace_period_seconds: 0,
            issue_period: issue_period(&env),
        }, &attestation_contract, &token,
    );
}


// ─── Redemption: basic structures ────────────────────────────────────────────

#[test]
fn test_redeem_fixed_bond() {
    let (env, admin, issuer, owner, token, attestation_contract) = setup_test();
    let contract_id = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let ip = String::from_str(&env, "2026-01");
    let bond_id = client.issue_bond(
        &issuer, &owner, &BondTerms {
            face_value: 10_000_000,
            structure: BondStructure::Fixed,
            revenue_share_bps: 0,
            min_payment_per_period: 500_000,
            max_payment_per_period: 500_000,
            maturity_periods: 12,
            grace_period_seconds: 0,
            issue_period: ip,
        }, &attestation_contract, &token,
    );

    let period = String::from_str(&env, "2026-02");
    set_mock_revenue(&env, &contract_id, &issuer, "2026-02", 2_000_000);
    client.redeem(&bond_id, &period);

    let rec = client.get_redemption(&bond_id, &period).unwrap();
    assert_eq!(rec.redemption_amount, 500_000);
    assert_eq!(client.get_total_redeemed(&bond_id), 500_000);
}

#[test]
fn test_redeem_revenue_linked_bond() {
    let (env, admin, issuer, owner, token, attestation_contract) = setup_test();
    let contract_id = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let ip = String::from_str(&env, "2026-01");
    let bond_id = client.issue_bond(
        &issuer, &owner, &BondTerms {
            face_value: 5_000_000,
            structure: BondStructure::RevenueLinked,
            revenue_share_bps: 1000,
            min_payment_per_period: 100_000,
            max_payment_per_period: 1_000_000,
            maturity_periods: 24,
            grace_period_seconds: 0,
            issue_period: ip,
        }, &attestation_contract, &token,
    );

    set_mock_revenue(&env, &contract_id, &issuer, "2026-02", 5_000_000);
    let period = String::from_str(&env, "2026-02");
    client.redeem(&bond_id, &period);

    // 10% of 5_000_000 = 500_000
    assert_eq!(client.get_redemption(&bond_id, &period).unwrap().redemption_amount, 500_000);
}

#[test]
fn test_redeem_revenue_linked_below_minimum() {
    let (env, admin, issuer, owner, token, attestation_contract) = setup_test();
    let contract_id = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let ip = String::from_str(&env, "2026-01");
    let bond_id = client.issue_bond(
        &issuer, &owner, &BondTerms {
            face_value: 5_000_000,
            structure: BondStructure::RevenueLinked,
            revenue_share_bps: 1000,
            min_payment_per_period: 100_000,
            max_payment_per_period: 1_000_000,
            maturity_periods: 24,
            grace_period_seconds: 0,
            issue_period: ip,
        }, &attestation_contract, &token,
    );

    // 10% of 500_000 = 50_000 < min 100_000 → floors to min
    set_mock_revenue(&env, &contract_id, &issuer, "2026-02", 500_000);
    let period = String::from_str(&env, "2026-02");
    client.redeem(&bond_id, &period);
    assert_eq!(client.get_redemption(&bond_id, &period).unwrap().redemption_amount, 100_000);
}

#[test]
fn test_redeem_revenue_linked_capped_at_max() {
    let (env, admin, issuer, owner, token, attestation_contract) = setup_test();
    let contract_id = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let ip = String::from_str(&env, "2026-01");
    let bond_id = client.issue_bond(
        &issuer, &owner, &BondTerms {
            face_value: 5_000_000,
            structure: BondStructure::RevenueLinked,
            revenue_share_bps: 1000,
            min_payment_per_period: 100_000,
            max_payment_per_period: 1_000_000,
            maturity_periods: 24,
            grace_period_seconds: 0,
            issue_period: ip,
        }, &attestation_contract, &token,
    );

    // 10% of 15_000_000 = 1_500_000 > max 1_000_000 → capped
    set_mock_revenue(&env, &contract_id, &issuer, "2026-02", 15_000_000);
    let period = String::from_str(&env, "2026-02");
    client.redeem(&bond_id, &period);
    assert_eq!(client.get_redemption(&bond_id, &period).unwrap().redemption_amount, 1_000_000);
}

#[test]
fn test_redeem_hybrid_bond() {
    let (env, admin, issuer, owner, token, attestation_contract) = setup_test();
    let contract_id = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let ip = String::from_str(&env, "2026-01");
    let bond_id = client.issue_bond(
        &issuer, &owner, &BondTerms {
            face_value: 8_000_000,
            structure: BondStructure::Hybrid,
            revenue_share_bps: 500,
            min_payment_per_period: 200_000,
            max_payment_per_period: 800_000,
            maturity_periods: 18,
            grace_period_seconds: 0,
            issue_period: ip,
        }, &attestation_contract, &token,
    );

    // min 200_000 + 5% of 10_000_000 = 200_000 + 500_000 = 700_000
    set_mock_revenue(&env, &contract_id, &issuer, "2026-02", 10_000_000);
    let period = String::from_str(&env, "2026-02");
    client.redeem(&bond_id, &period);
    assert_eq!(client.get_redemption(&bond_id, &period).unwrap().redemption_amount, 700_000);
}

// ─── Double-spend prevention ──────────────────────────────────────────────────

#[test]
#[should_panic(expected = "attested_revenue must be non-negative")]
fn test_redeem_rejects_negative_attested_revenue() {
    let (env, admin, issuer, owner, token, attestation_contract) = setup_test();
    let contract_id = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &contract_id);

    client.initialize(&admin);

    let ip = String::from_str(&env, "2026-01");
    let bond_id = client.issue_bond(
        &issuer,
        &owner,
        &BondTerms {
            face_value: 10_000_000,
            structure: BondStructure::Fixed,
            revenue_share_bps: 0,
            min_payment_per_period: 500_000,
            max_payment_per_period: 500_000,
            maturity_periods: 12,
            grace_period_seconds: 0,
            issue_period: ip,
        },
        &attestation_contract,
        &token,
    );

    let period = String::from_str(&env, "2026-02");
    // Explicitly inject a negative revenue value into the mock.
    set_mock_revenue(&env, &contract_id, &issuer, "2026-02", -1);
    client.redeem(&bond_id, &period);
}

#[test]
#[should_panic(expected = "already redeemed for period")]
fn test_redeem_double_spending_prevention() {
    let (env, admin, issuer, owner, token, attestation_contract) = setup_test();
    let contract_id = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let ip = String::from_str(&env, "2026-01");
    let bond_id = client.issue_bond(
        &issuer, &owner, &BondTerms {
            face_value: 10_000_000,
            structure: BondStructure::Fixed,
            revenue_share_bps: 0,
            min_payment_per_period: 500_000,
            max_payment_per_period: 500_000,
            maturity_periods: 12,
            grace_period_seconds: 0,
            issue_period: ip,
        }, &attestation_contract, &token,
    );

    let period = String::from_str(&env, "2026-02");
    set_mock_revenue(&env, &contract_id, &issuer, "2026-02", 0);
    client.redeem(&bond_id, &period);
    client.redeem(&bond_id, &period); // must panic
}

// ─── Multi-period redemptions ─────────────────────────────────────────────────

#[test]
fn test_multiple_period_redemptions() {
    let (env, admin, issuer, owner, token, attestation_contract) = setup_test();
    let contract_id = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let ip = String::from_str(&env, "2026-01");
    let bond_id = client.issue_bond(
        &issuer, &owner, &BondTerms {
            face_value: 10_000_000,
            structure: BondStructure::Fixed,
            revenue_share_bps: 0,
            min_payment_per_period: 500_000,
            max_payment_per_period: 500_000,
            maturity_periods: 12,
            grace_period_seconds: 0,
            issue_period: ip,
        }, &attestation_contract, &token,
    );

    for p in ["2026-01", "2026-02", "2026-03"] {
        set_mock_revenue(&env, &contract_id, &issuer, p, 0);
        client.redeem(&bond_id, &String::from_str(&env, p));
    }

    assert_eq!(client.get_total_redeemed(&bond_id), 1_500_000);
    assert_eq!(client.get_remaining_value(&bond_id), 8_500_000);
}

#[test]
fn test_full_redemption() {
    let (env, admin, issuer, owner, token, attestation_contract) = setup_test();
    let contract_id = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let ip = String::from_str(&env, "2026-01");
    let bond_id = client.issue_bond(
        &issuer, &owner, &BondTerms {
            face_value: 1_000_000,
            structure: BondStructure::Fixed,
            revenue_share_bps: 0,
            min_payment_per_period: 500_000,
            max_payment_per_period: 500_000,
            maturity_periods: 12,
            grace_period_seconds: 0,
            issue_period: ip,
        }, &attestation_contract, &token,
    );

    for p in ["2026-01", "2026-02"] {
        set_mock_revenue(&env, &contract_id, &issuer, p, 0);
        client.redeem(&bond_id, &String::from_str(&env, p));
    }

    let bond = client.get_bond(&bond_id).unwrap();
    assert_eq!(bond.status, BondStatus::FullyRedeemed);
    assert_eq!(client.get_total_redeemed(&bond_id), 1_000_000);
    assert_eq!(client.get_remaining_value(&bond_id), 0);
}

#[test]
fn test_partial_redemption_caps_at_face_value() {
    let (env, admin, issuer, owner, token, attestation_contract) = setup_test();
    let contract_id = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    // face_value=1_200_000, max_per_period=500_000 → 3 periods needed
    let bond_id = client.issue_bond(
        &issuer, &owner, &BondTerms {
            face_value: 1_200_000,
            structure: BondStructure::Fixed,
            revenue_share_bps: 0,
            min_payment_per_period: 500_000,
            max_payment_per_period: 500_000,
            maturity_periods: 12,
            grace_period_seconds: 0,
            issue_period: issue_period(&env),
        }, &attestation_contract, &token,
    );

    for p in ["2026-01", "2026-02", "2026-03"] {
        set_mock_revenue(&env, &contract_id, &issuer, p, 0);
        client.redeem(&bond_id, &String::from_str(&env, p));
    }

    let bond = client.get_bond(&bond_id).unwrap();
    assert_eq!(bond.status, BondStatus::FullyRedeemed);
    assert_eq!(client.get_total_redeemed(&bond_id), 1_200_000);
}


// ─── Ownership transfer ───────────────────────────────────────────────────────

#[test]
fn test_transfer_ownership() {
    let (env, admin, issuer, owner, token, attestation_contract) = setup_test();
    let contract_id = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let new_owner = Address::generate(&env);
    let bond_id = client.issue_bond(
        &issuer, &owner, &BondTerms {
            face_value: 10_000_000,
            structure: BondStructure::Fixed,
            revenue_share_bps: 0,
            min_payment_per_period: 500_000,
            max_payment_per_period: 500_000,
            maturity_periods: 12,
            grace_period_seconds: 0,
            issue_period: issue_period(&env),
        }, &attestation_contract, &token,
    );
    client.transfer_ownership(&bond_id, &owner, &new_owner);
    assert_eq!(client.get_owner(&bond_id).unwrap(), new_owner);
}

#[test]
#[should_panic(expected = "not bond owner")]
fn test_transfer_ownership_unauthorized() {
    let (env, admin, issuer, owner, token, attestation_contract) = setup_test();
    let contract_id = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let fake_owner = Address::generate(&env);
    let new_owner = Address::generate(&env);
    let bond_id = client.issue_bond(
        &issuer, &owner, &BondTerms {
            face_value: 10_000_000,
            structure: BondStructure::Fixed,
            revenue_share_bps: 0,
            min_payment_per_period: 500_000,
            max_payment_per_period: 500_000,
            maturity_periods: 12,
            grace_period_seconds: 0,
            issue_period: issue_period(&env),
        }, &attestation_contract, &token,
    );
    client.transfer_ownership(&bond_id, &fake_owner, &new_owner);
}

#[test]
#[should_panic(expected = "cannot transfer to self")]
fn test_transfer_ownership_to_self_panics() {
    let (env, admin, issuer, owner, token, attestation_contract) = setup_test();
    let contract_id = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let bond_id = client.issue_bond(
        &issuer, &owner, &10_000_000, &BondStructure::Fixed,
        &0, &500_000, &500_000, &12, &issue_period(&env), &attestation_contract, &token,
    );
    client.transfer_ownership(&bond_id, &owner, &owner);
}

#[test]
#[should_panic(expected = "cannot transfer to issuer")]
fn test_transfer_ownership_to_issuer_panics() {
    let (env, admin, issuer, owner, token, attestation_contract) = setup_test();
    let contract_id = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let bond_id = client.issue_bond(
        &issuer, &owner, &10_000_000, &BondStructure::Fixed,
        &0, &500_000, &500_000, &12, &issue_period(&env), &attestation_contract, &token,
    );
    client.transfer_ownership(&bond_id, &owner, &issuer);
}

#[test]
#[should_panic(expected = "cannot transfer to contract itself")]
fn test_transfer_ownership_to_contract_itself_panics() {
    let (env, admin, issuer, owner, token, attestation_contract) = setup_test();
    let contract_id = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let bond_id = client.issue_bond(
        &issuer, &owner, &10_000_000, &BondStructure::Fixed,
        &0, &500_000, &500_000, &12, &issue_period(&env), &attestation_contract, &token,
    );
    client.transfer_ownership(&bond_id, &owner, &contract_id);
}

#[test]
#[should_panic(expected = "bond not active")]
fn test_transfer_ownership_when_not_active_panics() {
    let (env, admin, issuer, owner, token, attestation_contract) = setup_test();
    let contract_id = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let bond_id = client.issue_bond(
        &issuer, &owner, &10_000_000, &BondStructure::Fixed,
        &0, &500_000, &500_000, &12, &issue_period(&env), &attestation_contract, &token,
    );
    
    client.mark_defaulted(&admin, &bond_id);
    
    let new_owner = Address::generate(&env);
    client.transfer_ownership(&bond_id, &owner, &new_owner);
}

#[test]
fn test_transfer_ownership_during_active_redemption_window() {
    let (env, admin, issuer, owner, token, attestation_contract) = setup_test();
    let contract_id = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let bond_id = client.issue_bond(
        &issuer, &owner, &10_000_000, &BondStructure::Fixed,
        &0, &500_000, &500_000, &12, &issue_period(&env), &attestation_contract, &token,
    );

    // Period 1 Redemption
    let period1 = String::from_str(&env, "2026-02");
    set_mock_revenue(&env, &issuer, "2026-02", 2_000_000);
    client.redeem(&bond_id, &period1);
    
    assert_eq!(client.get_total_redeemed(&bond_id), 500_000);

    // Transfer ownership during active redemption window
    let new_owner = Address::generate(&env);
    client.transfer_ownership(&bond_id, &owner, &new_owner);
    assert_eq!(client.get_owner(&bond_id).unwrap(), new_owner);

    // Period 2 Redemption - should pay the new owner
    let period2 = String::from_str(&env, "2026-03");
    set_mock_revenue(&env, &issuer, "2026-03", 2_000_000);
    
    let token_client = token::Client::new(&env, &token);
    let initial_balance = token_client.balance(&new_owner);
    assert_eq!(initial_balance, 0);

    client.redeem(&bond_id, &period2);

    let final_balance = token_client.balance(&new_owner);
    assert_eq!(final_balance, 500_000);
    assert_eq!(client.get_total_redeemed(&bond_id), 1_000_000);
}

// ─── Admin operations ─────────────────────────────────────────────────────────

#[test]
fn test_mark_defaulted() {
    let (env, admin, issuer, owner, token, attestation_contract) = setup_test();
    let contract_id = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let ip = String::from_str(&env, "2026-01");
    let bond_id = client.issue_bond(
        &issuer, &owner, &BondTerms {
            face_value: 10_000_000,
            structure: BondStructure::Fixed,
            revenue_share_bps: 0,
            min_payment_per_period: 500_000,
            max_payment_per_period: 500_000,
            maturity_periods: 12,
            grace_period_seconds: 0,
            issue_period: ip,
        }, &attestation_contract, &token,
    );
    client.mark_defaulted(&admin, &bond_id);
    assert_eq!(client.get_bond(&bond_id).unwrap().status, BondStatus::Defaulted);
}

#[test]
#[should_panic(expected = "unauthorized")]
fn test_mark_defaulted_unauthorized() {
    let (env, admin, issuer, owner, token, attestation_contract) = setup_test();
    let contract_id = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let ip = String::from_str(&env, "2026-01");
    let bond_id = client.issue_bond(
        &issuer, &owner, &BondTerms {
            face_value: 10_000_000,
            structure: BondStructure::Fixed,
            revenue_share_bps: 0,
            min_payment_per_period: 500_000,
            max_payment_per_period: 500_000,
            maturity_periods: 12,
            grace_period_seconds: 0,
            issue_period: ip,
        }, &attestation_contract, &token,
    );
    client.mark_defaulted(&Address::generate(&env), &bond_id);
}

#[test]
#[should_panic(expected = "bond not active")]
fn test_redeem_defaulted_bond() {
    let (env, admin, issuer, owner, token, attestation_contract) = setup_test();
    let contract_id = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let bond_id = client.issue_bond(
        &issuer, &owner, &BondTerms {
            face_value: 10_000_000,
            structure: BondStructure::Fixed,
            revenue_share_bps: 0,
            min_payment_per_period: 500_000,
            max_payment_per_period: 500_000,
            maturity_periods: 12,
            grace_period_seconds: 0,
            issue_period: issue_period(&env),
        }, &attestation_contract, &token,
    );
    client.mark_defaulted(&admin, &bond_id);
    set_mock_revenue(&env, &contract_id, &issuer, "2026-02", 0);
    client.redeem(&bond_id, &String::from_str(&env, "2026-02"));
}

// ─── Revocation mid-cycle guard ───────────────────────────────────────────────

#[test]
#[should_panic(expected = "attestation is revoked")]
fn test_redeem_revoked_attestation_panics() {
    let (env, admin, issuer, owner, token, attestation_contract) = setup_test();
    let contract_id = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let bond_id = client.issue_bond(
        &issuer, &owner, &BondTerms {
            face_value: 5_000_000,
            structure: BondStructure::Fixed,
            revenue_share_bps: 0,
            min_payment_per_period: 500_000,
            max_payment_per_period: 500_000,
            maturity_periods: 12,
            grace_period_seconds: 0,
            issue_period: issue_period(&env),
        }, &attestation_contract, &token,
    );

    // Attestation is revoked mid-cycle before redemption.
    set_mock_revoked(&env, &contract_id, &issuer, "2026-03");
    client.redeem(&bond_id, &String::from_str(&env, "2026-03"));
}

#[test]
fn test_redeem_succeeds_for_non_revoked_period_after_other_revoked() {
    // Revocation of one period must not block redemption of a different period.
    let (env, admin, issuer, owner, token, attestation_contract) = setup_test();
    let contract_id = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let ip = String::from_str(&env, "2026-01");
    let bond_id = client.issue_bond(
        &issuer, &owner, &BondTerms {
            face_value: 5_000_000,
            structure: BondStructure::Fixed,
            revenue_share_bps: 0,
            min_payment_per_period: 500_000,
            max_payment_per_period: 500_000,
            maturity_periods: 12,
            grace_period_seconds: 0,
            issue_period: ip,
        }, &attestation_contract, &token,
    );

    set_mock_revoked(&env, &contract_id, &issuer, "2026-02");
    set_mock_revenue(&env, &contract_id, &issuer, "2026-03", 0);
    // 2026-03 is not revoked — must succeed.
    client.redeem(&bond_id, &String::from_str(&env, "2026-03"));
    assert_eq!(client.get_total_redeemed(&bond_id), 500_000);
}

// ─── Per-period cap enforcement ───────────────────────────────────────────────

#[test]
fn test_per_period_cap_not_exceeded_on_last_partial() {
    // face_value=700_000, max_per_period=500_000.
    // After period1 (500_000), remaining=200_000 < max.
    // The per-period cap assertion must pass because 200_000 <= 500_000.
    let (env, admin, issuer, owner, token, attestation_contract) = setup_test();
    let contract_id = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let bond_id = client.issue_bond(
        &issuer, &owner, &BondTerms {
            face_value: 700_000,
            structure: BondStructure::Fixed,
            revenue_share_bps: 0,
            min_payment_per_period: 500_000,
            max_payment_per_period: 500_000,
            maturity_periods: 12,
            grace_period_seconds: 0,
            issue_period: issue_period(&env),
        }, &attestation_contract, &token,
    );

    set_mock_revenue(&env, &contract_id, &issuer, "2026-01", 0);
    client.redeem(&bond_id, &String::from_str(&env, "2026-01"));
    assert_eq!(client.get_total_redeemed(&bond_id), 500_000);

    set_mock_revenue(&env, &contract_id, &issuer, "2026-02", 0);
    client.redeem(&bond_id, &String::from_str(&env, "2026-02"));
    // Last partial: 200_000 transferred, bond fully redeemed.
    assert_eq!(client.get_total_redeemed(&bond_id), 700_000);
    assert_eq!(client.get_bond(&bond_id).unwrap().status, BondStatus::FullyRedeemed);
}

// ─── Revenue read from attestation (not caller-supplied) ─────────────────────

#[test]
fn test_revenue_is_read_from_attestation_not_caller() {
    // The mock returns whatever is in temporary storage.
    // We set revenue=8_000_000; 10% = 800_000, capped at max 1_000_000.
    let (env, admin, issuer, owner, token, attestation_contract) = setup_test();
    let contract_id = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let bond_id = client.issue_bond(
        &issuer, &owner, &BondTerms {
            face_value: 5_000_000,
            structure: BondStructure::RevenueLinked,
            revenue_share_bps: 1000,
            min_payment_per_period: 100_000,
            max_payment_per_period: 1_000_000,
            maturity_periods: 24,
            grace_period_seconds: 0,
            issue_period: issue_period(&env),
        }, &attestation_contract, &token,
    );

    set_mock_revenue(&env, &contract_id, &issuer, "2026-02", 8_000_000);
    let period = String::from_str(&env, "2026-02");
    client.redeem(&bond_id, &period);

    let rec = client.get_redemption(&bond_id, &period).unwrap();
    // attested_revenue must be what the attestation contract returned, not a caller value.
    assert_eq!(rec.attested_revenue, 8_000_000);
    assert_eq!(rec.redemption_amount, 800_000);
}

// ─── Edge cases ───────────────────────────────────────────────────────────────

#[test]
fn test_early_redemption_scenario() {
    let (env, admin, issuer, owner, token, attestation_contract) = setup_test();
    let contract_id = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let bond_id = client.issue_bond(
        &issuer, &owner, &BondTerms {
            face_value: 4_500_000,
            structure: BondStructure::RevenueLinked,
            revenue_share_bps: 2000,
            min_payment_per_period: 100_000,
            max_payment_per_period: 2_000_000,
            maturity_periods: 24,
            grace_period_seconds: 0,
            issue_period: issue_period(&env),
        }, &attestation_contract, &token,
    );

    // 20% of 8_000_000 = 1_600_000; 20% of 10_000_000 = 2_000_000; 20% of 5_000_000 = 1_000_000
    for (p, rev) in [("2026-01", 8_000_000i128), ("2026-02", 10_000_000), ("2026-03", 5_000_000)] {
        set_mock_revenue(&env, &contract_id, &issuer, p, rev);
        client.redeem(&bond_id, &String::from_str(&env, p));
    }

    assert_eq!(client.get_bond(&bond_id).unwrap().status, BondStatus::FullyRedeemed);
    assert_eq!(client.get_total_redeemed(&bond_id), 4_500_000);
}

#[test]
fn test_zero_revenue_hybrid_pays_minimum() {
    let (env, admin, issuer, owner, token, attestation_contract) = setup_test();
    let contract_id = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let ip = String::from_str(&env, "2026-01");
    let bond_id = client.issue_bond(
        &issuer, &owner, &BondTerms {
            face_value: 3_000_000,
            structure: BondStructure::Hybrid,
            revenue_share_bps: 500,
            min_payment_per_period: 200_000,
            max_payment_per_period: 800_000,
            maturity_periods: 12,
            grace_period_seconds: 0,
            issue_period: ip,
        }, &attestation_contract, &token,
    );

    // revenue=0 → min + 0 = 200_000
    set_mock_revenue(&env, &contract_id, &issuer, "2026-01", 0);
    let period = String::from_str(&env, "2026-01");
    client.redeem(&bond_id, &period);
    assert_eq!(client.get_redemption(&bond_id, &period).unwrap().redemption_amount, 200_000);
}

#[test]
fn test_max_i128_revenue_does_not_overflow() {
    // saturating_mul must prevent overflow for extreme revenue values.
    let (env, admin, issuer, owner, token, attestation_contract) = setup_test();
    let contract_id = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let bond_id = client.issue_bond(
        &issuer, &owner, &BondTerms {
            face_value: 5_000_000,
            structure: BondStructure::RevenueLinked,
            revenue_share_bps: 1000,
            min_payment_per_period: 0,
            max_payment_per_period: 1_000_000,
            maturity_periods: 24,
            grace_period_seconds: 0,
            issue_period: issue_period(&env),
        }, &attestation_contract, &token,
    );

    set_mock_revenue(&env, &contract_id, &issuer, "2026-01", i128::MAX);
    let period = String::from_str(&env, "2026-01");
    client.redeem(&bond_id, &period);
    // saturating_mul caps at u128::MAX then cast to i128 → clamped to max_payment_per_period
    assert_eq!(client.get_redemption(&bond_id, &period).unwrap().redemption_amount, 1_000_000);
}

#[test]
fn test_out_of_order_period_redemption() {
    let (env, admin, issuer, owner, token, attestation_contract) = setup_test();
    let contract_id = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let bond_id = client.issue_bond(
        &issuer, &owner, &BondTerms {
            face_value: 2_000_000,
            structure: BondStructure::Fixed,
            revenue_share_bps: 0,
            min_payment_per_period: 500_000,
            max_payment_per_period: 500_000,
            maturity_periods: 12,
            grace_period_seconds: 0,
            issue_period: issue_period(&env),
        }, &attestation_contract, &token,
    );

    // Redeem a later period first, then an earlier one — both must succeed.
    set_mock_revenue(&env, &contract_id, &issuer, "2026-03", 0);
    set_mock_revenue(&env, &contract_id, &issuer, "2026-01", 0);
    client.redeem(&bond_id, &String::from_str(&env, "2026-03"));
    client.redeem(&bond_id, &String::from_str(&env, "2026-01"));
    assert_eq!(client.get_total_redeemed(&bond_id), 1_000_000);
}
