#![cfg(test)]
use super::*;
use soroban_sdk::{testutils::Address as _, token, Address, Env, String};

fn create_token<'a>(env: &Env, admin: &Address) -> token::StellarAssetClient<'a> {
    token::StellarAssetClient::new(
        env,
        &env.register_stellar_asset_contract_v2(admin.clone()).address(),
    )
}

fn setup() -> (Env, Address, Address, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.mock_all_auths_allowing_non_root_auth();
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let owner = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let tc = create_token(&env, &token_admin);
    tc.mint(&issuer, &500_000_000);
    let attestation = Address::generate(&env);
    (env, admin, issuer, owner, tc.address.clone(), attestation)
}

fn set_mock_revenue(env: &Env, contract: &Address, business: &Address, period: &str, revenue: i128) {
    let p = String::from_str(env, period);
    env.as_contract(contract, || {
        env.storage().temporary().set(
            &(soroban_sdk::symbol_short!("rev"), business.clone(), p),
            &revenue,
        );
    });
}

fn issue_hybrid(
    client: &RevenueBondContractClient,
    env: &Env,
    issuer: &Address,
    owner: &Address,
    face_value: i128,
    revenue_share_bps: u32,
    min_payment: i128,
    max_payment: i128,
    maturity: u32,
    attestation: &Address,
    token: &Address,
) -> u64 {
    client.issue_bond(
        issuer, owner, &BondTerms {
            face_value,
            structure: BondStructure::Hybrid,
            revenue_share_bps,
            min_payment_per_period: min_payment,
            max_payment_per_period: max_payment,
            maturity_periods: maturity,
            grace_period_seconds: 0,
            issue_period: String::from_str(env, "2026-01"),
        }, attestation, token,
    )
}

#[test]
fn test_hybrid_revenue_spike_capped_at_max() {
    let (env, admin, issuer, owner, token, attestation) = setup();
    let c = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &c);
    client.initialize(&admin);
    
    let id = issue_hybrid(&client, &env, &issuer, &owner,
        10_000_000, 1000, 200_000, 800_000, 24, &attestation, &token);
    
    let p_str = "2026-02";
    let p = String::from_str(&env, p_str);
    set_mock_revenue(&env, &c, &issuer, p_str, 10_000_000);
    client.redeem(&id, &p);
    
    let rec = client.get_redemption(&id, &p).unwrap();
    assert_eq!(rec.redemption_amount, 800_000);
}

#[test]
fn test_hybrid_spike_multi_period_face_value_cap() {
    let (env, admin, issuer, owner, token, attestation) = setup();
    let c = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &c);
    client.initialize(&admin);

    let id = issue_hybrid(&client, &env, &issuer, &owner,
        1_500_000, 1000, 200_000, 800_000, 24, &attestation, &token);
    
    let p1_str = "2026-02";
    let p2_str = "2026-03";
    let p1 = String::from_str(&env, p1_str);
    let p2 = String::from_str(&env, p2_str);
    
    set_mock_revenue(&env, &c, &issuer, p1_str, 10_000_000);
    client.redeem(&id, &p1);
    
    set_mock_revenue(&env, &c, &issuer, p2_str, 10_000_000);
    client.redeem(&id, &p2);
    
    let bond = client.get_bond(&id).unwrap();
    assert_eq!(bond.status, BondStatus::FullyRedeemed);
    assert_eq!(client.get_total_redeemed(&id), 1_500_000);
    let r2 = client.get_redemption(&id, &p2).unwrap();
    assert_eq!(r2.redemption_amount, 700_000);
}

#[test]
fn test_hybrid_extreme_revenue_saturating_arithmetic() {
    let (env, admin, issuer, owner, token, attestation) = setup();
    let c = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &c);
    client.initialize(&admin);
    let id = issue_hybrid(&client, &env, &issuer, &owner,
        5_000_000, 500, 100_000, 1_000_000, 24, &attestation, &token);

    let p_str = "2026-02";
    let p = String::from_str(&env, p_str);
    set_mock_revenue(&env, &c, &issuer, p_str, i128::MAX);
    client.redeem(&id, &p);
    
    let rec = client.get_redemption(&id, &p).unwrap();
    assert_eq!(rec.redemption_amount, 1_000_000);
}

#[test]
fn test_hybrid_zero_revenue_pays_min_only() {
    let (env, admin, issuer, owner, token, attestation) = setup();
    let c = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &c);
    client.initialize(&admin);

    let id = issue_hybrid(&client, &env, &issuer, &owner,
        5_000_000, 1000, 200_000, 800_000, 24, &attestation, &token);
    
    let p_str = "2026-02";
    let p = String::from_str(&env, p_str);
    set_mock_revenue(&env, &c, &issuer, p_str, 0);
    client.redeem(&id, &p);
    
    let rec = client.get_redemption(&id, &p).unwrap();
    assert_eq!(rec.redemption_amount, 200_000);
    assert_eq!(client.get_total_redeemed(&id), 200_000);
}

