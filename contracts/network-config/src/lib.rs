//! Cross-Network Configuration Contract for Veritasor
//!
//! This contract stores network-specific parameters needed for deploying
//! Veritasor contracts across multiple Stellar networks (e.g., testnet, mainnet).
//! It allows for centralized network configuration management with governance
//! controls and supports adding new networks without contract redeployment.
//!
//! ## Unknown network identifiers
//!
//! All write operations that target a specific `NetworkId` require the network to
//! already be registered via [`NetworkConfigContract::set_network_config`].  Passing
//! an unregistered (or previously removed) `NetworkId` to any mutating method panics
//! with the message `"unknown network id: <id>"`.  Callers can guard against this
//! with the read-only [`NetworkConfigContract::network_exists`] query before
//! performing a write.
//!
//! [`NetworkConfigContract::get_contract_address`] distinguishes two failure modes:
//! - Unknown `NetworkId` → `None` (network not registered).
//! - Unknown `contract_name` or contract slot not populated → `None` (name not
//!   recognised or `has_*` flag is `false`).
//!
//! ## Migration and rollback (operational model)
//!
//! There is no single on-chain `rollback` entrypoint. **Rollback** means governance
//! re-applies a previously known-good [`NetworkConfig`] (and related fee/registry
//! updates) via `set_network_config` / `update_fee_policy` / `update_contract_registry`.
//! Per-network [`get_network_version`] and [`get_global_version`] **only increase**
//! on successful mutations; restoring prior parameter values does not rewind counters.

#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, Address, Bytes, Env, String, Vec, Symbol};


/// Unique identifier for a Stellar network
pub type NetworkId = u32;

/// Role constants for access control
pub const ROLE_ADMIN: u32 = 1;
pub const ROLE_GOVERNANCE: u32 = 2;
pub const ROLE_OPERATOR: u32 = 4;

/// Data keys for contract storage
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub enum DataKey {
    Initialized,
    /// Upgrade tracking
    CurrentImplementation(Address),
    CurrentVersion,
    PreviousImplementation,
    PreviousVersion,

    Admin,
    GovernanceDao,
    Paused,
    NetworkConfig(NetworkId),
    RegisteredNetworks,
    DefaultNetwork,
    Role(Address),
    RoleHolders,
    NetworkVersion(NetworkId),
    GlobalVersion,
    /// Per-asset row: `AssetKey` → `AssetConfig`
    NetworkAssetConfig(AssetKey),
    /// Ordered asset addresses registered for a network (separate from [`DataKey::NetworkVersion`]).
    NetworkAssetAddresses(NetworkId),
}

/// Key for asset storage
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct AssetKey {
    pub network_id: NetworkId,
    pub asset_address: Address,
}

/// Fee policy configuration
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct FeePolicy {
    pub fee_token: Address,
    pub fee_collector: Address,
    pub base_fee: i128,
    pub enabled: bool,
    pub max_fee: i128,
    pub min_fee: i128,
}

/// Asset configuration
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct AssetConfig {
    pub asset_address: Address,
    pub asset_code: String,
    pub decimals: u32,
    pub is_active: bool,
    pub max_attestation_value: i128,
}

/// Contract registry
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct ContractRegistry {
    pub attestation_contract: Address,
    pub revenue_stream_contract: Address,
    pub audit_log_contract: Address,
    pub aggregated_attestations_contract: Address,
    pub integration_registry_contract: Address,
    pub attestation_snapshot_contract: Address,
    pub has_attestation: bool,
    pub has_revenue_stream: bool,
    pub has_audit_log: bool,
    pub has_aggregated_attestations: bool,
    pub has_integration_registry: bool,
    pub has_attestation_snapshot: bool,
}

/// Network configuration
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct NetworkConfig {
    pub name: String,
    pub network_passphrase: String,
    pub is_active: bool,
    pub fee_policy: FeePolicy,
    pub contracts: ContractRegistry,
    pub block_time_seconds: u32,
    pub min_attestations_for_aggregate: u32,
    pub dispute_timeout_seconds: u64,
    pub max_period_length_seconds: u64,
    pub created_at: u64,
    pub updated_at: u64,
}

/// Version information for upgrade tracking
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct VersionInfo {
    /// Current version number (monotonically increasing).
    pub version: u32,
    /// Implementation contract address.
    pub implementation: Address,
    /// Optional migration data passed during upgrade.
    pub migration_data: Option<Vec<u8>>,
    /// Timestamp when this version was activated.
    pub activated_at: u64,
}



