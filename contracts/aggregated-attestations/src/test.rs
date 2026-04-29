//! Tests for aggregated attestations: batch consistency, portfolio guardrails, nonces, auth.

use super::{
    AggregatedAttestationsContract, AggregatedAttestationsContractClient, MAX_PORTFOLIO_BUSINESSES,
    NONCE_CHANNEL_ADMIN,
};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Env, String, Vec};
use veritasor_attestation_snapshot::{
    AttestationSnapshotContract, AttestationSnapshotContractClient,
};

fn admin_nonce(client: &AggregatedAttestationsContractClient<'_>, admin: &Address) -> u64 {
    client.get_replay_nonce(admin, &NONCE_CHANNEL_ADMIN)
}

fn setup(
    env: &Env,
) -> (
    AggregatedAttestationsContractClient<'static>,
    AttestationSnapshotContractClient<'static>,
    Address,
    Address,
) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let agg_id = env.register(AggregatedAttestationsContract, ());
    let agg_client = AggregatedAttestationsContractClient::new(env, &agg_id);
    agg_client.initialize(&admin, &0u64);

    let snap_id = env.register(AttestationSnapshotContract, ());
    let snap_client = AttestationSnapshotContractClient::new(env, &snap_id);
    snap_client.initialize(&admin, &None::<Address>);
    (agg_client, snap_client, admin, agg_id, snap_id)
}

fn businesses(env: &Env, addrs: &[Address]) -> Vec<Address> {
    let mut out = Vec::new(env);
    for addr in addrs {
        out.push_back(addr.clone());
    }
    out
}

#[test]
fn test_initialize() {
    let env = Env::default();
    let (client, _snap, admin, _agg_id, _snap_id) = setup(&env);
    assert_eq!(client.get_admin(), admin);
    assert_eq!(client.get_max_portfolio_businesses(), MAX_PORTFOLIO_BUSINESSES);
    assert_eq!(
        client.get_max_portfolio_id_bytes(),
        super::MAX_PORTFOLIO_ID_BYTES
    );
    assert_eq!(client.get_replay_nonce(&admin, &NONCE_CHANNEL_ADMIN), 1u64);
}

#[test]
#[should_panic(expected = "already initialized")]
fn test_initialize_twice_panics() {
    let env = Env::default();
    let (client, _snap, admin, _snap_id) = setup(&env);
    client.initialize(&admin, &1u64);
}

#[test]
#[should_panic(expected = "nonce mismatch")]
fn test_initialize_wrong_nonce_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let agg_id = env.register(AggregatedAttestationsContract, ());
    let client = AggregatedAttestationsContractClient::new(&env, &agg_id);
    client.initialize(&admin, &1u64);
}

#[test]
fn test_register_portfolio() {
    let env = Env::default();
    let (client, _snap, admin, _agg_id, _snap_id) = setup(&env);
    let id = String::from_str(&env, "portfolio-1");
    let mut businesses = Vec::new(&env);
    businesses.push_back(Address::generate(&env));
    businesses.push_back(Address::generate(&env));
    let n = admin_nonce(&client, &admin);
    client.register_portfolio(&admin, &n, &id, &businesses);
    let stored = client.get_portfolio(&id).unwrap();
    assert_eq!(stored.len(), 2);
    assert_eq!(client.get_replay_nonce(&admin, &NONCE_CHANNEL_ADMIN), n + 1);
}

#[test]
#[should_panic(expected = "caller is not admin")]
fn test_register_portfolio_unauthorized() {
    let env = Env::default();
    let (client, _snap, admin, _snap_id) = setup(&env);
    let other = Address::generate(&env);
    let id = String::from_str(&env, "p1");
    let businesses = Vec::new(&env);
    let n = admin_nonce(&client, &admin);
    client.register_portfolio(&other, &n, &id, &businesses);
}

#[test]
#[should_panic(expected = "nonce mismatch")]
fn test_register_portfolio_wrong_nonce_panics() {
    let env = Env::default();
    let (client, _snap, admin, _snap_id) = setup(&env);
    let id = String::from_str(&env, "p1");
    let businesses = Vec::new(&env);
    client.register_portfolio(&admin, &0u64, &id, &businesses);
}

#[test]
#[should_panic(expected = "duplicate business in portfolio")]
fn test_register_portfolio_duplicate_address_panics() {
    let env = Env::default();
    let (client, _snap, admin, _snap_id) = setup(&env);
    let id = String::from_str(&env, "dup");
    let b = Address::generate(&env);
    let mut businesses = Vec::new(&env);
    businesses.push_back(b.clone());
    businesses.push_back(b);
    let n = admin_nonce(&client, &admin);
    client.register_portfolio(&admin, &n, &id, &businesses);
}