#[test]
fn test_hybrid_min_fixed_not_double_paid_across_periods() {
    let (env, admin, issuer, owner, token, attestation) = setup();
    let c = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &c);
    client.initialize(&admin);

    let id = issue_hybrid(&client, &env, &issuer, &owner,
        600_000, 1000, 200_000, 800_000, 24, &attestation, &token);
    
    let periods = ["2026-02", "2026-03", "2026-04"];
    for p_str in periods {
        let p = String::from_str(&env, p_str);
        set_mock_revenue(&env, &c, &issuer, p_str, 0);
        client.redeem(&id, &p);
    }
    
    assert_eq!(client.get_total_redeemed(&id), 600_000);
    let bond = client.get_bond(&id).unwrap();
    assert_eq!(bond.status, BondStatus::FullyRedeemed);
}

#[test]
fn test_hybrid_collapse_after_spike_caps_correctly() {
    let (env, admin, issuer, owner, token, attestation) = setup();
    let c = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &c);
    client.initialize(&admin);

    let id = issue_hybrid(&client, &env, &issuer, &owner,
        1_000_000, 500, 100_000, 600_000, 24, &attestation, &token);
    
    let p1_str = "2026-02";
    let p1 = String::from_str(&env, p1_str);
    set_mock_revenue(&env, &c, &issuer, p1_str, 8_000_000);
    client.redeem(&id, &p1);
    assert_eq!(client.get_total_redeemed(&id), 500_000);

    let p2_str = "2026-03";
    let p2 = String::from_str(&env, p2_str);
    set_mock_revenue(&env, &c, &issuer, p2_str, 0);
    client.redeem(&id, &p2);
    assert_eq!(client.get_total_redeemed(&id), 600_000);

    let p3_str = "2026-04";
    let p3 = String::from_str(&env, p3_str);
    set_mock_revenue(&env, &c, &issuer, p3_str, 0);
    client.redeem(&id, &p3);
    assert_eq!(client.get_total_redeemed(&id), 700_000);

    let bond = client.get_bond(&id).unwrap();
    assert_eq!(bond.status, BondStatus::Active);
    assert_eq!(client.get_remaining_value(&id), 300_000);
}

#[test]
fn test_hybrid_revenue_component_equals_min_no_double_count() {
    let (env, admin, issuer, owner, token, attestation) = setup();
    let c = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &c);
    client.initialize(&admin);

    let id = issue_hybrid(&client, &env, &issuer, &owner,
        5_000_000, 1000, 200_000, 800_000, 24, &attestation, &token);
    
    let p_str = "2026-02";
    let p = String::from_str(&env, p_str);
    set_mock_revenue(&env, &c, &issuer, p_str, 2_000_000);
    client.redeem(&id, &p);
    
    let rec = client.get_redemption(&id, &p).unwrap();
    assert_eq!(rec.redemption_amount, 400_000);
}

#[test]
#[should_panic(expected = "already redeemed for period")]
fn test_hybrid_double_spend_same_period_panics() {
    let (env, admin, issuer, owner, token, attestation) = setup();
    let c = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &c);
    client.initialize(&admin);
    let id = issue_hybrid(&client, &env, &issuer, &owner,
        5_000_000, 1000, 200_000, 800_000, 24, &attestation, &token);
    
    let p_str = "2026-02";
    let p = String::from_str(&env, p_str);
    set_mock_revenue(&env, &c, &issuer, p_str, 3_000_000);
    client.redeem(&id, &p);
    
    set_mock_revenue(&env, &c, &issuer, p_str, 0);
    client.redeem(&id, &p);
}

#[test]
#[should_panic(expected = "already redeemed for period")]
fn test_hybrid_double_spend_zero_after_spike_panics() {
    let (env, admin, issuer, owner, token, attestation) = setup();
    let c = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &c);
    client.initialize(&admin);
    let id = issue_hybrid(&client, &env, &issuer, &owner,
        5_000_000, 1000, 200_000, 800_000, 24, &attestation, &token);
    
    let p_str = "2026-03";
    let p = String::from_str(&env, p_str);
    set_mock_revenue(&env, &c, &issuer, p_str, 10_000_000);
    client.redeem(&id, &p);
    
    set_mock_revenue(&env, &c, &issuer, p_str, 0);
    client.redeem(&id, &p);
}

