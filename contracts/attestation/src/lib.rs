#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, Address, BytesN, Env, String, Vec, Symbol, xdr::ToXdr};
use veritasor_attestor_staking::AttestorStakingContractClient;
use veritasor_common::replay_protection;

const STATUS_KEY_TAG: u32 = 1;
const ADMIN_KEY_TAG: (u32,) = (2,);
const ANOMALY_KEY_TAG: (u32,) = (3,);
const AUTHORIZED_KEY_TAG: (u32,) = (4,);
const ANOMALY_SCORE_MAX: u32 = 100;
pub const NONCE_CHANNEL_ADMIN: u32 = 0;
pub const NONCE_CHANNEL_BUSINESS: u32 = 1;

pub const STATUS_ACTIVE: u32 = 0;
pub const STATUS_REVOKED: u32 = 1;
pub const STATUS_FILTER_ALL: u32 = 2;

// Type aliases to reduce complexity - exported for other contracts
pub type AttestationData = (BytesN<32>, u64, u32, i128, Option<BytesN<32>>, Option<u64>);
pub type RevocationData = (Address, u64, String);
pub type AttestationWithRevocation = (AttestationData, Option<RevocationData>);
pub type AttestationStatusResult = Vec<(String, Option<AttestationData>, Option<RevocationData>)>;

pub mod access_control;
pub mod dynamic_fees;
pub mod events;
pub mod extended_metadata;
pub mod fees;
pub mod multisig;
pub mod rate_limit;
pub mod registry;
pub mod dispute;

pub use access_control::{ROLE_ADMIN, ROLE_ATTESTOR, ROLE_BUSINESS, ROLE_OPERATOR};
pub use dynamic_fees::{compute_fee, DataKey, FeeConfig};
pub use events::{AttestationMigratedEvent, AttestationRevokedEvent, AttestationSubmittedEvent};
pub use fees::{FlatFeeConfig, collect_flat_fee};
pub use multisig::{Proposal, ProposalAction, ProposalStatus};
pub use rate_limit::RateLimitConfig;
pub use registry::{BusinessRecord, BusinessStatus};
pub use dispute::{
    Dispute, DisputeOutcome, DisputeResolution, DisputeStatus, DisputeType, OptionalResolution,
};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttestationRange {
    pub start_period: u32, // Format: YYYYMM
    pub end_period: u32,   // Format: YYYYMM
    pub merkle_root: BytesN<32>,
    pub timestamp: u64,
    pub version: u32,
    pub fee_paid: i128,
    pub proof_hash: Option<BytesN<32>>,
    pub expiry_timestamp: Option<u64>,
    pub revoked: bool,
}

#[contracttype]
pub enum MultiPeriodKey {
    Ranges(Address),
}

#[contracttype]
pub struct BatchAttestationItem {
    pub business: Address,
    pub period: String,
    pub merkle_root: BytesN<32>,
    pub timestamp: u64,
    pub version: u32,
    pub expiry_timestamp: Option<u64>,
}

#[contract]
pub struct AttestationContract;

#[contractimpl]
impl AttestationContract {
    pub fn initialize(env: Env, admin: Address, _nonce: u64) {
        if dynamic_fees::is_initialized(&env) {
            panic!("already initialized");
        }
        admin.require_auth();
        dynamic_fees::set_admin(&env, &admin);
        access_control::grant_role(&env, &admin, ROLE_ADMIN);
    }

    pub fn configure_fees(
        env: Env,
        token: Address,
        collector: Address,
        base_fee: i128,
        enabled: bool,
    ) {
        dynamic_fees::require_admin(&env);
        assert!(base_fee >= 0, "base_fee must be non-negative");
        let config = FeeConfig { token, collector, base_fee, enabled };
        dynamic_fees::set_fee_config(&env, &config);
    }

    pub fn set_tier_discount(env: Env, tier: u32, discount_bps: u32) {
        dynamic_fees::require_admin(&env);
        dynamic_fees::set_tier_discount(&env, tier, discount_bps);
    }

    pub fn set_business_tier(env: Env, business: Address, tier: u32) {
        dynamic_fees::require_admin(&env);
        dynamic_fees::set_business_tier(&env, &business, tier);
    }

    pub fn set_volume_brackets(env: Env, thresholds: Vec<u64>, discounts: Vec<u32>) {
        dynamic_fees::require_admin(&env);
        dynamic_fees::set_volume_brackets(&env, &thresholds, &discounts);
    }

    pub fn set_fee_enabled(env: Env, enabled: bool) {
        dynamic_fees::require_admin(&env);
        let mut config = dynamic_fees::get_fee_config(&env).expect("fees not configured");
        config.enabled = enabled;
        dynamic_fees::set_fee_config(&env, &config);
    }

    // ── Rate Limiting ─────────────────────────────────────────────────────────