#[test]
#[should_panic(expected = "portfolio exceeds maximum businesses")]
fn test_register_portfolio_too_many_businesses_panics() {
    let env = Env::default();
    let (client, _snap, admin, _snap_id) = setup(&env);
    let id = String::from_str(&env, "big");
    let mut businesses = Vec::new(&env);
    for _ in 0..=MAX_PORTFOLIO_BUSINESSES {
        businesses.push_back(Address::generate(&env));
    }
    let n = admin_nonce(&client, &admin);
    client.register_portfolio(&admin, &n, &id, &businesses);
}

#[test]
#[should_panic(expected = "portfolio_id exceeds maximum length")]
fn test_register_portfolio_id_too_long_panics() {
    let env = Env::default();
    let (client, _snap, admin, _snap_id) = setup(&env);
    let long = core::str::from_utf8(&[b'z'; (super::MAX_PORTFOLIO_ID_BYTES as usize) + 1]).unwrap();
    let id = String::from_str(&env, long);
    let businesses = Vec::new(&env);
    let n = admin_nonce(&client, &admin);
    client.register_portfolio(&admin, &n, &id, &businesses);
}

#[test]
fn test_register_portfolio_at_max_id_length_ok() {
    let env = Env::default();
    let (client, _snap, admin, _snap_id) = setup(&env);
    let long = core::str::from_utf8(&[b'a'; super::MAX_PORTFOLIO_ID_BYTES as usize]).unwrap();
    let id = String::from_str(&env, long);
    let mut businesses = Vec::new(&env);
    businesses.push_back(Address::generate(&env));
    let n = admin_nonce(&client, &admin);
    client.register_portfolio(&admin, &n, &id, &businesses);
    assert_eq!(client.get_portfolio(&id).unwrap().len(), 1);
}

#[test]
fn test_register_portfolio_sequential_nonces() {
    let env = Env::default();
    let (client, _snap, admin, _snap_id) = setup(&env);
    let mut businesses = Vec::new(&env);
    businesses.push_back(Address::generate(&env));
    let n1 = admin_nonce(&client, &admin);
    client.register_portfolio(&admin, &n1, &String::from_str(&env, "a"), &businesses);
    let n2 = admin_nonce(&client, &admin);
    client.register_portfolio(&admin, &n2, &String::from_str(&env, "b"), &businesses);
    assert_eq!(client.get_replay_nonce(&admin, &NONCE_CHANNEL_ADMIN), n2 + 1);
}

#[test]
#[should_panic(expected = "duplicate business address in portfolio")]
fn test_register_portfolio_duplicate_business_panics() {
    let env = Env::default();
    let (client, _snap, admin, _agg_id, _snap_id) = setup(&env);
    let id = String::from_str(&env, "dup");
    let business = Address::generate(&env);
    let portfolio = businesses(&env, &[business.clone(), business]);

    client.register_portfolio(&admin, &id, &portfolio);
}

#[test]
fn test_failed_duplicate_registration_preserves_existing_portfolio() {
    let env = Env::default();
    let (client, _snap, admin, _agg_id, _snap_id) = setup(&env);
    let id = String::from_str(&env, "stable");
    let business_a = Address::generate(&env);
    let business_b = Address::generate(&env);
    let initial = businesses(&env, &[business_a.clone(), business_b]);
    let duplicate = businesses(&env, &[business_a.clone(), business_a]);

    client.register_portfolio(&admin, &id, &initial);
    assert!(client
        .try_register_portfolio(&admin, &id, &duplicate)
        .is_err());

    let stored = client.get_portfolio(&id).unwrap();
    assert_eq!(stored, initial);
}

#[test]
fn test_get_aggregated_metrics_empty_portfolio() {
    let env = Env::default();
    let (agg_client, _snap_client, admin, _agg_id, snap_id) = setup(&env);
    let id = String::from_str(&env, "empty");
    let businesses: Vec<Address> = Vec::new(&env);
    let n = admin_nonce(&agg_client, &admin);
    agg_client.register_portfolio(&admin, &n, &id, &businesses);
    let m = agg_client.get_aggregated_metrics(&snap_id, &id);
    assert_eq!(m.business_count, 0);
    assert_eq!(m.total_trailing_revenue, 0);
    assert_eq!(m.total_anomaly_count, 0);
    assert_eq!(m.businesses_with_snapshots, 0);
    assert_eq!(m.average_trailing_revenue, 0);
}