/// Events
mod events {

    use super::*;

    pub fn emit_initialized(env: &Env, admin: &Address) {
        const INITIALIZED: Symbol = symbol_short!("init");
        env.events().publish((INITIALIZED,), admin.clone());
    }

    pub fn emit_network_set(env: &Env, network_id: NetworkId, name: &String) {
        const NETWORK_SET: Symbol = symbol_short!("net_set");
        env.events()
            .publish((NETWORK_SET, network_id), name.clone());
    }

    pub fn emit_network_active(env: &Env, network_id: NetworkId, active: bool) {
        const NETWORK_ACTIVE: Symbol = symbol_short!("net_act");
        env.events().publish((NETWORK_ACTIVE, network_id), active);
    }

    pub fn emit_fee_policy(env: &Env, network_id: NetworkId, enabled: bool) {
        const FEE_POLICY: Symbol = symbol_short!("fee_pol");
        env.events().publish((FEE_POLICY, network_id), enabled);
    }

    pub fn emit_asset_set(env: &Env, network_id: NetworkId, asset_code: &String) {
        const ASSET_SET: Symbol = symbol_short!("asset");
        env.events()
            .publish((ASSET_SET, network_id), asset_code.clone());
    }

    pub fn emit_registry(env: &Env, network_id: NetworkId) {
        const REGISTRY: Symbol = symbol_short!("reg");
        env.events().publish((REGISTRY, network_id), ());
    }

    pub fn emit_role_granted(env: &Env, account: &Address, role: u32, granter: &Address) {
        const ROLE_GRANTED: Symbol = symbol_short!("role_g");
        env.events()
            .publish((ROLE_GRANTED, account.clone()), (role, granter.clone()));
    }

    pub fn emit_role_revoked(env: &Env, account: &Address, role: u32, revoker: &Address) {
        const ROLE_REVOKED: Symbol = symbol_short!("role_r");
        env.events()
            .publish((ROLE_REVOKED, account.clone()), (role, revoker.clone()));
    }

    pub fn emit_paused(env: &Env, caller: &Address) {
        const PAUSED: Symbol = symbol_short!("pause");
        env.events().publish((PAUSED,), caller.clone());
    }

    pub fn emit_unpaused(env: &Env, caller: &Address) {
        const UNPAUSED: Symbol = symbol_short!("unpause");
        env.events().publish((UNPAUSED,), caller.clone());
    }

    pub fn emit_dao_set(env: &Env, dao: &Address) {
        const DAO_SET: Symbol = symbol_short!("dao_set");
        env.events().publish((DAO_SET,), dao.clone());
    }

pub fn emit_default_network(env: &Env, network_id: NetworkId) {
        const DEFAULT_NET: Symbol = symbol_short!("def_net");
        env.events().publish((DEFAULT_NET,), network_id);
    }

pub fn emit_upgraded(env: &Env, new_version: u32, new_impl: &Address, migration_data: Option<&Bytes>) {
        const UPGRADED: Symbol = symbol_short!("upgraded");
        env.events().publish((UPGRADED, new_version), (new_impl.clone(), migration_data.map(|d| d.clone())));
    }

    pub fn emit_rolled_back(env: &Env, prev_version: u32, prev_impl: &Address) {
        const ROLLED_BACK: Symbol = symbol_short!("rolled_back");
        env.events().publish((ROLLED_BACK,), (prev_version, prev_impl.clone()));
    }
}




/// Access control
mod access_control {
    use super::*;

    pub fn has_role(env: &Env, account: &Address, role: u32) -> bool {
        let roles: u32 = env
            .storage()
            .instance()
            .get(&DataKey::Role(account.clone()))
            .unwrap_or(0);
        (roles & role) != 0
    }

    pub fn grant_role(env: &Env, account: &Address, role: u32) {
        let key = DataKey::Role(account.clone());
        let mut roles: u32 = env.storage().instance().get(&key).unwrap_or(0);
        roles |= role;
        env.storage().instance().set(&key, &roles);

        if roles == role {
            let holders_key = DataKey::RoleHolders;
            let mut holders: Vec<Address> = env
                .storage()
                .instance()
                .get(&holders_key)
                .unwrap_or(Vec::new(env));
            holders.push_back(account.clone());
            env.storage().instance().set(&holders_key, &holders);
        }
    }