    /// Configure per-business rate limiting.
    ///
    /// Admin-only. Validates all parameters before persisting. Emits a
    /// `RateLimitConfigChanged` event on success.
    ///
    /// # Parameters
    ///
    /// * `max_submissions`       – Max attestations per full window `[1, 100]`.
    /// * `window_seconds`        – Full window duration in seconds `[1, 31_536_000]`.
    /// * `burst_max_submissions` – Max attestations per burst window `[1, max_submissions]`.
    /// * `burst_window_seconds`  – Burst window duration in seconds `[1, window_seconds]`.
    /// * `enabled`               – Master switch; `false` disables rate limiting.
    /// * `nonce`                 – Replay-protection nonce for the admin channel.
    pub fn configure_rate_limit(
        env: Env,
        max_submissions: u32,
        window_seconds: u64,
        burst_max_submissions: u32,
        burst_window_seconds: u64,
        enabled: bool,
        nonce: u64,
    ) {
        let admin = dynamic_fees::get_admin(&env);
        admin.require_auth();
        replay_protection::verify_and_increment_nonce(&env, &admin, NONCE_CHANNEL_ADMIN, nonce);

        let config = rate_limit::RateLimitConfig {
            max_submissions,
            window_seconds,
            burst_max_submissions,
            burst_window_seconds,
            enabled,
        };
        rate_limit::set_rate_limit_config(&env, &config);

        events::emit_rate_limit_config_changed(
            &env,
            max_submissions,
            window_seconds,
            burst_max_submissions,
            burst_window_seconds,
            enabled,
            &admin,
        );
    }

    /// Return the current rate limit configuration, or `None` if not set.
    pub fn get_rate_limit_config(env: Env) -> Option<RateLimitConfig> {
        rate_limit::get_rate_limit_config(&env)
    }

    /// Return how many submissions `business` has in the current full window.
    ///
    /// Returns `0` when rate limiting is not configured or is disabled.
    pub fn get_submission_window_count(env: Env, business: Address) -> u32 {
        rate_limit::get_submission_count(&env, &business)
    }

    /// Return how many submissions `business` has in the current burst window.
    ///
    /// Returns `0` when rate limiting is not configured or is disabled.
    pub fn get_submission_burst_count(env: Env, business: Address) -> u32 {
        rate_limit::get_burst_submission_count(&env, &business)
    }

    pub fn configure_flat_fee(
        env: Env,
        token: Address,
        collector: Address,
        amount: i128,
        enabled: bool,
    ) {
        dynamic_fees::require_admin(&env);
        assert!(amount >= 0, "flat fee amount must be non-negative");
        let config = FlatFeeConfig {
            token,
            collector,
            amount,
            enabled,
        };
        fees::set_flat_fee_config(&env, &config);

        let admin = dynamic_fees::get_admin(&env);
        events::emit_flat_fee_config_changed(
            &env,
            &config.token,
            &config.collector,
            config.amount,
            config.enabled,
            &admin,
        );
    }

    pub fn set_flat_fee_dao(env: Env, dao: Address) {
        dynamic_fees::require_admin(&env);
        fees::set_dao(&env, &dao);
    }

    pub fn set_attestor_staking_contract(env: Env, caller: Address, staking_contract: Address) {
        access_control::require_admin(&env, &caller);
        env.storage()
            .instance()
            .set(&DataKey::AttestorStakingContract, &staking_contract);
    }