#[test]
fn test_hybrid_attestation_present_succeeds() {
    let (env, admin, issuer, owner, token, attestation) = setup();
    let c = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &c);
    client.initialize(&admin);
    let id = issue_hybrid(&client, &env, &issuer, &owner,
        5_000_000, 1000, 200_000, 800_000, 24, &attestation, &token);
    
    let p_str = "2026-02";
    let p = String::from_str(&env, p_str);
    set_mock_revenue(&env, &c, &issuer, p_str, 1_000_000);
    client.redeem(&id, &p);
    
    let rec = client.get_redemption(&id, &p).unwrap();
    assert_eq!(rec.redemption_amount, 300_000);
}

#[test]
fn test_hybrid_payment_routes_to_current_owner_after_transfer() {
    let (env, admin, issuer, owner, token, attestation) = setup();
    let c = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &c);
    client.initialize(&admin);
    let id = issue_hybrid(&client, &env, &issuer, &owner,
        5_000_000, 1000, 200_000, 800_000, 24, &attestation, &token);

    let p1_str = "2026-02";
    let p1 = String::from_str(&env, p1_str);
    set_mock_revenue(&env, &c, &issuer, p1_str, 5_000_000);
    client.redeem(&id, &p1);
    assert_eq!(client.get_owner(&id).unwrap(), owner);

    let new_owner = Address::generate(&env);
    client.transfer_ownership(&id, &owner, &new_owner);
    assert_eq!(client.get_owner(&id).unwrap(), new_owner);

    let p2_str = "2026-03";
    let p2 = String::from_str(&env, p2_str);
    set_mock_revenue(&env, &c, &issuer, p2_str, 0);
    client.redeem(&id, &p2);
    
    let r2 = client.get_redemption(&id, &p2).unwrap();
    assert_eq!(r2.redemption_amount, 200_000);
}

#[test]
fn test_hybrid_exact_face_value_exhaustion() {
    let (env, admin, issuer, owner, token, attestation) = setup();
    let c = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &c);
    client.initialize(&admin);
    
    let id = issue_hybrid(&client, &env, &issuer, &owner,
        500_000, 1000, 200_000, 500_000, 24, &attestation, &token);
    
    let p_str = "2026-02";
    let p = String::from_str(&env, p_str);
    set_mock_revenue(&env, &c, &issuer, p_str, 3_000_000);
    client.redeem(&id, &p);
    
    let bond = client.get_bond(&id).unwrap();
    assert_eq!(bond.status, BondStatus::FullyRedeemed);
    assert_eq!(client.get_total_redeemed(&id), 500_000);
    assert_eq!(client.get_remaining_value(&id), 0);
}

#[test]
#[should_panic(expected = "bond not active")]
fn test_hybrid_redeem_after_fully_redeemed_panics() {
    let (env, admin, issuer, owner, token, attestation) = setup();
    let c = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &c);
    client.initialize(&admin);
    let id = issue_hybrid(&client, &env, &issuer, &owner,
        300_000, 1000, 200_000, 500_000, 24, &attestation, &token);
    
    let p1_str = "2026-02";
    let p2_str = "2026-03";
    let p1 = String::from_str(&env, p1_str);
    let p2 = String::from_str(&env, p2_str);
    
    set_mock_revenue(&env, &c, &issuer, p1_str, 0);
    client.redeem(&id, &p1);
    
    set_mock_revenue(&env, &c, &issuer, p2_str, 0);
    client.redeem(&id, &p2);
    
    let p3_str = "2026-04";
    let p3 = String::from_str(&env, p3_str);
    client.redeem(&id, &p3);
}

#[test]
fn test_hybrid_zero_share_bps_equals_fixed_behavior() {
    let (env, admin, issuer, owner, token, attestation) = setup();
    let c = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &c);
    client.initialize(&admin);
    let ip = String::from_str(&env, "2026-01");
    let id = client.issue_bond(
        &issuer, &owner, &BondTerms {
            face_value: 2_000_000,
            structure: BondStructure::Hybrid,
            revenue_share_bps: 0,
            min_payment_per_period: 300_000,
            max_payment_per_period: 300_000,
            maturity_periods: 12,
            grace_period_seconds: 0,
            issue_period: ip,
        }, &attestation, &token,
    );
    
    let p_str = "2026-02";
    let p = String::from_str(&env, p_str);
    set_mock_revenue(&env, &c, &issuer, p_str, 50_000_000);
    client.redeem(&id, &p);
    
    let rec = client.get_redemption(&id, &p).unwrap();
    assert_eq!(rec.redemption_amount, 300_000);
}