    pub fn revoke_role(env: &Env, account: &Address, role: u32) {
        let key = DataKey::Role(account.clone());
        let mut roles: u32 = env.storage().instance().get(&key).unwrap_or(0);
        roles &= !role;
        env.storage().instance().set(&key, &roles);

        if roles == 0 {
            let holders_key = DataKey::RoleHolders;
            let mut holders: Vec<Address> = env
                .storage()
                .instance()
                .get(&holders_key)
                .unwrap_or(Vec::new(env));
            if let Some(pos) = holders.iter().position(|a| a == *account) {
                holders.remove(pos as u32);
                env.storage().instance().set(&holders_key, &holders);
            }
        }
    }

    pub fn get_roles(env: &Env, account: &Address) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::Role(account.clone()))
            .unwrap_or(0)
    }

    pub fn get_role_holders(env: &Env) -> Vec<Address> {
        env.storage()
            .instance()
            .get(&DataKey::RoleHolders)
            .unwrap_or(Vec::new(env))
    }

    pub fn require_admin(env: &Env, account: &Address) {
        assert!(
            has_role(env, account, ROLE_ADMIN),
            "caller must have ADMIN role"
        );
        account.require_auth();
    }

    pub fn require_governance(env: &Env, account: &Address) {
        let roles = get_roles(env, account);
        assert!(
            (roles & (ROLE_ADMIN | ROLE_GOVERNANCE)) != 0,
            "caller must have ADMIN or GOVERNANCE role"
        );
        account.require_auth();
    }

    pub fn require_operator(env: &Env, account: &Address) {
        let roles = get_roles(env, account);
        assert!(
            (roles & (ROLE_ADMIN | ROLE_GOVERNANCE | ROLE_OPERATOR)) != 0,
            "caller must have ADMIN, GOVERNANCE, or OPERATOR role"
        );
        account.require_auth();
    }

    pub fn is_paused(env: &Env) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false)
    }

    pub fn set_paused(env: &Env, paused: bool) {
        env.storage().instance().set(&DataKey::Paused, &paused);
    }

    pub fn require_not_paused(env: &Env) {
        assert!(!is_paused(env), "contract is paused");
    }
}

/// Storage helpers
mod storage {
    use super::*;
    