    pub fn get_attestor_staking_contract(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::AttestorStakingContract)
    }

    pub fn grant_role(env: Env, caller: Address, account: Address, role: u32, _nonce: u64) {
        access_control::require_admin(&env, &caller);
        access_control::grant_role(&env, &account, role);
        events::emit_role_granted(&env, &account, role, &caller);
    }

    pub fn revoke_role(env: Env, caller: Address, account: Address, role: u32, _nonce: u64) {
        access_control::require_admin(&env, &caller);
        access_control::revoke_role(&env, &account, role);
        events::emit_role_revoked(&env, &account, role, &caller);
    }

    pub fn has_role(env: Env, account: Address, role: u32) -> bool {
        access_control::has_role(&env, &account, role)
    }

    pub fn submit_attestation(
        env: Env,
        business: Address,
        period: String,
        merkle_root: BytesN<32>,
        timestamp: u64,
        version: u32,
        proof_hash: Option<BytesN<32>>,
        expiry_timestamp: Option<u64>,
        nonce: u64,
    ) {
        access_control::require_not_paused(&env);
        business.require_auth();
        replay_protection::verify_and_increment_nonce(&env, &business, NONCE_CHANNEL_BUSINESS, nonce);
        rate_limit::check_rate_limit(&env, &business);

        let key = DataKey::Attestation(business.clone(), period.clone());
        if env.storage().instance().has(&key) {
            panic!("attestation already exists for this business and period");
        }
        Self::validate_expiry(&env, timestamp, expiry_timestamp);

        // Security: If a proof_hash is provided, it MUST match the canonical commitment.
        if let Some(ref provided_hash) = proof_hash {
            let expected_hash = Self::compute_commitment(
                env.clone(),
                business.clone(),
                period.clone(),
                merkle_root.clone(),
                version,
            );
            if provided_hash != &expected_hash {
                panic!("proof_hash does not match canonical commitment");
            }
        }

        let dynamic_fee = dynamic_fees::collect_fee(&env, &business);
        let flat_fee = fees::collect_flat_fee(&env, &business);
        let total_fee = dynamic_fee + flat_fee;

        dynamic_fees::increment_business_count(&env, &business);

        let data = (
            merkle_root.clone(),
            timestamp,
            version,
            total_fee,
            proof_hash.clone(),
            expiry_timestamp,
        );
        env.storage().instance().set(&key, &data);

        events::emit_attestation_submitted(
            &env,
            &business,
            &period,
            &merkle_root,
            timestamp,
            version,
            total_fee,
            &proof_hash,
            expiry_timestamp,
        );

        rate_limit::record_submission(&env, &business);
    }

    pub fn submit_attestation_as_attestor(
        env: Env,
        attestor: Address,
        business: Address,
        period: String,
        merkle_root: BytesN<32>,
        timestamp: u64,
        version: u32,
        proof_hash: Option<BytesN<32>>,
    ) {
        attestor.require_auth();
        access_control::require_not_paused(&env);
        
        if !access_control::has_role(&env, &attestor, ROLE_ATTESTOR) {
            panic!("caller is not an authorized attestor");
        }

        if let Some(staking_addr) = Self::get_attestor_staking_contract(env.clone()) {
            let staking_client = AttestorStakingContractClient::new(&env, &staking_addr);
            if !staking_client.is_eligible(&attestor) {
                panic!("attestor not eligible (insufficient stake)");
            }
        } else {
            panic!("attestor staking contract not configured");
        }

        // Security: Commitment enforcement
        if let Some(ref provided_hash) = proof_hash {
            let expected_hash = Self::compute_commitment(
                env.clone(),
                business.clone(),
                period.clone(),
                merkle_root.clone(),
                version,
            );
            if provided_hash != &expected_hash {
                panic!("proof_hash does not match canonical commitment");
            }
        }

        let key = DataKey::Attestation(business.clone(), period.clone());
        if env.storage().instance().has(&key) {
            panic!("attestation already exists");
        }

        let data: AttestationData = (
            merkle_root.clone(),
            timestamp,
            version,
            0,
            proof_hash,
            None,
        );
        env.storage().instance().set(&key, &data);
    }

    pub fn submit_attestations_batch(env: Env, items: Vec<BatchAttestationItem>) {
        if items.is_empty() {
            panic!("batch cannot be empty");
        }
        for item in items.iter() {
            let key = DataKey::Attestation(item.business.clone(), item.period.clone());
            if env.storage().instance().has(&key) {
                panic!("attestation already exists");
            }
            
            let data: AttestationData = (
                item.merkle_root,
                item.timestamp,
                item.version,
                0,
                None,
                item.expiry_timestamp,
            );
            env.storage().instance().set(&key, &data);
        }
    }

    pub fn submit_batch_as_attestor(env: Env, attestor: Address, items: Vec<BatchAttestationItem>) {
        attestor.require_auth();
        if !access_control::has_role(&env, &attestor, ROLE_ATTESTOR) {
            panic!("not authorized");
        }
        Self::submit_attestations_batch(env, items);
    }

    pub fn migrate_attestation(
        env: Env,
        admin: Address,
        business: Address,
        period: String,
        new_merkle_root: BytesN<32>,
        new_version: u32,
        _nonce: u64,
    ) {
        // TODO: implement migration logic
        unimplemented!("migrate_attestation not implemented");
    }

    /// Submit a revenue attestation with extended metadata (currency and net/gross).
    ///
    /// Same as `submit_attestation` but also stores currency code and revenue basis.
    /// * `currency_code` – ISO 4217-style code, e.g. "USD", "EUR". Alphabetic, max 3 chars.
    /// * `is_net` – `true` for net revenue, `false` for gross revenue.


    pub fn submit_multi_period_attestation(
        env: Env,
        business: Address,
        start_period: u32,
        end_period: u32,
        merkle_root: BytesN<32>,
        timestamp: u64,
        version: u32,
        proof_hash: Option<BytesN<32>>,
        expiry_timestamp: Option<u64>,
    ) {
        business.require_auth();

        if start_period > end_period {
            panic!("start_period must be <= end_period");
        }

        if let Some(ref provided_hash) = proof_hash {
            let period_str = String::from_str(&env, "MULTI:");
            let expected_hash = Self::compute_commitment(
                env.clone(),
                business.clone(),
                period_str,
                merkle_root.clone(),
                version,
            );
            if provided_hash != &expected_hash {
                panic!("proof_hash does not match canonical commitment");
            }
        }

        let key = MultiPeriodKey::Ranges(business.clone());
        let mut ranges: Vec<AttestationRange> = env
            .storage()
            .instance()
            .get(&key)
            .unwrap_or(Vec::new(&env));

        for range in ranges.iter() {
            if !range.revoked {
                if start_period <= range.end_period && end_period >= range.start_period {
                    panic!("overlapping attestation range detected");
                }
            }
        }

        let fee_paid = dynamic_fees::collect_fee(&env, &business);
        dynamic_fees::increment_business_count(&env, &business);

        ranges.push_back(AttestationRange {
            start_period,
            end_period,
            merkle_root: merkle_root.clone(),
            timestamp,
            version,
            fee_paid,
            proof_hash,
            expiry_timestamp,
            revoked: false,
        });

        env.storage().instance().set(&key, &ranges);

        let topics = (Symbol::new(&env, "attestation"), Symbol::new(&env, "multi_period_issued"), business.clone());
        env.events().publish(topics, (start_period, end_period, merkle_root));
    }

    pub fn revoke_multi_period_attestation(
        env: Env,
        business: Address,
        merkle_root: BytesN<32>,
    ) {
        business.require_auth();

        let key = MultiPeriodKey::Ranges(business.clone());
        let ranges: Vec<AttestationRange> = env
            .storage()
            .instance()
            .get(&key)
            .unwrap_or_else(|| panic!("no multi-period attestations found"));

        let mut found = false;
        let mut updated_ranges = Vec::new(&env);

        for mut range in ranges.iter() {
            if range.merkle_root == merkle_root {
                range.revoked = true;
                found = true;
            }
            updated_ranges.push_back(range);
        }

        if !found {
            panic!("attestation root not found");
        }

        env.storage().instance().set(&key, &updated_ranges);
    }

    pub fn get_attestation(
        env: Env,
        business: Address,
        period: String,
    ) -> Option<AttestationData> {
        let key = DataKey::Attestation(business, period);
        env.storage().instance().get(&key)
    }

    pub fn get_proof_hash(env: Env, business: Address, period: String) -> Option<BytesN<32>> {
        let record = Self::get_attestation(env, business, period);
        record.and_then(|(_, _, _, _, ph, _)| ph)
    }



    pub fn get_attestation_for_period(
        env: Env,
        business: Address,
        target_period: u32,
    ) -> Option<AttestationRange> {
        let key = MultiPeriodKey::Ranges(business);
        if let Some(ranges) = env.storage().instance().get::<_, Vec<AttestationRange>>(&key) {
            for range in ranges.iter() {
                if !range.revoked 
                    && target_period >= range.start_period 
                    && target_period <= range.end_period 
                {
                    return Some(range);
                }
            }
        }
        None
    }

    pub fn verify_multi_period_attestation(
        env: Env,
        business: Address,
        target_period: u32,
        merkle_root: BytesN<32>,
    ) -> bool {
        if let Some(range) = Self::get_attestation_for_period(env.clone(), business.clone(), target_period) {
            if range.revoked {
                return false;
            }
            range.merkle_root == merkle_root
        } else {
            false
        }
    }


    pub fn init(env: Env, admin: Address) {
        admin.require_auth();
        if env.storage().instance().has(&ADMIN_KEY_TAG) {
            panic!("admin already set");
        }
        env.storage().instance().set(&ADMIN_KEY_TAG, &admin);
    }

    pub fn add_authorized_analytics(env: Env, caller: Address, analytics: Address) {
        caller.require_auth();
        let admin: Address = env
            .storage()
            .instance()
            .get(&ADMIN_KEY_TAG)
            .expect("admin not set");
        if caller != admin {
            panic!("caller is not admin");
        }
        let key = (AUTHORIZED_KEY_TAG, analytics);
        env.storage().instance().set(&key, &());
    }

    pub fn remove_authorized_analytics(env: Env, caller: Address, analytics: Address) {
        caller.require_auth();
        let admin: Address = env
            .storage()
            .instance()
            .get(&ADMIN_KEY_TAG)
            .expect("admin not set");
        if caller != admin {
            panic!("caller is not admin");
        }
        let key = (AUTHORIZED_KEY_TAG, analytics);
        env.storage().instance().remove(&key);
    }

    pub fn set_anomaly(
        env: Env,
        updater: Address,
        business: Address,
        period: String,
        flags: u32,
        score: u32,
    ) {
        updater.require_auth();
        let key_auth = (AUTHORIZED_KEY_TAG, updater.clone());
        if !env.storage().instance().has(&key_auth) {
            panic!("updater not authorized");
        }
        let attest_key = DataKey::Attestation(business.clone(), period.clone());
        if !env.storage().instance().has(&attest_key) {
            panic!("attestation does not exist for this business and period");
        }
        if score > ANOMALY_SCORE_MAX {
            panic!("score out of range");
        }
        let anomaly_key = (ANOMALY_KEY_TAG, business.clone(), period.clone());
        env.storage().instance().set(&anomaly_key, &(flags, score));
        dispute::update_anomaly_escalation(&env, &business, score);
    }

    pub fn get_anomaly(env: Env, business: Address, period: String) -> Option<(u32, u32)> {
        let key = (ANOMALY_KEY_TAG, business.clone(), period);
        env.storage().instance().get(&key)
    }

    pub fn get_anomaly_escalation(env: Env, business: Address) -> Option<u32> {
        dispute::get_anomaly_escalation(&env, &business)
    }

    pub fn clear_anomaly_escalation(env: Env, caller: Address, business: Address) {
        caller.require_auth();
        let admin: Address = env
            .storage()
            .instance()
            .get(&ADMIN_KEY_TAG)
            .expect("admin not set");
        if caller != admin {
            panic!("caller is not admin");
        }
        dispute::clear_anomaly_escalation(&env, &business);
    }

    // --- Registry Methods ---
    pub fn register_business(
        env: Env,
        business: Address,
        name_hash: BytesN<32>,
        jurisdiction: Symbol,
        tags: Vec<Symbol>,
    ) {
        registry::register_business(&env, &business, name_hash, jurisdiction, tags);
    }

    pub fn approve_business(env: Env, caller: Address, business: Address) {
        registry::approve_business(&env, &caller, &business);
    }

    pub fn suspend_business(env: Env, caller: Address, business: Address, reason: Symbol) {
        registry::suspend_business(&env, &caller, &business, reason);
    }

    pub fn reactivate_business(env: Env, caller: Address, business: Address) {
        registry::reactivate_business(&env, &caller, &business);
    }

    pub fn update_tags(env: Env, caller: Address, business: Address, tags: Vec<Symbol>) {
        registry::update_tags(&env, &caller, &business, tags);
    }

    pub fn is_business_active(env: Env, business: Address) -> bool {
        registry::is_active(&env, &business)
    }

    pub fn get_business(env: Env, business: Address) -> Option<BusinessRecord> {
        registry::get_business(&env, &business)
    }

    // --- Extended Metadata Methods ---
    pub fn get_attestation_metadata(
        env: Env,
        business: Address,
        period: String,
    ) -> Option<extended_metadata::AttestationMetadata> {
        extended_metadata::get_metadata(&env, &business, &period)
    }

    pub fn submit_attestation_with_metadata(
        env: Env,
        business: Address,
        period: String,
        merkle_root: BytesN<32>,
        timestamp: u64,
        version: u32,
        currency_code: String,
        is_net: bool,
        nonce: u64,
    ) {
        Self::submit_attestation(
            env.clone(),
            business.clone(),
            period.clone(),
            merkle_root,
            timestamp,
            version,
            None,
            None,
            nonce,
        );
        let meta = extended_metadata::validate_metadata(&env, &currency_code, is_net);
        extended_metadata::set_metadata(&env, &business, &period, &meta);
    }

    pub fn get_business_attestations(
        env: Env,
        business: Address,
        periods: Vec<String>,
    ) -> AttestationStatusResult {
        let mut results = Vec::new(&env);
        for period in periods.iter() {
            let attestation = Self::get_attestation(env.clone(), business.clone(), period.clone());
            let revocation = dispute::get_attestation_revocation(&env, &business, &period);
            results.push_back((period, attestation, revocation));
        }
        results
    }

    pub fn get_attestations_page(
        env: Env,
        business: Address,
        periods: Vec<String>,
        start_period: Option<String>,
        end_period: Option<String>,
        status_filter: u32,
        version_filter: Option<u32>,
        limit: u32,
        cursor: u32,
    ) -> (Vec<(String, BytesN<32>, u64, u32, u32)>, u32) {
        let mut results = Vec::new(&env);
        let mut current_cursor = cursor;
        let mut count = 0;
        let max_limit = 30;
        let actual_limit = if limit > max_limit { max_limit } else { limit };

        while current_cursor < periods.len() && count < actual_limit {
            let period = periods.get(current_cursor).unwrap();
            
            if let Some(ref start) = start_period {
                if period < *start {
                    current_cursor += 1;
                    continue;
                }
            }
            if let Some(ref end) = end_period {
                if period > *end {
                    current_cursor += 1;
                    continue;
                }
            }

            if let Some((root, ts, ver, _fee, _ph, _exp)) = Self::get_attestation(env.clone(), business.clone(), period.clone()) {
                if let Some(v_filter) = version_filter {
                    if ver != v_filter {
                        current_cursor += 1;
                        continue;
                    }
                }

                let revocation = dispute::get_attestation_revocation(&env, &business, &period);
                let status = if revocation.is_some() { STATUS_REVOKED } else { STATUS_ACTIVE };
                
                if status_filter != STATUS_FILTER_ALL && status != status_filter {
                    current_cursor += 1;
                    continue;
                }

                results.push_back((period, root, ts, ver, status));
                count += 1;
            }
            current_cursor += 1;
        }

        (results, current_cursor)
    }

    pub fn compute_commitment(
        env: Env,
        business: Address,
        period: String,
        merkle_root: BytesN<32>,
        version: u32,
    ) -> BytesN<32> {
        let mut buf = soroban_sdk::Bytes::new(&env);
        buf.append(&business.to_xdr(&env));
        buf.append(&period.to_xdr(&env));
        buf.append(&merkle_root.to_xdr(&env));
        
        let mut ver_buf = [0u8; 4];
        for (i, byte) in version.to_be_bytes().iter().enumerate() {
            ver_buf[i] = *byte;
        }
        buf.append(&soroban_sdk::Bytes::from_array(&env, &ver_buf));

        env.crypto().sha256(&buf).into()
    }

    pub fn get_flat_fee_config(env: Env) -> Option<FlatFeeConfig> {
        fees::get_flat_fee_config(&env)
    }

    pub fn get_admin(env: Env) -> Address {
        dynamic_fees::get_admin(&env)
    }

    pub fn pause(env: Env, caller: Address, _nonce: u64) {
        caller.require_auth();
        access_control::set_paused(&env, true);
        events::emit_paused(&env, &caller);
    }

    pub fn unpause(env: Env, caller: Address, _nonce: u64) {
        caller.require_auth();
        access_control::set_paused(&env, false);
        events::emit_unpaused(&env, &caller);
    }

    pub fn is_paused(env: Env) -> bool {
        access_control::is_paused(&env)
    }

    fn validate_expiry(env: &Env, timestamp: u64, expiry_timestamp: Option<u64>) {
        if let Some(expiry) = expiry_timestamp {
            if expiry <= timestamp {
                panic!("expiry_timestamp must be greater than timestamp");
            }
            if expiry <= env.ledger().timestamp() {
                panic!("attestation expired on arrival");
            }
        }
    }

    pub fn get_fee_quote(env: Env, business: Address) -> i128 {
        dynamic_fees::compute_fee(&env, &business)
    }

    pub fn get_business_count(env: Env, business: Address) -> u64 {
        dynamic_fees::get_business_count(&env, &business)
    }

    pub fn get_business_tier(env: Env, business: Address) -> u32 {
        dynamic_fees::get_business_tier(&env, &business)
    }

    pub fn get_replay_nonce(env: Env, business: Address, channel: u32) -> u64 {
        replay_protection::get_nonce(&env, &business, channel)
    }

    pub fn open_dispute(
        env: Env,
        challenger: Address,
        business: Address,
        period: String,
        dispute_type: DisputeType,
        evidence: String,
    ) -> u64 {
        challenger.require_auth();
        dispute::validate_dispute_eligibility(&env, &challenger, &business, &period).unwrap();

        let id = dispute::generate_dispute_id(&env);
        let dispute_record = Dispute {
            id,
            challenger: challenger.clone(),
            business: business.clone(),
            period: period.clone(),
            status: DisputeStatus::Open,
            dispute_type,
            evidence,
            timestamp: env.ledger().timestamp(),
            resolution: OptionalResolution::None,
        };

        dispute::store_dispute(&env, &dispute_record);
        dispute::add_dispute_to_attestation_index(&env, &business, &period, id);
        dispute::add_dispute_to_challenger_index(&env, &challenger, id);

        id
    }

    pub fn resolve_dispute(
        env: Env,
        dispute_id: u64,
        resolver: Address,
        outcome: DisputeOutcome,
        notes: String,
    ) {
        resolver.require_auth();
        let mut dispute_record = dispute::validate_dispute_resolution(&env, dispute_id, &resolver).unwrap();

        let resolution = DisputeResolution {
            resolver: resolver.clone(),
            outcome,
            timestamp: env.ledger().timestamp(),
            notes,
        };

        dispute_record.status = DisputeStatus::Resolved;
        dispute_record.resolution = OptionalResolution::Some(resolution.clone());

        dispute::store_dispute(&env, &dispute_record);
        dispute::store_dispute_resolution(&env, dispute_id, &resolution);
    }

    pub fn close_dispute(env: Env, dispute_id: u64) {
        let mut dispute_record = dispute::validate_dispute_closure(&env, dispute_id).unwrap();

        dispute_record.status = DisputeStatus::Closed;
        dispute::store_dispute(&env, &dispute_record);
    }

    pub fn get_dispute(env: Env, dispute_id: u64) -> Option<Dispute> {
        dispute::get_dispute(&env, dispute_id)
    }

    pub fn get_disputes_by_attestation(env: Env, business: Address, period: String) -> Vec<u64> {
        dispute::get_dispute_ids_by_attestation(&env, &business, &period)
    }

    pub fn get_disputes_by_challenger(env: Env, challenger: Address) -> Vec<u64> {
        dispute::get_dispute_ids_by_challenger(&env, &challenger)
    }

    pub fn initialize_multisig(env: Env, owners: Vec<Address>, threshold: u32, _nonce: u64) {
        multisig::initialize_multisig(&env, &owners, threshold, _nonce);
    }

    pub fn get_multisig_owners(env: Env) -> Vec<Address> {
        multisig::get_owners(&env)
    }

    pub fn get_multisig_threshold(env: Env) -> u32 {
        multisig::get_threshold(&env)
    }

    pub fn is_multisig_owner(env: Env, address: Address) -> bool {
        multisig::is_owner(&env, &address)
    }

    pub fn create_proposal(
        env: Env,
        proposer: Address,
        action: ProposalAction,
        _nonce: u64,
    ) -> u64 {
        multisig::create_proposal(&env, &proposer, action)
    }

    pub fn get_proposal(env: Env, id: u64) -> Option<Proposal> {
        multisig::get_proposal(&env, id)
    }

    pub fn approve_proposal(env: Env, approver: Address, id: u64, _nonce: u64) {
        multisig::approve_proposal(&env, &approver, id)
    }

    pub fn reject_proposal(env: Env, rejecter: Address, id: u64, _nonce: u64) {
        multisig::reject_proposal(&env, &rejecter, id)
    }

    pub fn is_proposal_approved(env: Env, id: u64) -> bool {
        multisig::is_proposal_approved(&env, id)
    }

    pub fn get_approval_count(env: Env, id: u64) -> u32 {
        multisig::get_approval_count(&env, id)
    }

    pub fn execute_proposal(env: Env, executor: Address, proposal_id: u64, _nonce: u64) {
        multisig::require_owner(&env, &executor);
        let proposal = multisig::get_proposal(&env, proposal_id).expect("proposal not found");
        multisig::mark_executed(&env, proposal_id);

        match proposal.action {
            ProposalAction::Pause => {
                access_control::set_paused(&env, true);
                events::emit_paused(&env, &executor);
            }
            ProposalAction::Unpause => {
                access_control::set_paused(&env, false);
                events::emit_unpaused(&env, &executor);
            }
            ProposalAction::AddOwner(new_owner) => {
                let mut owners = multisig::get_owners(&env);
                if !owners.contains(&new_owner) {
                    owners.push_back(new_owner);
                    multisig::set_owners(&env, &owners);
                }
            }
            ProposalAction::RemoveOwner(owner_to_remove) => {
                let mut owners = multisig::get_owners(&env);
                if let Some(index) = owners.first_index_of(&owner_to_remove) {
                    owners.remove(index);
                    multisig::set_owners(&env, &owners);
                }
            }
            ProposalAction::ChangeThreshold(new_threshold) => {
                let owners_len = multisig::get_owners(&env).len();
                assert!(new_threshold > 0 && new_threshold <= owners_len, "invalid threshold");
                env.storage().instance().set(&multisig::MultisigKey::Threshold, &new_threshold);
            }
            ProposalAction::GrantRole(account, role) => {
                access_control::grant_role(&env, &account, role);
                events::emit_role_granted(&env, &account, role, &executor);
            }
            ProposalAction::RevokeRole(account, role) => {
                access_control::revoke_role(&env, &account, role);
                events::emit_role_revoked(&env, &account, role, &executor);
            }
            ProposalAction::UpdateFeeConfig(token, collector, base_fee, enabled) => {
                let config = dynamic_fees::FeeConfig { token, collector, base_fee, enabled };
                dynamic_fees::set_fee_config(&env, &config);
            }
            ProposalAction::EmergencyRotateAdmin(new_admin) => {
                let old_admin = dynamic_fees::get_admin(&env);
                dynamic_fees::set_admin(&env, &new_admin);
                // Grant admin role to new admin and revoke from old admin
                access_control::grant_role(&env, &new_admin, ROLE_ADMIN);
                let old_roles = access_control::get_roles(&env, &old_admin);
                access_control::set_roles(&env, &old_admin, old_roles & !ROLE_ADMIN);
                // Clear any pending planned rotation
                env.storage().instance().remove(&KeyRotationStorageKey::Pending);
                // Record history
                let seq = env.ledger().sequence();
                let entry = KeyRotationHistoryEntry {
                    old_admin: old_admin.clone(),
                    new_admin: new_admin.clone(),
                    sequence: seq,
                    is_emergency: true,
                };
                let mut history: Vec<KeyRotationHistoryEntry> = env
                    .storage()
                    .instance()
                    .get(&KeyRotationStorageKey::History)
                    .unwrap_or_else(|| Vec::new(&env));
                history.push_back(entry);
                env.storage().instance().set(&KeyRotationStorageKey::History, &history);
                events::emit_key_rotation_confirmed(&env, &old_admin, &new_admin, true);
            }
        }
    }

    pub fn revoke_attestation(
        env: Env,
        caller: Address,
        business: Address,
        period: String,
        reason: String,
        _nonce: u64,
    ) {
        caller.require_auth();
        access_control::require_not_paused(&env);

        let is_business = caller == business;
        let is_admin = access_control::has_role(&env, &caller, ROLE_ADMIN);
        
        if !is_business && !is_admin {
            panic!("caller must be the business or an admin");
        }

        let key = DataKey::Attestation(business.clone(), period.clone());
        if !env.storage().instance().has(&key) {
            panic!("attestation not found");
        }

        let rev_key = DataKey::Revoked(business.clone(), period.clone());
        if env.storage().instance().has(&rev_key) {
            panic!("attestation already revoked");
        }

        let revocation_data: (Address, u64, String) = (caller.clone(), env.ledger().timestamp(), reason.clone());
        env.storage().instance().set(&rev_key, &revocation_data);

        events::emit_attestation_revoked(&env, &business, &period, &reason, &caller);
    }

    pub fn is_revoked(env: Env, business: Address, period: String) -> bool {
        let rev_key = DataKey::Revoked(business, period);
        env.storage().instance().has(&rev_key)
    }

    pub fn get_revocation_info(env: Env, business: Address, period: String) -> Option<(Address, u64, String)> {
        let rev_key = DataKey::Revoked(business, period);
        env.storage().instance().get(&rev_key)
    }

    pub fn is_expired(env: Env, business: Address, period: String) -> bool {
        let key = DataKey::Attestation(business, period);
        if let Some(att) = env.storage().instance().get::<_, AttestationData>(&key) {
            if let Some(expiry) = att.5 {
                return env.ledger().timestamp() >= expiry;
            }
        }
        false
    }

    pub fn verify_attestation(env: Env, business: Address, period: String, root: BytesN<32>) -> bool {
        let key = DataKey::Attestation(business.clone(), period.clone());
        if let Some(att) = env.storage().instance().get::<_, AttestationData>(&key) {
            if att.0 != root {
                return false;
            }
            if Self::is_revoked(env.clone(), business.clone(), period.clone()) {
                return false;
            }
            if Self::is_expired(env.clone(), business, period) {
                return false;
            }
            return true;
        }
        false
    }

    // ─────────────────────────────────────────────────────────────────
    //  Key Rotation (planned + emergency via multisig)
    // ─────────────────────────────────────────────────────────────────

    /// Proposed key-rotation record stored in instance storage.
    pub fn configure_key_rotation(
        env: Env,
        timelock_ledgers: u32,
        confirmation_window_ledgers: u32,
        cooldown_ledgers: u32,
        max_rotations: u32,
    ) {
        dynamic_fees::require_admin(&env);
        let config = KeyRotationConfig {
            timelock_ledgers,
            confirmation_window_ledgers,
            cooldown_ledgers,
            max_rotations,
        };
        env.storage().instance().set(&KeyRotationStorageKey::Config, &config);
    }

    pub fn get_key_rotation_config(env: Env) -> KeyRotationConfig {
        env.storage()
            .instance()
            .get(&KeyRotationStorageKey::Config)
            .unwrap_or(KeyRotationConfig {
                timelock_ledgers: 17_280,
                confirmation_window_ledgers: 34_560,
                cooldown_ledgers: 8_640,
                max_rotations: u32::MAX,
            })
    }

    pub fn propose_key_rotation(env: Env, new_admin: Address) {
        let old_admin = dynamic_fees::get_admin(&env);
        old_admin.require_auth();
        let config = Self::get_key_rotation_config(env.clone());
        let current_seq = env.ledger().sequence();
        let pending = PendingKeyRotation {
            old_admin: old_admin.clone(),
            new_admin: new_admin.clone(),
            timelock_until: current_seq + config.timelock_ledgers,
            expires_at: current_seq + config.timelock_ledgers + config.confirmation_window_ledgers,
        };
        env.storage().instance().set(&KeyRotationStorageKey::Pending, &pending);
        events::emit_key_rotation_proposed(
            &env,
            &old_admin,
            &new_admin,
            pending.timelock_until,
            pending.expires_at,
        );
    }

    pub fn confirm_key_rotation(env: Env, new_admin: Address) {
        let pending: PendingKeyRotation = env
            .storage()
            .instance()
            .get(&KeyRotationStorageKey::Pending)
            .expect("no pending key rotation");
        assert_eq!(pending.new_admin, new_admin, "new_admin mismatch");
        new_admin.require_auth();
        let seq = env.ledger().sequence();
        assert!(seq >= pending.timelock_until, "timelock has not elapsed");
        assert!(seq <= pending.expires_at, "rotation window expired");

        // Transfer admin
        let old_admin = pending.old_admin.clone();
        dynamic_fees::set_admin(&env, &new_admin);
        access_control::grant_role(&env, &new_admin, ROLE_ADMIN);
        // Revoke old admin role
        let old_roles = access_control::get_roles(&env, &old_admin);
        access_control::set_roles(&env, &old_admin, old_roles & !ROLE_ADMIN);

        // Clear pending
        env.storage().instance().remove(&KeyRotationStorageKey::Pending);

        // Record history
        let entry = KeyRotationHistoryEntry {
            old_admin: old_admin.clone(),
            new_admin: new_admin.clone(),
            sequence: seq,
            is_emergency: false,
        };
        let mut history: Vec<KeyRotationHistoryEntry> = env
            .storage()
            .instance()
            .get(&KeyRotationStorageKey::History)
            .unwrap_or_else(|| Vec::new(&env));
        history.push_back(entry);
        env.storage().instance().set(&KeyRotationStorageKey::History, &history);

        events::emit_key_rotation_confirmed(&env, &old_admin, &new_admin, false);
    }

    pub fn cancel_key_rotation(env: Env) {
        let pending: PendingKeyRotation = env
            .storage()
            .instance()
            .get(&KeyRotationStorageKey::Pending)
            .expect("no pending key rotation");
        pending.old_admin.require_auth();
        env.storage().instance().remove(&KeyRotationStorageKey::Pending);
        events::emit_key_rotation_cancelled(&env, &pending.old_admin, &pending.new_admin);
    }

    pub fn has_pending_key_rotation(env: Env) -> bool {
        env.storage()
            .instance()
            .has(&KeyRotationStorageKey::Pending)
    }

    pub fn get_pending_key_rotation(env: Env) -> Option<PendingKeyRotation> {
        env.storage()
            .instance()
            .get(&KeyRotationStorageKey::Pending)
    }

    pub fn get_key_rotation_count(env: Env) -> u32 {
        let history: Vec<KeyRotationHistoryEntry> = env
            .storage()
            .instance()
            .get(&KeyRotationStorageKey::History)
            .unwrap_or_else(|| Vec::new(&env));
        history.len()
    }

    pub fn get_key_rotation_history(env: Env) -> Vec<KeyRotationHistoryEntry> {
        env.storage()
            .instance()
            .get(&KeyRotationStorageKey::History)
            .unwrap_or_else(|| Vec::new(&env))
    }
}