#[test]
fn test_get_aggregated_metrics_no_snapshots() {
    let env = Env::default();
    let (agg_client, _snap_client, admin, _agg_id, snap_id) = setup(&env);
    let id = String::from_str(&env, "no-snap");
    let mut businesses = Vec::new(&env);
    businesses.push_back(Address::generate(&env));
    let n = admin_nonce(&agg_client, &admin);
    agg_client.register_portfolio(&admin, &n, &id, &businesses);
    let m = agg_client.get_aggregated_metrics(&snap_id, &id);
    assert_eq!(m.business_count, 1);
    assert_eq!(m.businesses_with_snapshots, 0);
    assert_eq!(m.total_trailing_revenue, 0);
}

#[test]
fn test_get_aggregated_metrics_with_snapshots() {
    let env = Env::default();
    let (agg_client, snap_client, admin, _agg_id, snap_id) = setup(&env);
    let b1 = Address::generate(&env);
    let b2 = Address::generate(&env);
    let period = String::from_str(&env, "2026-02");
    snap_client.record_snapshot(&admin, &b1, &period, &100_000i128, &1u32, &1u64);
    snap_client.record_snapshot(&admin, &b2, &period, &200_000i128, &2u32, &1u64);
    let id = String::from_str(&env, "p1");
    let mut businesses = Vec::new(&env);
    businesses.push_back(b1);
    businesses.push_back(b2);
    let n = admin_nonce(&agg_client, &admin);
    agg_client.register_portfolio(&admin, &n, &id, &businesses);
    let m = agg_client.get_aggregated_metrics(&snap_id, &id);
    assert_eq!(m.business_count, 2);
    assert_eq!(m.businesses_with_snapshots, 2);
    assert_eq!(m.total_trailing_revenue, 300_000i128);
    assert_eq!(m.total_anomaly_count, 3u32);
    assert_eq!(m.average_trailing_revenue, 150_000i128);
}

#[test]
fn test_duplicate_businesses_do_not_inflate_aggregated_metrics_for_legacy_portfolios() {
    let env = Env::default();
    let (agg_client, snap_client, admin, agg_id, snap_id) = setup(&env);
    let b1 = Address::generate(&env);
    let b2 = Address::generate(&env);
    let period = String::from_str(&env, "2026-02");

    snap_client.record_snapshot(&admin, &b1, &period, &100_000i128, &2u32, &1u64);
    snap_client.record_snapshot(&admin, &b2, &period, &50_000i128, &1u32, &1u64);

    let portfolio_id = String::from_str(&env, "legacy-dup");
    let legacy_portfolio = businesses(&env, &[b1.clone(), b1, b2]);
    env.as_contract(&agg_id, || {
        env.storage()
            .instance()
            .set(&DataKey::Portfolio(portfolio_id.clone()), &legacy_portfolio);
    });

    let metrics = agg_client.get_aggregated_metrics(&snap_id, &portfolio_id);
    assert_eq!(metrics.business_count, 2);
    assert_eq!(metrics.businesses_with_snapshots, 2);
    assert_eq!(metrics.total_trailing_revenue, 150_000i128);
    assert_eq!(metrics.total_anomaly_count, 3u32);
    assert_eq!(metrics.average_trailing_revenue, 75_000i128);
}

#[test]
fn test_get_aggregated_metrics_unregistered_portfolio() {
    let env = Env::default();
    let (agg_client, _snap_client, _admin, _agg_id, snap_id) = setup(&env);
    let id = String::from_str(&env, "nonexistent");
    let m = agg_client.get_aggregated_metrics(&snap_id, &id);
    assert_eq!(m.business_count, 0);
    assert_eq!(m.total_trailing_revenue, 0);
}

#[test]
fn test_check_batch_snapshot_consistency_empty_portfolio() {
    let env = Env::default();
    let (agg_client, _snap, admin, snap_id) = setup(&env);
    let id = String::from_str(&env, "e");
    let businesses = Vec::new(&env);
    let n = admin_nonce(&agg_client, &admin);
    agg_client.register_portfolio(&admin, &n, &id, &businesses);
    assert!(agg_client.check_batch_snapshot_consistency(
        &snap_id,
        &id,
        &999u64
    ));
}