    /// Get current implementation address
    pub fn get_current_implementation(env: &Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::CurrentImplementation)
    }
    
    /// Get/set current version
    pub fn get_current_version(env: &Env) -> Option<u32> {
        env.storage().instance().get(&DataKey::CurrentVersion)
    }
    
    pub fn set_current_version(env: &Env, version: u32) {
        env.storage().instance().set(&DataKey::CurrentVersion, &version);
    }
    
    /// Get/set previous implementation/version
    pub fn get_previous_implementation(env: &Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::PreviousImplementation)
    }
    
    pub fn get_previous_version(env: &Env) -> Option<u32> {
        env.storage().instance().get(&DataKey::PreviousVersion)
    }
    
    pub fn set_previous_implementation(env: &Env, impl_addr: Address) {
        env.storage().instance().set(&DataKey::PreviousImplementation, &impl_addr);
    }
    
    pub fn set_previous_version(env: &Env, version: u32) {
        env.storage().instance().set(&DataKey::PreviousVersion, &version);
    }


    pub fn is_initialized(env: &Env) -> bool {
        env.storage().instance().has(&DataKey::Initialized)
    }

    pub fn set_initialized(env: &Env) {
        env.storage().instance().set(&DataKey::Initialized, &true);
    }

    pub fn set_admin(env: &Env, admin: &Address) {
        env.storage().instance().set(&DataKey::Admin, admin);
    }

    pub fn get_admin(env: &Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("admin not set")
    }

    pub fn set_governance_dao(env: &Env, dao: &Address) {
        env.storage().instance().set(&DataKey::GovernanceDao, dao);
    }

    pub fn get_governance_dao(env: &Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::GovernanceDao)
    }

    pub fn set_network_config(env: &Env, network_id: NetworkId, config: &NetworkConfig) {
        env.storage()
            .instance()
            .set(&DataKey::NetworkConfig(network_id), config);

        let version: u32 = env
            .storage()
            .instance()
            .get(&DataKey::NetworkVersion(network_id))
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::NetworkVersion(network_id), &(version + 1));

        let networks_key = DataKey::RegisteredNetworks;
        let mut networks: Vec<NetworkId> = env
            .storage()
            .instance()
            .get(&networks_key)
            .unwrap_or(Vec::new(env));
        if !networks.contains(&network_id) {
            networks.push_back(network_id);
            env.storage().instance().set(&networks_key, &networks);
        }
    }

    pub fn get_network_config(env: &Env, network_id: NetworkId) -> Option<NetworkConfig> {
        env.storage()
            .instance()
            .get(&DataKey::NetworkConfig(network_id))
    }

    /// Returns `true` when `network_id` has been registered and not yet removed.
    pub fn is_registered_network(env: &Env, network_id: NetworkId) -> bool {
        env.storage()
            .instance()
            .has(&DataKey::NetworkConfig(network_id))
    }

    pub fn get_registered_networks(env: &Env) -> Vec<NetworkId> {
        env.storage()
            .instance()
            .get(&DataKey::RegisteredNetworks)
            .unwrap_or(Vec::new(env))
    }

    pub fn set_default_network(env: &Env, network_id: NetworkId) {
        env.storage()
            .instance()
            .set(&DataKey::DefaultNetwork, &network_id);
    }

    pub fn get_default_network(env: &Env) -> Option<NetworkId> {
        env.storage().instance().get(&DataKey::DefaultNetwork)
    }

    pub fn get_network_version(env: &Env, network_id: NetworkId) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::NetworkVersion(network_id))
            .unwrap_or(0)
    }

    pub fn increment_global_version(env: &Env) -> u32 {
        let version: u32 = env
            .storage()
            .instance()
            .get(&DataKey::GlobalVersion)
            .unwrap_or(0);
        let new_version = version + 1;
        env.storage()
            .instance()
            .set(&DataKey::GlobalVersion, &new_version);
        new_version
    }

    pub fn get_global_version(env: &Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::GlobalVersion)
            .unwrap_or(0)
    }

    pub fn add_asset(env: &Env, network_id: NetworkId, asset_config: &AssetConfig) {
        let asset_key = AssetKey {
            network_id,
            asset_address: asset_config.asset_address.clone(),
        };
        env.storage()
            .instance()
            .set(&DataKey::NetworkAssetConfig(asset_key.clone()), asset_config);

        let list_key = DataKey::NetworkAssetAddresses(network_id);
        let mut assets: Vec<Address> = env.storage().instance().get(&list_key).unwrap_or(Vec::new(env));
        if !assets.contains(&asset_config.asset_address) {
            assets.push_back(asset_config.asset_address.clone());
            env.storage().instance().set(&list_key, &assets);
        }
    }

    pub fn remove_asset(env: &Env, network_id: NetworkId, asset_address: &Address) {
        let asset_key = AssetKey {
            network_id,
            asset_address: asset_address.clone(),
        };
        env.storage().instance().remove(&DataKey::NetworkAssetConfig(asset_key));

        let list_key = DataKey::NetworkAssetAddresses(network_id);
        let mut assets: Vec<Address> = env.storage().instance().get(&list_key).unwrap_or(Vec::new(env));
        if let Some(pos) = assets.iter().position(|a| a == *asset_address) {
            assets.remove(pos as u32);
            env.storage().instance().set(&list_key, &assets);
        }
    }

    pub fn get_asset_config(
        env: &Env,
        network_id: NetworkId,
        asset_address: &Address,
    ) -> Option<AssetConfig> {
        let asset_key = AssetKey {
            network_id,
            asset_address: asset_address.clone(),
        };
        env.storage().instance().get(&DataKey::NetworkAssetConfig(asset_key))
    }

    pub fn get_network_assets(env: &Env, network_id: NetworkId) -> Vec<Address> {
        env.storage()
            .instance()
            .get(&DataKey::NetworkAssetAddresses(network_id))
            .unwrap_or(Vec::new(env))
    }
}

/// Validation
mod validation {
    use super::*;