#[test]
fn test_hybrid_max_share_bps_formula() {
    let (env, admin, issuer, owner, token, attestation) = setup();
    let c = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &c);
    client.initialize(&admin);
    let ip = String::from_str(&env, "2026-01");
    let id = client.issue_bond(
        &issuer, &owner, &BondTerms {
            face_value: 10_000_000,
            structure: BondStructure::Hybrid,
            revenue_share_bps: 10000,
            min_payment_per_period: 100_000,
            max_payment_per_period: 2_000_000,
            maturity_periods: 24,
            grace_period_seconds: 0,
            issue_period: ip,
        }, &attestation, &token,
    );
    
    let p_str = "2026-02";
    let p = String::from_str(&env, p_str);
    set_mock_revenue(&env, &c, &issuer, p_str, 500_000);
    client.redeem(&id, &p);
    
    let rec = client.get_redemption(&id, &p).unwrap();
    assert_eq!(rec.redemption_amount, 600_000);
}

#[test]
#[should_panic(expected = "issuer and owner must differ")]
fn test_hybrid_issuer_equals_owner_panics() {
    let (env, admin, issuer, _, token, attestation) = setup();
    let c = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &c);
    client.initialize(&admin);
    let ip = String::from_str(&env, "2026-01");
    client.issue_bond(
        &issuer, &issuer, &BondTerms {
            face_value: 1_000_000,
            structure: BondStructure::Hybrid,
            revenue_share_bps: 500,
            min_payment_per_period: 100_000,
            max_payment_per_period: 500_000,
            maturity_periods: 12,
            grace_period_seconds: 0,
            issue_period: ip,
        }, &attestation, &token,
    );
}

#[test]
#[should_panic(expected = "max must be >= min")]
fn test_hybrid_invalid_payment_range_panics() {
    let (env, admin, issuer, owner, token, attestation) = setup();
    let c = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &c);
    client.initialize(&admin);
    let ip = String::from_str(&env, "2026-01");
    client.issue_bond(
        &issuer, &owner, &BondTerms {
            face_value: 1_000_000,
            structure: BondStructure::Hybrid,
            revenue_share_bps: 500,
            min_payment_per_period: 500_000,
            max_payment_per_period: 100_000,
            maturity_periods: 12,
            grace_period_seconds: 0,
            issue_period: ip,
        }, &attestation, &token,
    );
}

#[test]
#[should_panic(expected = "attested_revenue must be non-negative")]
fn test_hybrid_negative_revenue_panics() {
    let (env, admin, issuer, owner, token, attestation) = setup();
    let c = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &c);
    client.initialize(&admin);
    let id = issue_hybrid(&client, &env, &issuer, &owner,
        5_000_000, 1000, 200_000, 800_000, 24, &attestation, &token);
    
    let p_str = "2026-02";
    let p = String::from_str(&env, p_str);
    set_mock_revenue(&env, &c, &issuer, p_str, -1);
    client.redeem(&id, &p);
}

#[test]
#[should_panic(expected = "bond not active")]
fn test_hybrid_defaulted_bond_rejects_redemption() {
    let (env, admin, issuer, owner, token, attestation) = setup();
    let c = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &c);
    client.initialize(&admin);
    let id = issue_hybrid(&client, &env, &issuer, &owner,
        5_000_000, 1000, 200_000, 800_000, 24, &attestation, &token);
    client.mark_defaulted(&admin, &id);
    let p = String::from_str(&env, "2026-02");
    client.redeem(&id, &p);
}

#[test]
fn test_hybrid_remaining_value_monotonically_decreasing() {
    let (env, admin, issuer, owner, token, attestation) = setup();
    let c = env.register(RevenueBondContract, ());
    let client = RevenueBondContractClient::new(&env, &c);
    client.initialize(&admin);
    let id = issue_hybrid(&client, &env, &issuer, &owner,
        2_000_000, 500, 100_000, 500_000, 24, &attestation, &token);

    let revenues: [i128; 5] = [0, 10_000_000, 0, 5_000_000, 0];
    let periods = ["2026-02", "2026-03", "2026-04", "2026-05", "2026-06"];
    let mut prev_remaining = client.get_remaining_value(&id);

    for (rev, period_str) in revenues.iter().zip(periods.iter()) {
        let p = String::from_str(&env, *period_str);
        set_mock_revenue(&env, &c, &issuer, *period_str, *rev);
        client.redeem(&id, &p);
        let remaining = client.get_remaining_value(&id);
        assert!(remaining <= prev_remaining, "remaining_value must not increase");
        assert!(remaining >= 0, "remaining_value must not go negative");
        prev_remaining = remaining;
    }
}