#[test]
fn test_check_batch_snapshot_consistency_all_same_recorded_at() {
    let env = Env::default();
    env.ledger().set_timestamp(60_000);
    let (agg_client, snap_client, admin, snap_id) = setup(&env);
    let b1 = Address::generate(&env);
    let b2 = Address::generate(&env);
    let p1 = String::from_str(&env, "2026-01");
    let p2 = String::from_str(&env, "2026-02");
    snap_client.record_snapshot(&admin, &b1, &p1, &10i128, &1u32, &1u64);
    snap_client.record_snapshot(&admin, &b1, &p2, &20i128, &0u32, &1u64);
    snap_client.record_snapshot(&admin, &b2, &p1, &30i128, &2u32, &1u64);
    let id = String::from_str(&env, "batch-ok");
    let mut businesses = Vec::new(&env);
    businesses.push_back(b1);
    businesses.push_back(b2);
    let n = admin_nonce(&agg_client, &admin);
    agg_client.register_portfolio(&admin, &n, &id, &businesses);
    assert!(agg_client.check_batch_snapshot_consistency(
        &snap_id,
        &id,
        &60_000u64
    ));
}

#[test]
fn test_check_batch_snapshot_consistency_mixed_recorded_at_false() {
    let env = Env::default();
    env.ledger().set_timestamp(50_000);
    let (agg_client, snap_client, admin, snap_id) = setup(&env);
    let b1 = Address::generate(&env);
    let period = String::from_str(&env, "2026-01");
    snap_client.record_snapshot(&admin, &b1, &period, &10i128, &1u32, &1u64);
    env.ledger().set_timestamp(99_000);
    snap_client.record_snapshot(&admin, &b1, &String::from_str(&env, "2026-02"), &20i128, &0u32, &1u64);
    let id = String::from_str(&env, "batch-bad");
    let mut businesses = Vec::new(&env);
    businesses.push_back(b1);
    let n = admin_nonce(&agg_client, &admin);
    agg_client.register_portfolio(&admin, &n, &id, &businesses);
    assert!(!agg_client.check_batch_snapshot_consistency(
        &snap_id,
        &id,
        &99_000u64
    ));
}

#[test]
fn test_get_aggregated_metrics_for_batch_filters_by_recorded_at() {
    let env = Env::default();
    env.ledger().set_timestamp(1_000);
    let (agg_client, snap_client, admin, snap_id) = setup(&env);
    let b1 = Address::generate(&env);
    let b2 = Address::generate(&env);
    let period = String::from_str(&env, "2026-02");
    snap_client.record_snapshot(&admin, &b1, &period, &100i128, &1u32, &1u64);
    env.ledger().set_timestamp(2_000);
    snap_client.record_snapshot(&admin, &b2, &period, &200i128, &2u32, &1u64);
    let id = String::from_str(&env, "filt");
    let mut businesses = Vec::new(&env);
    businesses.push_back(b1);
    businesses.push_back(b2);
    let n = admin_nonce(&agg_client, &admin);
    agg_client.register_portfolio(&admin, &n, &id, &businesses);

    let m_old = agg_client.get_aggregated_metrics_for_batch(&snap_id, &id, &1_000u64);
    assert_eq!(m_old.total_trailing_revenue, 100i128);
    assert_eq!(m_old.total_anomaly_count, 1u32);
    assert_eq!(m_old.businesses_with_snapshots, 1u32);
    assert_eq!(m_old.average_trailing_revenue, 100i128);

    let m_new = agg_client.get_aggregated_metrics_for_batch(&snap_id, &id, &2_000u64);
    assert_eq!(m_new.total_trailing_revenue, 200i128);
    assert_eq!(m_new.businesses_with_snapshots, 1u32);

    let m_all = agg_client.get_aggregated_metrics(&snap_id, &id);
    assert_eq!(m_all.total_trailing_revenue, 300i128);
    assert_eq!(m_all.businesses_with_snapshots, 2u32);
}

#[test]
fn test_batch_consistency_partial_business_no_snapshot_still_ok() {
    let env = Env::default();
    env.ledger().set_timestamp(77_000);
    let (agg_client, snap_client, admin, snap_id) = setup(&env);
    let b1 = Address::generate(&env);
    let b2 = Address::generate(&env);
    snap_client.record_snapshot(
        &admin,
        &b1,
        &String::from_str(&env, "p"),
        &1i128,
        &0u32,
        &1u64,
    );
    let id = String::from_str(&env, "partial");
    let mut businesses = Vec::new(&env);
    businesses.push_back(b1);
    businesses.push_back(b2);
    let n = admin_nonce(&agg_client, &admin);
    agg_client.register_portfolio(&admin, &n, &id, &businesses);
    assert!(agg_client.check_batch_snapshot_consistency(
        &snap_id,
        &id,
        &77_000u64
    ));
}