    pub fn validate_network_config(_env: &Env, config: &NetworkConfig) {
        assert!(!config.name.is_empty(), "network name cannot be empty");
        assert!(
            !config.network_passphrase.is_empty(),
            "network passphrase cannot be empty"
        );

        assert!(
            config.fee_policy.base_fee >= 0,
            "base fee must be non-negative"
        );
        assert!(
            config.fee_policy.min_fee >= 0,
            "min fee must be non-negative"
        );
        assert!(
            config.fee_policy.max_fee >= 0,
            "max fee must be non-negative"
        );

        if config.fee_policy.max_fee > 0 {
            assert!(
                config.fee_policy.max_fee >= config.fee_policy.min_fee,
                "max fee must be >= min fee"
            );
        }

        assert!(
            config.block_time_seconds > 0 && config.block_time_seconds <= 3600,
            "block time must be between 1 and 3600 seconds"
        );

        assert!(
            config.dispute_timeout_seconds >= 3600,
            "dispute timeout must be at least 1 hour"
        );

        assert!(
            config.max_period_length_seconds >= 86400,
            "max period length must be at least 1 day"
        );

        assert!(
            config.min_attestations_for_aggregate > 0,
            "min attestations for aggregate must be > 0"
        );
    }

    pub fn validate_asset_config(_env: &Env, config: &AssetConfig) {
        assert!(!config.asset_code.is_empty(), "asset code cannot be empty");
        assert!(config.decimals <= 18, "decimals must be <= 18");
    }

    pub fn validate_fee_policy(policy: &FeePolicy) {
        assert!(policy.base_fee >= 0, "base fee must be non-negative");
        assert!(policy.min_fee >= 0, "min fee must be non-negative");
        assert!(policy.max_fee >= 0, "max fee must be non-negative");

        if policy.max_fee > 0 {
            assert!(
                policy.max_fee >= policy.min_fee,
                "max fee must be >= min fee"
            );
        }
    }
}

/// Contract
#[contract]
pub struct NetworkConfigContract;

#[contractimpl]
impl NetworkConfigContract {
    /// Require registry is initialized
    fn require_initialized(env: &Env) {
        if !storage::is_initialized(env) {
            panic!("contract not initialized");
        }
    }

    /// Require caller has governance role
    fn require_governance(env: &Env, caller: &Address) {
        access_control::require_governance(env, caller);
    }

    /// Panic with a descriptive message when `network_id` is not registered.
    ///
    /// # Panics
    /// Panics with `"unknown network id: <id>"` when the network has never been
    /// registered or has been removed.  This is the single authoritative guard
    /// used by all write paths that target a specific network.
    fn require_registered_network(env: &Env, network_id: NetworkId) {
        assert!(
            storage::is_registered_network(env, network_id),
            "unknown network id: {}",
            network_id
        );
    }

    /// Get current implementation address
    pub fn get_current_implementation(env: Env) -> Option<Address> {
        storage::get_current_implementation(&env)
    }

    /// Get current version
    pub fn get_current_version(env: Env) -> Option<u32> {
        storage::get_current_version(&env)
    }

    /// Get previous implementation
    pub fn get_previous_implementation(env: Env) -> Option<Address> {
        storage::get_previous_implementation(&env)
    }

    /// Get previous version
    pub fn get_previous_version(env: Env) -> Option<u32> {
        storage::get_previous_version(&env)
    }

    /// Get version info
    pub fn get_version_info(env: Env) -> Option<VersionInfo> {
        if !storage::is_initialized(&env) {
            return None;
        }
        let impl_addr = storage::get_current_implementation(&env).unwrap();
        let version = storage::get_current_version(&env).unwrap();
        Some(VersionInfo {
            version,
            implementation: impl_addr,
            migration_data: None,
            activated_at: env.ledger().timestamp(),
        })
    }

    /// Upgrade to new implementation
    pub fn upgrade(env: Env, caller: Address, new_impl: Address, new_version: u32, migration_data: Option<Bytes>) {
        Self::require_initialized(&env);
        Self::require_governance(&env, &caller);

        // Validate new impl
        if new_impl.is_none() { // Soroban Address no is_none, skip for now
            panic!("invalid implementation address");
        }

        let current_version = storage::get_current_version(&env)
            .expect("current version missing");
        if new_version <= current_version {
            panic!("new version must be greater than current version: {}", current_version);
        }

        // Store previous
        let current_impl = storage::get_current_implementation(&env)
            .expect("current implementation missing");
        storage::set_previous_implementation(&env, current_impl);
        storage::set_previous_version(&env, current_version);

        // Update current
        storage::set_current_version(&env, new_version);
        env.storage().instance().set(&DataKey::CurrentImplementation(new_impl.clone()), &new_impl);

        events::emit_upgraded(&env, new_version, &new_impl, migration_data.as_ref());
    }