// ────────────────────────────────────────────────────────────────────
//  Key Rotation Supporting Types
// ────────────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone, Debug)]
pub struct KeyRotationConfig {
    pub timelock_ledgers: u32,
    pub confirmation_window_ledgers: u32,
    pub cooldown_ledgers: u32,
    pub max_rotations: u32,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct PendingKeyRotation {
    pub old_admin: Address,
    pub new_admin: Address,
    pub timelock_until: u32,
    pub expires_at: u32,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct KeyRotationHistoryEntry {
    pub old_admin: Address,
    pub new_admin: Address,
    pub sequence: u32,
    pub is_emergency: bool,
}

#[contracttype]
#[derive(Clone)]
enum KeyRotationStorageKey {
    Config,
    Pending,
    History,
}

#[cfg(test)]
mod proof_hash_test;
#[cfg(test)]
mod property_test;
#[cfg(test)]
mod dynamic_fees_test;
#[cfg(test)]
mod fees_test;
#[cfg(test)]
mod rate_limit_test;
#[cfg(test)]
mod access_control_test;
#[cfg(test)]
mod anomaly_test;
#[cfg(test)]
mod attestor_staking_integration_test;
#[cfg(test)]
mod batch_submission_test;
#[cfg(test)]
mod dispute_test;
#[cfg(test)]
mod events_test;
#[cfg(test)]
mod multisig_test;
#[cfg(test)]
mod registry_test;
#[cfg(test)]
mod extended_metadata_test;
#[cfg(test)]
mod multi_period_test;
#[cfg(test)]
mod pause_test;
#[cfg(test)]
mod key_rotation_test;
#[cfg(test)]
mod gas_benchmark_test;
#[cfg(test)]
mod query_pagination_test;
#[cfg(test)]
mod expiry_test;
#[cfg(test)]
mod revocation_test;