#[test]
fn test_submit_aggregated_root_success() {
    let env = Env::default();
    let (client, _snap, admin, _agg_id, _snap_id) = setup(&env);
    let id = String::from_str(&env, "p1");
    let root = soroban_sdk::BytesN::from_array(&env, &[1u8; 32]);
    
    env.ledger().set_timestamp(1000);
    client.submit_aggregated_root(&admin, &id, &root, &0u64, &500u64, &1u32);
    
    let roots = client.get_aggregated_roots(&id);
    assert_eq!(roots.len(), 1);
    let record = roots.get(0).unwrap();
    assert_eq!(record.root, root);
    assert_eq!(record.start_timestamp, 0);
    assert_eq!(record.end_timestamp, 500);
    assert_eq!(record.version, 1);
}

#[test]
#[should_panic(expected = "invalid window boundaries")]
fn test_submit_aggregated_root_invalid_boundaries() {
    let env = Env::default();
    let (client, _snap, admin, _agg_id, _snap_id) = setup(&env);
    let id = String::from_str(&env, "p1");
    let root = soroban_sdk::BytesN::from_array(&env, &[1u8; 32]);
    
    env.ledger().set_timestamp(1000);
    client.submit_aggregated_root(&admin, &id, &root, &500u64, &100u64, &1u32);
}

#[test]
#[should_panic(expected = "future window boundary")]
fn test_submit_aggregated_root_future_boundary() {
    let env = Env::default();
    let (client, _snap, admin, _agg_id, _snap_id) = setup(&env);
    let id = String::from_str(&env, "p1");
    let root = soroban_sdk::BytesN::from_array(&env, &[1u8; 32]);
    
    env.ledger().set_timestamp(1000);
    client.submit_aggregated_root(&admin, &id, &root, &0u64, &1001u64, &1u32);
}

#[test]
#[should_panic(expected = "overlapping window boundaries")]
fn test_submit_aggregated_root_overlapping_windows() {
    let env = Env::default();
    let (client, _snap, admin, _agg_id, _snap_id) = setup(&env);
    let id = String::from_str(&env, "p1");
    let root1 = soroban_sdk::BytesN::from_array(&env, &[1u8; 32]);
    let root2 = soroban_sdk::BytesN::from_array(&env, &[2u8; 32]);
    
    env.ledger().set_timestamp(2000);
    client.submit_aggregated_root(&admin, &id, &root1, &0u64, &1000u64, &1u32);
    // Overlapping [500, 1500] with [0, 1000]
    client.submit_aggregated_root(&admin, &id, &root2, &500u64, &1500u64, &1u32);
}

#[test]
fn test_submit_aggregated_root_version_bump() {
    let env = Env::default();
    let (client, _snap, admin, _agg_id, _snap_id) = setup(&env);
    let id = String::from_str(&env, "p1");
    let root1 = soroban_sdk::BytesN::from_array(&env, &[1u8; 32]);
    let root2 = soroban_sdk::BytesN::from_array(&env, &[2u8; 32]);
    
    env.ledger().set_timestamp(2000);
    client.submit_aggregated_root(&admin, &id, &root1, &0u64, &1000u64, &1u32);
    // Version bump! Allowed to overlap.
    client.submit_aggregated_root(&admin, &id, &root2, &500u64, &1500u64, &2u32);
    
    let roots = client.get_aggregated_roots(&id);
    assert_eq!(roots.len(), 2);
}

#[test]
fn test_get_aggregated_root_at_timestamp() {
    let env = Env::default();
    let (client, _snap, admin, _agg_id, _snap_id) = setup(&env);
    let id = String::from_str(&env, "p1");
    let root1 = soroban_sdk::BytesN::from_array(&env, &[1u8; 32]);
    let root2 = soroban_sdk::BytesN::from_array(&env, &[2u8; 32]);
    
    env.ledger().set_timestamp(2000);
    client.submit_aggregated_root(&admin, &id, &root1, &0u64, &1000u64, &1u32);
    client.submit_aggregated_root(&admin, &id, &root2, &500u64, &1500u64, &2u32);
    
    // At 200 -> only root1 applies
    let r1 = client.get_aggregated_root_at_timestamp(&id, &200u64).unwrap();
    assert_eq!(r1.root, root1);
    
    // At 600 -> both apply, but version 2 is higher!
    let r2 = client.get_aggregated_root_at_timestamp(&id, &600u64).unwrap();
    assert_eq!(r2.root, root2);
    assert_eq!(r2.version, 2);
    
    // At 1200 -> only root2 applies
    let r3 = client.get_aggregated_root_at_timestamp(&id, &1200u64).unwrap();
    assert_eq!(r3.root, root2);
    
    // At 1600 -> none apply
    let r4 = client.get_aggregated_root_at_timestamp(&id, &1600u64);
    assert!(r4.is_none());
}