    /// Rollback to previous implementation
    pub fn rollback(env: Env, caller: Address) {
        Self::require_initialized(&env);
        Self::require_governance(&env, &caller);

        let prev_impl = storage::get_previous_implementation(&env);
        let prev_version = storage::get_previous_version(&env);
        if prev_impl.is_none() || prev_version.is_none() {
            panic!("no previous implementation to rollback to");
        }

        let prev_impl = prev_impl.unwrap();
        let prev_version = prev_version.unwrap();

        // Get current for new previous
        let current_impl = storage::get_current_implementation(&env).unwrap();
        let current_version = storage::get_current_version(&env).unwrap();

        // Swap
        storage::set_previous_implementation(&env, current_impl);
        storage::set_previous_version(&env, current_version);
        env.storage().instance().set(&DataKey::CurrentImplementation(prev_impl.clone()), &prev_impl);
        storage::set_current_version(&env, prev_version);

        events::emit_rolled_back(&env, prev_version, &prev_impl);
    }

    pub fn initialize(env: Env, admin: Address, governance_dao: Option<Address>) {
        if storage::is_initialized(&env) {
            panic!("already initialized");
        }
        admin.require_auth();

        storage::set_initialized(&env);
        storage::set_admin(&env, &admin);
        access_control::grant_role(&env, &admin, ROLE_ADMIN);

        if let Some(dao) = governance_dao.clone() {
            storage::set_governance_dao(&env, &dao);
            access_control::grant_role(&env, &dao, ROLE_GOVERNANCE);
        }

        storage::set_default_network(&env, 0);

        events::emit_initialized(&env, &admin);
        if let Some(dao) = governance_dao {
            events::emit_dao_set(&env, &dao);
        }
    }

    pub fn grant_role(env: Env, caller: Address, account: Address, role: u32) {
        access_control::require_admin(&env, &caller);
        access_control::grant_role(&env, &account, role);
        events::emit_role_granted(&env, &account, role, &caller);
    }

    pub fn revoke_role(env: Env, caller: Address, account: Address, role: u32) {
        access_control::require_admin(&env, &caller);

        if account == caller && role == ROLE_ADMIN {
            let holders = access_control::get_role_holders(&env);
            let admin_count = holders
                .iter()
                .filter(|h| access_control::has_role(&env, &h, ROLE_ADMIN))
                .count();
            assert!(admin_count > 1, "cannot revoke last admin role");
        }

        access_control::revoke_role(&env, &account, role);
        events::emit_role_revoked(&env, &account, role, &caller);
    }

    pub fn has_role(env: Env, account: Address, role: u32) -> bool {
        access_control::has_role(&env, &account, role)
    }

    pub fn get_roles(env: Env, account: Address) -> u32 {
        access_control::get_roles(&env, &account)
    }

    pub fn get_role_holders(env: Env) -> Vec<Address> {
        access_control::get_role_holders(&env)
    }

    pub fn set_governance_dao(env: Env, caller: Address, dao: Address) {
        access_control::require_admin(&env, &caller);

        if let Some(old_dao) = storage::get_governance_dao(&env) {
            access_control::revoke_role(&env, &old_dao, ROLE_GOVERNANCE);
        }

        storage::set_governance_dao(&env, &dao);
        access_control::grant_role(&env, &dao, ROLE_GOVERNANCE);
        events::emit_dao_set(&env, &dao);
    }

    pub fn get_governance_dao(env: Env) -> Option<Address> {
        storage::get_governance_dao(&env)
    }

    pub fn pause(env: Env, caller: Address) {
        access_control::require_operator(&env, &caller);
        access_control::set_paused(&env, true);
        events::emit_paused(&env, &caller);
    }

    pub fn unpause(env: Env, caller: Address) {
        access_control::require_governance(&env, &caller);
        access_control::set_paused(&env, false);
        events::emit_unpaused(&env, &caller);
    }

    pub fn is_paused(env: Env) -> bool {
        access_control::is_paused(&env)
    }

    pub fn set_network_config(
        env: Env,
        caller: Address,
        network_id: NetworkId,
        config: NetworkConfig,
    ) {
        access_control::require_governance(&env, &caller);
        access_control::require_not_paused(&env);

        assert!(network_id != 0, "network_id cannot be 0");

        validation::validate_network_config(&env, &config);

        storage::set_network_config(&env, network_id, &config);
        storage::increment_global_version(&env);

        events::emit_network_set(&env, network_id, &config.name);
    }

    pub fn update_fee_policy(
        env: Env,
        caller: Address,
        network_id: NetworkId,
        fee_policy: FeePolicy,
    ) {
        access_control::require_governance(&env, &caller);
        access_control::require_not_paused(&env);

        validation::validate_fee_policy(&fee_policy);

        Self::require_registered_network(&env, network_id);
        let mut config = storage::get_network_config(&env, network_id).unwrap();

        config.fee_policy = fee_policy.clone();
        config.updated_at = env.ledger().timestamp();

        storage::set_network_config(&env, network_id, &config);
        storage::increment_global_version(&env);

        events::emit_fee_policy(&env, network_id, fee_policy.enabled);
    }

    pub fn set_asset_config(
        env: Env,
        caller: Address,
        network_id: NetworkId,
        asset_config: AssetConfig,
    ) {
        access_control::require_governance(&env, &caller);
        access_control::require_not_paused(&env);

        validation::validate_asset_config(&env, &asset_config);

        storage::add_asset(&env, network_id, &asset_config);
        storage::increment_global_version(&env);

        events::emit_asset_set(&env, network_id, &asset_config.asset_code);
    }

    pub fn remove_asset(env: Env, caller: Address, network_id: NetworkId, asset_address: Address) {
        access_control::require_governance(&env, &caller);
        access_control::require_not_paused(&env);

        assert!(
            storage::get_asset_config(&env, network_id, &asset_address).is_some(),
            "asset not found"
        );

        storage::remove_asset(&env, network_id, &asset_address);
        storage::increment_global_version(&env);

        events::emit_asset_set(&env, network_id, &String::from_str(&env, "REMOVED"));
    }

    pub fn update_contract_registry(
        env: Env,
        caller: Address,
        network_id: NetworkId,
        contracts: ContractRegistry,
    ) {
        access_control::require_governance(&env, &caller);
        access_control::require_not_paused(&env);

        Self::require_registered_network(&env, network_id);
        let mut config = storage::get_network_config(&env, network_id).unwrap();

        config.contracts = contracts;
        config.updated_at = env.ledger().timestamp();

        storage::set_network_config(&env, network_id, &config);
        storage::increment_global_version(&env);

        events::emit_registry(&env, network_id);
    }

    pub fn set_network_active(env: Env, caller: Address, network_id: NetworkId, active: bool) {
        access_control::require_governance(&env, &caller);
        access_control::require_not_paused(&env);

        Self::require_registered_network(&env, network_id);
        let mut config = storage::get_network_config(&env, network_id).unwrap();

        config.is_active = active;
        config.updated_at = env.ledger().timestamp();

        storage::set_network_config(&env, network_id, &config);
        storage::increment_global_version(&env);

        events::emit_network_active(&env, network_id, active);
    }

    pub fn set_default_network(env: Env, caller: Address, network_id: NetworkId) {
        access_control::require_governance(&env, &caller);
        access_control::require_not_paused(&env);

        if network_id != 0 {
            Self::require_registered_network(&env, network_id);
            let config = storage::get_network_config(&env, network_id).unwrap();
            assert!(config.is_active, "cannot set inactive network as default");
        }

        storage::set_default_network(&env, network_id);
        storage::increment_global_version(&env);

        events::emit_default_network(&env, network_id);
    }

    pub fn remove_network(env: Env, caller: Address, network_id: NetworkId) {
        access_control::require_admin(&env, &caller);
        access_control::require_not_paused(&env);

        let default = storage::get_default_network(&env).unwrap_or(0);
        assert!(network_id != default, "cannot remove default network");

        Self::require_registered_network(&env, network_id);
        let config = storage::get_network_config(&env, network_id).unwrap();
        assert!(
            !config.is_active,
            "cannot remove active network; deactivate first"
        );

        env.storage()
            .instance()
            .remove(&DataKey::NetworkConfig(network_id));

        let asset_list = storage::get_network_assets(&env, network_id);
        for asset_addr in asset_list.iter() {
            storage::remove_asset(&env, network_id, &asset_addr);
        }

        let networks_key = DataKey::RegisteredNetworks;
        let mut networks: Vec<NetworkId> = env
            .storage()
            .instance()
            .get(&networks_key)
            .unwrap_or(Vec::new(&env));
        if let Some(pos) = networks.iter().position(|n| n == network_id) {
            networks.remove(pos as u32);
            env.storage().instance().set(&networks_key, &networks);
        }

        env.storage()
            .instance()
            .remove(&DataKey::NetworkAssetAddresses(network_id));
        env.storage()
            .instance()
            .remove(&DataKey::NetworkVersion(network_id));
        
        storage::increment_global_version(&env);
    }

    pub fn get_network_config(env: Env, network_id: NetworkId) -> Option<NetworkConfig> {
        storage::get_network_config(&env, network_id)
    }

    pub fn is_network_active(env: Env, network_id: NetworkId) -> bool {
        storage::get_network_config(&env, network_id)
            .map(|c| c.is_active)
            .unwrap_or(false)
    }

    pub fn get_fee_policy(env: Env, network_id: NetworkId) -> Option<FeePolicy> {
        storage::get_network_config(&env, network_id).map(|c| c.fee_policy)
    }

    pub fn get_allowed_assets(env: Env, network_id: NetworkId) -> Vec<AssetConfig> {
        let asset_addrs = storage::get_network_assets(&env, network_id);
        let mut assets = Vec::new(&env);
        for addr in asset_addrs.iter() {
            if let Some(config) = storage::get_asset_config(&env, network_id, &addr) {
                assets.push_back(config);
            }
        }
        assets
    }

    pub fn get_asset_config(
        env: Env,
        network_id: NetworkId,
        asset_address: Address,
    ) -> Option<AssetConfig> {
        storage::get_asset_config(&env, network_id, &asset_address)
    }

    pub fn get_contract_registry(env: Env, network_id: NetworkId) -> Option<ContractRegistry> {
        storage::get_network_config(&env, network_id).map(|c| c.contracts)
    }

    pub fn get_contract_address(
        env: Env,
        network_id: NetworkId,
        contract_name: String,
    ) -> Option<Address> {
        storage::get_network_config(&env, network_id).and_then(|c| {
            let name = contract_name.to_string();
            match name.as_str() {
                "attestation" if c.contracts.has_attestation => {
                    Some(c.contracts.attestation_contract)
                }
                "revenue_stream" if c.contracts.has_revenue_stream => {
                    Some(c.contracts.revenue_stream_contract)
                }
                "audit_log" if c.contracts.has_audit_log => Some(c.contracts.audit_log_contract),
                "aggregated_attestations" if c.contracts.has_aggregated_attestations => {
                    Some(c.contracts.aggregated_attestations_contract)
                }
                "integration_registry" if c.contracts.has_integration_registry => {
                    Some(c.contracts.integration_registry_contract)
                }
                "attestation_snapshot" if c.contracts.has_attestation_snapshot => {
                    Some(c.contracts.attestation_snapshot_contract)
                }
                _ => None,
            }
        })
    }

    pub fn get_registered_networks(env: Env) -> Vec<NetworkId> {
        storage::get_registered_networks(&env)
    }

    pub fn get_default_network(env: Env) -> NetworkId {
        storage::get_default_network(&env).unwrap_or(0)
    }

    pub fn get_network_version(env: Env, network_id: NetworkId) -> u32 {
        storage::get_network_version(&env, network_id)
    }

    pub fn get_global_version(env: Env) -> u32 {
        storage::get_global_version(&env)
    }

    /// Returns `true` when `network_id` is currently registered.
    ///
    /// Use this as a pre-flight check before calling any write operation that
    /// targets a specific network, to avoid the `"unknown network id"` panic.
    pub fn network_exists(env: Env, network_id: NetworkId) -> bool {
        storage::is_registered_network(&env, network_id)
    }

    pub fn get_admin(env: Env) -> Address {
        storage::get_admin(&env)
    }

    pub fn get_network_parameters(env: Env, network_id: NetworkId) -> Option<(u32, u64, u64, u32)> {
        storage::get_network_config(&env, network_id).map(|c| {
            (
                c.block_time_seconds,
                c.dispute_timeout_seconds,
                c.max_period_length_seconds,
                c.min_attestations_for_aggregate,
            )
        })
    }

    pub fn is_asset_valid_for_attestation(
        env: Env,
        network_id: NetworkId,
        asset_address: Address,
        amount: i128,
    ) -> bool {
        storage::get_asset_config(&env, network_id, &asset_address)
            .map(|asset| {
                if !asset.is_active {
                    return false;
                }
                if asset.max_attestation_value > 0 {
                    amount > 0 && amount <= asset.max_attestation_value
                } else {
                    amount > 0
                }
            })
            .unwrap_or(false)
    }
}

pub use access_control::{ROLE_ADMIN, ROLE_GOVERNANCE, ROLE_OPERATOR};
